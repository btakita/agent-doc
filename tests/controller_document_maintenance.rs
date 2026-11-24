//! `#q-e9483473162d`: the project controller owns per-document background
//! maintenance. With a controller running, preflight admission publishes the
//! document's maintenance request and only notifies the controller, which runs
//! it on its own worker thread (no detached `preflight --queue-maintenance-only`
//! process). The controller's reconciler keeps one `ReconcileCache` per
//! document and runs the queue/closeout chain through `reconcile_incremental`;
//! every chain run is checked byte-identical to `reconcile_full`
//! (`AGENT_DOC_TEST_RECONCILE_CHAIN_VERIFY_FULL`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;

const CONTROLLER_LAUNCH_REASON: &str = "controller_document_maintenance";

fn agent_doc() -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env("AGENT_DOC_TEST_RECONCILE_CHAIN_VERIFY_FULL", "1")
        .env_remove("AGENT_DOC_BACKGROUND_MAINTENANCE_HOST")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("AGENT_DOC_TURN_STATUS_ROOT");
    command
}

struct ControllerGuard {
    project_root: PathBuf,
}

impl Drop for ControllerGuard {
    fn drop(&mut self) {
        let _ = agent_doc()
            .args([
                "controller",
                "shutdown",
                "--project-root",
                &self.project_root.to_string_lossy(),
            ])
            .output();
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// A session document whose queue the chain changes: the open-count fixture
/// (top-level rows whose open-work counts are stale) with an exchange.
fn session_document() -> String {
    let fixture = include_str!("../agent-doc-queue/tests/fixtures/open_count_session.md");
    format!(
        "{fixture}\n## Exchange <!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n"
    )
}

fn ops_log(root: &Path) -> String {
    std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default()
}

fn wait_for_ops(root: &Path, from: usize, what: &str, done: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let ops = ops_log(root);
        let appended = ops.get(from..).unwrap_or_default().to_string();
        if done(&appended) {
            return appended;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}\n--- ops.log (appended) ---\n{appended}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn controller_maintenance_finished(appended: &str) -> bool {
    appended.lines().any(|line| {
        line.contains("queue_maintenance_background_exited") && line.contains("mode=controller")
    })
}

fn preflight(root: &Path, doc: &Path) {
    let output = agent_doc()
        .current_dir(root)
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_controller_cycle(appended: &str) {
    assert!(
        appended.contains("queue_maintenance_controller_enqueued"),
        "admission did not hand the request to the controller:\n{appended}"
    );
    assert!(
        !appended.contains("queue_maintenance_background_dispatched")
            && !appended.contains("queue_maintenance_controller_unavailable"),
        "admission spawned a maintenance process although a controller was running:\n{appended}"
    );
    assert!(
        appended.lines().any(|line| {
            line.contains("queue_maintenance_background_exited")
                && line.contains("mode=controller")
                && line.contains("outcome=ok")
        }),
        "controller-hosted maintenance did not finish cleanly:\n{appended}"
    );
    let chain: Vec<&str> = appended
        .lines()
        .filter(|line| line.contains("queue_reconcile_chain file="))
        .collect();
    assert!(
        !chain.is_empty(),
        "the controller's reconciler did not run the queue chain:\n{appended}"
    );
    for line in chain {
        assert!(
            line.contains("full_equal=true"),
            "the incremental chain diverged from reconcile_full: {line}"
        );
    }
}

#[test]
fn the_controller_runs_document_maintenance_and_the_incremental_chain_equals_the_full_chain() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let doc = root.join("tasks/session.md");
    let content = session_document();
    std::fs::write(&doc, &content).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(
        &root,
        &["config", "user.name", "Controller Maintenance Test"],
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "initial", "--no-verify"]);

    // Precondition: the fixture is not already reconciled, so the controller
    // run has a chain cut to persist.
    let full = {
        let no = |_: &str| false;
        let not_live = |_: &str, _: u64| false;
        agent_doc_queue::reconcile_incremental::reconcile_full(
            &content,
            &agent_doc_queue::reconcile_incremental::ReconcileInputs {
                is_live: &not_live,
                response_proven: &no,
                struck_provenance: &no,
                is_claimed: &no,
            },
        )
    };
    assert_ne!(full.document, content, "the fixture must have chain work");

    let status = agent_doc()
        .args([
            "controller",
            "status",
            "--project-root",
            &root.to_string_lossy(),
            "--ensure",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "controller did not start: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let _controller = ControllerGuard {
        project_root: root.clone(),
    };

    // First admission: the controller reserves, runs, and persists the chain.
    let from = ops_log(&root).len();
    preflight(&root, &doc);
    let first = wait_for_ops(&root, from, "the first controller run", |appended| {
        controller_maintenance_finished(appended)
    });
    assert_controller_cycle(&first);
    assert!(
        first
            .lines()
            .any(|line| line.contains("queue_reconcile_chain file=")
                && line.contains("changed=true")),
        "the chain had work on the first revision:\n{first}"
    );
    assert!(
        !agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_pending(&doc),
        "the controller run must consume the request"
    );

    // Close the first cycle the way a turn does, then admit an operator edit:
    // the same controller worker runs again with the document's cache.
    let finalize = agent_doc()
        .current_dir(&root)
        .args(["finalize", "--template", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: counts\n\nCounts checked.\n<!-- /patch:exchange -->\n\
             <!-- patch:recap -->\nCounts checked.\n<!-- /patch:recap -->\n",
        )
        .output()
        .unwrap();
    // The fixture's queue rows are live work this test does not drain, so
    // the strict closeout's session check reports them after it commits the
    // cycle; only the commit matters here.
    let finalize_stderr = String::from_utf8_lossy(&finalize.stderr);
    assert!(
        finalize.status.success() || finalize_stderr.contains("closeout committed cycle"),
        "finalize did not commit the cycle: {finalize_stderr}"
    );
    let edited = std::fs::read_to_string(&doc).unwrap().replace(
        "### Re: prior\n\nDone.\n",
        "### Re: prior\n\nDone.\n\nOperator follow-up: check the counts.\n",
    );
    std::fs::write(&doc, &edited).unwrap();
    let from = ops_log(&root).len();
    preflight(&root, &doc);
    let second = wait_for_ops(&root, from, "the second controller run", |appended| {
        controller_maintenance_finished(appended)
    });
    assert_controller_cycle(&second);
    let settled = std::fs::read_to_string(&doc).unwrap();
    assert!(
        settled.contains("Operator follow-up: check the counts."),
        "background maintenance must keep the operator's edit"
    );

    // `#incrementalreconcile`: no editor is attached, so the document is
    // git-authoritative and the controller hosts no CRDT canonical for it:
    // the chain finds the changeset by comparing the texts (the attached
    // case is `an_attached_document_reconciles_from_the_canonical_journal`).
    assert!(
        second
            .lines()
            .filter(|line| line.contains("queue_reconcile_chain file="))
            .all(|line| line.contains(" crdt=no_hub ") && line.contains(" changeset=byte_compare ")),
        "{second}"
    );
}
