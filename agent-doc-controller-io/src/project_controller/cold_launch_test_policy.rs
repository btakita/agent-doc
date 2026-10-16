//! `#coldlaunchflakes` / `#integrationtesttargets`: which test launches may
//! start a real controller process.
//!
//! Tests that reached [`connect_or_launch`](super::connect_or_launch) with no
//! controller running each spawned a separate `agent-doc controller serve`
//! (`AGENT_DOC_BIN` under `make test`, otherwise whatever `agent-doc` came first
//! on `PATH`) and waited a fixed test-mode budget for its socket. A workspace
//! run measured 498 such launches from 17 lib-test binaries (2026-10-15), and a
//! second count found 546 more from the integration-test targets (`tests/*.rs`):
//! 403 from integration-test harnesses calling library code in-process and 143
//! from the `agent-doc` CLI those targets run as a subprocess. Under load the
//! child was alive but had not yet bound, and the test failed with "timed out
//! waiting for launched controller pid N" while asserting nothing about
//! startup.
//!
//! The policy has two halves, keyed on who is launching:
//!
//! - **A workspace test harness** (a lib test or an integration test,
//!   `target/<profile>/deps/<name>-<hash>`) is served by this process's
//!   in-process state actor
//!   ([`ensure_state_actor_for_tests`](super::ensure_state_actor_for_tests)),
//!   which runs the same RPC handlers with no process boundary and no launch
//!   clock. A test whose subject IS the process launch opts in with
//!   [`allow_cold_controller_launch_for_tests`](super::allow_cold_controller_launch_for_tests).
//! - **A process a test harness spawned** (the `agent-doc` CLI an integration
//!   test drives, or a controller that CLI launched) is real behavior: the
//!   launch is production code. It may launch only when the test opted in by
//!   setting [`TEST_CONTROLLER_LAUNCH_REASON_ENV`] to a non-empty reason, and
//!   its wait for the child is readiness-based (socket published or child
//!   exited) with only a hang ceiling. A launch without a reason panics, so a
//!   new target that starts launching controllers fails instead of adding
//!   unbounded cold launches.

use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The opt-in an integration test sets on the `agent-doc` command it spawns
/// when that command (or a controller it launches) may cold-launch a real
/// controller process. The value is the reason, and it must be non-empty.
/// Children inherit it, so a launched controller's own replacement launches
/// stay opted in.
pub const TEST_CONTROLLER_LAUNCH_REASON_ENV: &str = "AGENT_DOC_TEST_CONTROLLER_LAUNCH_REASON";

/// The reason handed to a controller a harness launched after
/// [`allow`]: its own launches (a handoff replacement) inherit the opt-in.
const HARNESS_OPT_IN_REASON: &str = "allow_cold_controller_launch_for_tests";

/// How many ancestors [`spawned_under_workspace_test_harness`] inspects. A CLI
/// an integration test spawns is one hop below the harness; a controller that
/// CLI launched is two. The bound keeps the walk cheap and finite.
const MAX_ANCESTOR_HOPS: usize = 8;

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

/// A workspace crate's test harness: `target/<profile>/deps/<name>-<hash>`,
/// where `<hash>` is Cargo's 16-hex-digit metadata hash. This covers lib-test
/// harnesses (`agent_doc_queue_io-<hash>`) and integration-test harnesses,
/// which are named after their `tests/*.rs` file (`test_cli-<hash>`).
///
/// The `agent-doc` binary itself (a launched controller, or the CLI an
/// integration test runs) is uplifted to `target/<profile>/agent-doc` or
/// installed under a `bin/` directory, and is never matched.
pub(super) fn exe_is_workspace_test_harness(exe: &Path) -> bool {
    let in_deps = exe
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|dir| dir == "deps");
    let named_like_a_harness = exe
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('-'))
        .is_some_and(|(stem, hash)| {
            !stem.is_empty()
                && hash.len() == 16
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        });
    in_deps && named_like_a_harness
}

pub(super) fn running_in_workspace_test_harness() -> bool {
    static HARNESS: OnceLock<bool> = OnceLock::new();
    *HARNESS.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .is_some_and(|exe| exe_is_workspace_test_harness(&exe))
    })
}

/// The parent pid in one `/proc/<pid>/stat` line. `comm` (field 2) may hold
/// spaces and parentheses, so fields are counted after its last `)`.
fn parent_pid_from_stat(stat: &str) -> Option<u32> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    // Fields after `comm`: state (3), ppid (4).
    after_comm.split_whitespace().nth(1)?.parse().ok()
}

/// Whether a workspace test harness is among this process's ancestors: this
/// process is the `agent-doc` CLI an integration test spawned, or a controller
/// that CLI launched while it was still running. Linux only; elsewhere, and for
/// a CLI started inside a tmux pane (whose parent is the tmux server), no
/// harness is found and the launch is unguarded.
pub(super) fn spawned_under_workspace_test_harness() -> bool {
    static SPAWNED: OnceLock<bool> = OnceLock::new();
    *SPAWNED.get_or_init(|| {
        #[cfg(target_os = "linux")]
        {
            let mut pid = std::os::unix::process::parent_id();
            for _ in 0..MAX_ANCESTOR_HOPS {
                if pid <= 1 {
                    return false;
                }
                if std::fs::read_link(format!("/proc/{pid}/exe"))
                    .is_ok_and(|exe| exe_is_workspace_test_harness(&exe))
                {
                    return true;
                }
                let Some(parent) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .as_deref()
                    .and_then(parent_pid_from_stat)
                else {
                    return false;
                };
                pid = parent;
            }
            false
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    })
}

fn env_opt_in_reason() -> Option<String> {
    std::env::var(TEST_CONTROLLER_LAUNCH_REASON_ENV)
        .ok()
        .map(|reason| reason.trim().to_string())
        .filter(|reason| !reason.is_empty())
}

/// Who is asking to launch a controller, as the policy sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TestLauncher {
    /// A workspace test harness: served in-process unless it opted in.
    Harness,
    /// A process a test harness spawned: real launches need an env reason.
    SpawnedByHarness,
    /// Not under a workspace test.
    NotUnderTest,
}

pub(super) fn test_launcher() -> TestLauncher {
    if running_in_workspace_test_harness() {
        TestLauncher::Harness
    } else if spawned_under_workspace_test_harness() {
        TestLauncher::SpawnedByHarness
    } else {
        TestLauncher::NotUnderTest
    }
}

/// Whether a test harness that did not opt in reached a controller launch for
/// `project_root`; [`super::connect_or_launch`] serves it in-process instead.
pub(super) fn unopted_harness_launch(project_root: &Path) -> bool {
    test_launcher() == TestLauncher::Harness && !allowed(project_root)
}

/// Why a launch for `project_root` is refused under the test policy, or `None`
/// when it may proceed.
pub(super) fn refused_test_launch(project_root: &Path) -> Option<&'static str> {
    match test_launcher() {
        TestLauncher::Harness if !allowed(project_root) => Some(
            "a test harness tried to launch a detached `agent-doc controller serve` without opting in. \
             Serve the root in-process with ensure_state_actor_for_tests, or call \
             allow_cold_controller_launch_for_tests if the process launch is what the test exercises.",
        ),
        TestLauncher::SpawnedByHarness if env_opt_in_reason().is_none() => Some(
            "an `agent-doc` process spawned by a test harness tried to launch a detached \
             `agent-doc controller serve`, but the test did not opt in. If the launch is \
             production behavior the test drives, set AGENT_DOC_TEST_CONTROLLER_LAUNCH_REASON \
             to the reason on the spawned command; otherwise keep the command off the controller.",
        ),
        _ => None,
    }
}

/// The opt-in reason a launched controller inherits so its own launches (a
/// handoff replacement) are allowed: the harness's in-process opt-in, made
/// explicit across the process boundary.
pub(super) fn child_launch_reason(project_root: &Path) -> Option<&'static str> {
    (test_launcher() == TestLauncher::Harness && allowed(project_root))
        .then_some(HARNESS_OPT_IN_REASON)
}

/// Whether the wait for a launched child ends only on readiness (socket
/// published or child exited), with a hang ceiling instead of the 2s
/// test-mode budget: any launch the test policy admitted.
pub(super) fn launched_child_wait_is_readiness_based() -> bool {
    test_launcher() != TestLauncher::NotUnderTest
}

/// The `#coldlaunchflakes` guard itself. A later change that adds a launch path
/// bypassing [`super::launch_detached_at`], or stops classifying test
/// harnesses, reintroduces the startup race silently; these pin both halves.
#[cfg(test)]
mod tests {
    use super::super::{LaunchMode, connect_or_launch, launch_detached, status};
    use super::{
        TestLauncher, exe_is_workspace_test_harness, parent_pid_from_stat,
        running_in_workspace_test_harness, spawned_under_workspace_test_harness, test_launcher,
    };
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
    fn test_harnesses_are_classified_and_the_binary_is_not() {
        for harness in [
            "/w/target/debug/deps/agent_doc-3fc46b27add6fc7c",
            "/w/target/debug/deps/agent_doc_preflight_io-515bd79334718de9",
            "/w/target/debug/deps/agent_doc_queue_io-88c36bec573ccca4",
            // Integration-test harnesses are named after their `tests/*.rs` file.
            "/w/target/debug/deps/test_cli-0123456789abcdef",
            "/w/target/debug/deps/session_check-5ed933108ec6fb30",
        ] {
            assert!(
                exe_is_workspace_test_harness(Path::new(harness)),
                "{harness}"
            );
        }
        for not_a_harness in [
            // A launched controller and the CLI integration tests spawn.
            "/w/target/debug/agent-doc",
            "/usr/local/bin/agent-doc",
            "/home/u/.cargo/bin/agent-doc",
            // Build outputs beside the harnesses that are not executables Cargo
            // names with a metadata hash.
            "/w/target/debug/deps/libagent_doc-3fc46b27add6fc7c.rlib",
            "/w/target/debug/deps/test_cli-0123456789abcdef.d",
            "/w/target/debug/deps/agent_doc-3FC46B27ADD6FC7C",
            "/w/target/debug/deps/agent_doc-3fc46b27",
            "/w/target/debug/agent_doc-3fc46b27add6fc7c",
        ] {
            assert!(
                !exe_is_workspace_test_harness(Path::new(not_a_harness)),
                "{not_a_harness}"
            );
        }
        assert!(
            running_in_workspace_test_harness(),
            "this harness is agent-doc-controller-io's lib test and must be classified as one"
        );
        assert_eq!(test_launcher(), TestLauncher::Harness);
    }

    #[test]
    fn parent_pid_is_read_after_the_last_comm_paren() {
        assert_eq!(
            parent_pid_from_stat("4242 (agent-doc) S 4100 4242 4100 0 -1"),
            Some(4100)
        );
        assert_eq!(
            parent_pid_from_stat("4242 (weird ) name) R 77 4242 77 0 -1"),
            Some(77)
        );
        assert_eq!(parent_pid_from_stat("garbage"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_harness_is_not_its_own_spawned_child() {
        // The harness's ancestors are the test runner (nextest or cargo), not a
        // harness; only a process the harness spawns sees one above it.
        assert!(!spawned_under_workspace_test_harness());
    }

    #[test]
    fn an_unopted_harness_cannot_spawn_a_controller_process() {
        let dir = project();
        let refused = std::panic::catch_unwind(|| {
            let _ = launch_detached(dir.path(), LaunchMode::Lazy);
        })
        .expect_err("an unopted test harness reached a real controller spawn");
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
            "no detached controller may be launched for an unopted test harness"
        );
        let served = status(dir.path()).expect("status from the in-process actor");
        assert!(served.active, "{served:?}");
        assert_eq!(
            served.pid,
            Some(std::process::id()),
            "the controller answering a test harness must be this process"
        );
    }
}
