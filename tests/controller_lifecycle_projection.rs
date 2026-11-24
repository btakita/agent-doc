//! `#q-9f8f5f60c2c3` (preflightlag goal 4): with a project controller running,
//! a ledger-only lifecycle transition (`queue freeze`) is repainted on its row
//! by the controller's background worker right away, not at the next cycle,
//! and the freezing command itself only publishes and notifies.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;

const CONTROLLER_LAUNCH_REASON: &str = "controller_lifecycle_projection";

fn agent_doc() -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env_remove(agent_doc_queue_io::lifecycle_projection::LIFECYCLE_PROJECTION_HOST_ENV)
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

fn row(doc: &Path, needle: &str) -> String {
    std::fs::read_to_string(doc)
        .unwrap()
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_default()
        .to_string()
}

fn run_ok(root: &Path, args: &[&str]) {
    let output = agent_doc().current_dir(root).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "agent-doc {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const SESSION: &str = "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n- check the first count [#a]\n- check the second count [#b]\n<!-- /agent:queue -->\n";

#[test]
fn a_freeze_is_repainted_by_the_controller_at_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let doc = root.join("tasks/session.md");
    std::fs::write(&doc, SESSION).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "Lifecycle Projection Test"]);
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

    let file = doc.to_str().unwrap();
    run_ok(
        &root,
        &[
            "queue",
            "claim",
            file,
            "--item",
            "#a",
            "--owner",
            "subagent:a",
        ],
    );
    run_ok(
        &root,
        &[
            "queue",
            "start",
            file,
            "--item",
            "#a",
            "--owner",
            "subagent:a",
            "--worker-id",
            "worker-a",
            "--generation",
            "1",
        ],
    );
    let running = agent_doc_document::queue_projection::QueueLifecycleMarker::Running.prefix();
    let lost = agent_doc_document::queue_projection::QueueLifecycleMarker::Lost.prefix();
    assert!(row(&doc, "[#a]").contains(running), "{}", row(&doc, "[#a]"));

    let from = ops_log(&root).len();
    run_ok(
        &root,
        &[
            "queue",
            "freeze",
            file,
            "--item",
            "#a",
            "--generation",
            "1",
            "--reason",
            "worker stopped",
        ],
    );
    let appended = wait_for_ops(
        &root,
        from,
        "the controller's lifecycle projection",
        |appended| {
            appended.lines().any(|line| {
                line.contains("queue_lifecycle_projection_ran")
                    && line.contains("runner=controller")
            })
        },
    );
    assert!(
        appended.contains("queue_lifecycle_projection_deferred"),
        "the freeze did not hand its projection to the controller:\n{appended}"
    );
    assert!(
        !appended.contains("queue_lifecycle_projection_controller_unavailable"),
        "the freeze projected inline although a controller was running:\n{appended}"
    );
    let painted = row(&doc, "[#a]");
    assert!(
        painted.contains(lost),
        "frozen row not repainted: {painted}"
    );
    assert!(!painted.contains(running), "{painted}");
    assert!(
        !row(&doc, "[#b]").contains(lost),
        "only the frozen row changes: {}",
        row(&doc, "[#b]")
    );
    assert!(
        agent_doc_queue_io::lifecycle_projection::load_pending(&doc)
            .unwrap()
            .is_none(),
        "the controller run consumes the pending projection"
    );
}
