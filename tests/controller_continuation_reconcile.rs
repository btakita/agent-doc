//! `#q-c71d106debc4`: with a project controller running, a closeout hands its
//! queue-continuation reconcile to the controller's background worker instead
//! of running it inline, and the continuation marker still ends up exactly as
//! the inline reconcile would leave it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;

const CONTROLLER_LAUNCH_REASON: &str = "controller_continuation_reconcile";

fn agent_doc() -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env_remove(agent_doc_queue_io::continuation_reconcile::CONTINUATION_RECONCILE_HOST_ENV)
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

const SESSION: &str = "---\nsession: sid\nagent_doc_format: template\nqueue: start\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto go -->\n- check the first count [#a]\n- check the second count [#b]\n<!-- /agent:queue -->\n";

#[test]
fn a_closeout_reconciles_queue_continuation_on_the_controller() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let doc = root.join("tasks/session.md");
    std::fs::write(&doc, SESSION).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(
        &root,
        &["config", "user.name", "Continuation Reconcile Test"],
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "initial", "--no-verify"]);

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

    let preflight = agent_doc()
        .current_dir(&root)
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        preflight.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&preflight.stderr)
    );

    let from = ops_log(&root).len();
    let finalize = agent_doc()
        .current_dir(&root)
        .args(["finalize", "--template", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: first count\n\n> **Queue prompt:** check the first count [#a]\n\n\
             The first count is checked: it matches.\n<!-- /patch:exchange -->\n\
             <!-- patch:recap -->\nFirst count checked.\n<!-- /patch:recap -->\n",
        )
        .output()
        .unwrap();
    let finalize_stderr = String::from_utf8_lossy(&finalize.stderr);
    assert!(
        finalize.status.success() || finalize_stderr.contains("closeout committed cycle"),
        "finalize did not commit the cycle: {finalize_stderr}"
    );

    let appended = wait_for_ops(&root, from, "the controller's reconcile", |appended| {
        appended.lines().any(|line| {
            line.contains("queue_continuation_reconcile_ran") && line.contains("runner=controller")
        })
    });
    assert!(
        appended.contains("queue_continuation_reconcile_deferred"),
        "the closeout did not hand its reconcile to the controller:\n{appended}"
    );
    assert!(
        !appended.contains("queue_continuation_reconcile_controller_unavailable"),
        "the closeout reconciled inline although a controller was running:\n{appended}"
    );
    let finalize_line = appended
        .lines()
        .find(|line| line.contains("commit_finalize_latency"))
        .expect("finalize latency line");
    assert!(
        finalize_line.contains("queue:"),
        "unexpected finalize line: {finalize_line}"
    );

    assert!(
        agent_doc_queue_io::continuation_reconcile::load_pending(&doc)
            .unwrap()
            .is_none(),
        "the controller run consumes the pending reconcile"
    );
    // The marker is what the inline reconcile would leave: present exactly
    // when the committed document still owes a continuation.
    let marker = agent_doc_queue_io::continuation_marker::load_continuation_marker(&doc).unwrap();
    let owed = agent_doc_queue_io::queue_continuation::detect(&doc).unwrap();
    assert!(
        owed.is_some(),
        "the second row is still owed after the first closes:\n{}",
        std::fs::read_to_string(&doc).unwrap()
    );
    assert_eq!(
        marker.as_ref().map(|marker| marker.head_prompt.clone()),
        owed.as_ref().map(|owed| owed.head_prompt.clone()),
        "marker {marker:?} vs detected continuation {owed:?}\n{appended}"
    );
}
