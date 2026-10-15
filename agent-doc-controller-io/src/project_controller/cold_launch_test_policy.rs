//! `#coldlaunchflakes`: which lib-test launches may start a real controller
//! process.
//!
//! Lib tests that reached [`connect_or_launch`](super::connect_or_launch) with
//! no controller running each spawned a separate `agent-doc controller serve`
//! (`AGENT_DOC_BIN` under `make test`, otherwise whatever `agent-doc` came first
//! on `PATH`) and waited a fixed test-mode budget for its socket. A workspace
//! run measured 498 such launches from 17 lib-test binaries (2026-10-15). Under
//! load the child was alive but had not yet bound, and the test failed with
//! "timed out waiting for launched controller pid N" while asserting nothing
//! about startup. The verdict also depended on a binary other than the code
//! under test. Those tests now get this process's in-process state actor
//! ([`ensure_state_actor_for_tests`](super::ensure_state_actor_for_tests)),
//! which runs the same RPC handlers with no process boundary and no launch
//! clock. A test whose subject IS the process launch opts in with
//! [`allow_cold_controller_launch_for_tests`](super::allow_cold_controller_launch_for_tests).

use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

static OPTED_IN_ROOTS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn key(project_root: &Path) -> PathBuf {
    project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
}

pub(super) fn allow(project_root: &Path) {
    let mut roots = OPTED_IN_ROOTS.lock();
    roots.insert(project_root.to_path_buf());
    roots.insert(key(project_root));
}

fn allowed(project_root: &Path) -> bool {
    let roots = OPTED_IN_ROOTS.lock();
    !roots.is_empty() && (roots.contains(project_root) || roots.contains(&key(project_root)))
}

/// A workspace crate's unit-test harness: `target/<profile>/deps/agent_doc*-<hash>`.
///
/// The `agent-doc` binary itself (a launched controller, or the CLI an
/// integration test runs) is named with a hyphen and is never matched, so
/// a `test-support` build of the binary launches exactly as production does.
pub(super) fn exe_is_workspace_lib_test(exe: &Path) -> bool {
    exe.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("agent_doc"))
}

pub(super) fn running_in_workspace_lib_test() -> bool {
    std::env::current_exe()
        .ok()
        .is_some_and(|exe| exe_is_workspace_lib_test(&exe))
}

/// Whether a lib test that did not opt in reached a controller launch for
/// `project_root`.
pub(super) fn unopted_lib_test_launch(project_root: &Path) -> bool {
    running_in_workspace_lib_test() && !allowed(project_root)
}

/// The `#coldlaunchflakes` guard itself. A later change that adds a launch path
/// bypassing [`super::launch_detached_at`], or stops classifying lib-test
/// harnesses, reintroduces the startup race silently; these pin both halves.
#[cfg(test)]
mod tests {
    use super::super::{LaunchMode, connect_or_launch, launch_detached, status};
    use super::{exe_is_workspace_lib_test, running_in_workspace_lib_test};
    use std::path::Path;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    fn controller_stderr_log(root: &Path) -> std::path::PathBuf {
        root.join(".agent-doc/logs/controller-stderr.log")
    }

    #[test]
    fn lib_test_harnesses_are_classified_and_the_binary_is_not() {
        for harness in [
            "/w/target/debug/deps/agent_doc-3fc46b27add6fc7c",
            "/w/target/debug/deps/agent_doc_preflight_io-515bd79334718de9",
            "/w/target/debug/deps/agent_doc_queue_io-88c36bec573ccca4",
        ] {
            assert!(exe_is_workspace_lib_test(Path::new(harness)), "{harness}");
        }
        for not_a_lib_test in [
            // A launched controller and the CLI integration tests spawn.
            "/w/target/debug/agent-doc",
            "/usr/local/bin/agent-doc",
            // Integration-test harnesses are named after their `tests/*.rs` file.
            "/w/target/debug/deps/test_cli-0123456789abcdef",
        ] {
            assert!(
                !exe_is_workspace_lib_test(Path::new(not_a_lib_test)),
                "{not_a_lib_test}"
            );
        }
        assert!(
            running_in_workspace_lib_test(),
            "this harness is agent-doc-controller-io's lib test and must be classified as one"
        );
    }

    #[test]
    fn an_unopted_lib_test_cannot_spawn_a_controller_process() {
        let dir = project();
        let refused = std::panic::catch_unwind(|| {
            let _ = launch_detached(dir.path(), LaunchMode::Lazy);
        })
        .expect_err("an unopted lib test reached a real controller spawn");
        let message = refused
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| refused.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        assert!(message.contains("#coldlaunchflakes"), "{message}");
        assert!(
            !controller_stderr_log(dir.path()).exists(),
            "the refusal must precede any spawn side effect"
        );
    }

    #[test]
    fn an_unopted_connect_or_launch_is_served_by_this_process() {
        let dir = project();
        connect_or_launch(dir.path(), LaunchMode::Lazy).expect("in-process controller");
        assert!(
            !controller_stderr_log(dir.path()).exists(),
            "no detached controller may be launched for an unopted lib test"
        );
        let served = status(dir.path()).expect("status from the in-process actor");
        assert!(served.active, "{served:?}");
        assert_eq!(
            served.pid,
            Some(std::process::id()),
            "the controller answering a lib test must be this process"
        );
    }
}
