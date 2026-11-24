//! `#q-1d2a127f32aa`: with a project controller running, a closeout refreshes
//! the controller's per-revision preflight cache in the background, and the
//! next preflight's `repair` and `stuck_cycle_detection` phases read its
//! verdicts instead of running their recovery reads.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;

const CONTROLLER_LAUNCH_REASON: &str = "controller_preflight_revision_cache";

fn agent_doc() -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env_remove(agent_doc_queue_io::continuation_reconcile::CONTINUATION_RECONCILE_HOST_ENV)
        .env_remove(agent_doc_preflight_command_io::revision_cache::PREFLIGHT_REVISION_CACHE_ENV)
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
fn the_next_preflight_reads_the_controllers_repair_and_stuck_verdicts() {
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
        &["config", "user.name", "Preflight Revision Cache Test"],
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

    let preflight = |label: &str| {
        let output = agent_doc()
            .current_dir(&root)
            .args(["preflight", doc.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{label} preflight failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    preflight("first");

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
    wait_for_ops(
        &root,
        from,
        "the controller's revision-cache refresh",
        |appended| {
            appended
                .lines()
                .any(|line| line.contains("preflight_revision_cache_refreshed"))
        },
    );

    let from = ops_log(&root).len();
    preflight("second");
    let appended = ops_log(&root).get(from..).unwrap_or_default().to_string();
    let repair = appended
        .lines()
        .find(|line| line.contains("preflight_revision_cache_repair"))
        .unwrap_or_else(|| panic!("the repair phase did not read the verdict:\n{appended}"));
    assert!(
        repair.contains("served=true") && repair.contains("cycle_open=Some(false)"),
        "a cleanly closed prior cycle is served from the cache: {repair}"
    );
    let stuck = appended
        .lines()
        .find(|line| line.contains("preflight_revision_cache_stuck"))
        .unwrap_or_else(|| panic!("the stuck phase did not read the verdict:\n{appended}"));
    assert!(
        stuck.contains("served=true"),
        "the committed response is in HEAD, so the sweep is served from the cache: {stuck}"
    );
    assert!(
        !appended.contains("preflight_revision_cache_unavailable"),
        "the running controller answered both reads:\n{appended}"
    );
}
