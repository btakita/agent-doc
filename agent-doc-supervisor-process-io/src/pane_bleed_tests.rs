//! `#perfpanebleed` live-pane regressions on the isolated tmux harness.
//!
//! The operator saw two symptoms on a hand-started `tasks/frontend.md`
//! supervisor: `[perf] find_code_ranges: 2ms` painted over the Claude composer
//! (which then read as an operator draft and wedged the idle queue), and the
//! pane "going into a format where tabs are inserted". The second is a
//! staircase: Claude Code puts the pty in raw mode (`-opost`, so no
//! `ONLCR`), and a foreign `eprintln!` ends in a bare `\n`, which moves the
//! cursor down one row WITHOUT returning to column 0. Every leaked line starts
//! where the previous one ended, which looks like inserted tabs.
//!
//! Each test respawns an isolated pane into `stty raw -echo` and runs this test
//! binary as a helper (selected by env) that writes diagnostic lines to its
//! inherited fd2 exactly as the supervisor did. Run via `make tmux-ci`.

use super::*;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const HELPER_MODE_ENV: &str = "AGENT_DOC_PANE_BLEED_HELPER_MODE";
const HELPER_ROOT_ENV: &str = "AGENT_DOC_PANE_BLEED_HELPER_ROOT";
const HELPER_TEST: &str = "pane_bleed_tests::pane_bleed_helper";
const LINES: [&str; 3] = [
    "[perf] find_code_ranges: 2ms",
    "[perf] find_code_ranges: 3ms",
    "[perf] find_code_ranges: 1ms",
];

fn write_fd2(line: &str) {
    let bytes = format!("{line}\n");
    unsafe {
        libc::write(
            libc::STDERR_FILENO,
            bytes.as_ptr().cast::<libc::c_void>(),
            bytes.len(),
        );
    }
}

fn exec_self_as(mode: &str) -> ! {
    use std::os::unix::process::CommandExt;
    let err = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", HELPER_TEST, "--ignored", "--test-threads=1"])
        .env(HELPER_MODE_ENV, mode)
        .exec();
    panic!("helper execve failed: {err}");
}

/// Helper body, selected by env. A no-op under a plain `--ignored` run.
#[test]
#[ignore = "helper process for the #perfpanebleed live tmux tests"]
fn pane_bleed_helper() {
    let Ok(mode) = std::env::var(HELPER_MODE_ENV) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var(HELPER_ROOT_ENV).unwrap());
    let mut log = NullLaunchLog;
    match mode.as_str() {
        // The pre-fix shape: a hand-started supervisor never redirected fd2.
        "leak" => LINES.iter().for_each(|line| write_fd2(line)),
        // A TUI supervisor, route-owned or not, redirects before writing.
        "redirect" => {
            let _guard =
                SupervisorStderrRedirect::maybe_start(&root, &HarnessConfig::claude(), &mut log);
            LINES.iter().for_each(|line| write_fd2(line));
        }
        // Hot-reload from a redirected image: the redirect is never undone
        // before execve, so the replacement inherits the log as fd2.
        "reexec_from_redirected" => {
            let guard =
                SupervisorStderrRedirect::maybe_start(&root, &HarnessConfig::claude(), &mut log);
            std::mem::forget(guard);
            exec_self_as("reexec_child_writes_inherited");
        }
        "reexec_child_writes_inherited" => LINES.iter().for_each(|line| write_fd2(line)),
        // Hot-reload from an image that never redirected (a hand-started
        // supervisor on a pre-fix binary): the replacement must move fd2 off
        // the pane before its first write.
        "reexec_from_unredirected" => exec_self_as("reexec_child_redirects_first"),
        "reexec_child_redirects_first" => {
            let _guard = SupervisorStderrRedirect::start_for_surviving_child_reexec(&root);
            LINES.iter().for_each(|line| write_fd2(line));
        }
        other => panic!("unknown helper mode {other}"),
    }
}

struct NullLaunchLog;

impl SupervisorLaunchLog for NullLaunchLog {
    fn log_event(&mut self, _msg: &str) {}
    fn start_console_status(&mut self, _message: &str) {}
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Run the helper in a raw-mode isolated pane and return the visible capture.
fn run_helper_in_raw_pane(label: &str, mode: &str, root: &Path) -> String {
    let iso = agent_doc_tmux_io::isolated_test_tmux(label);
    let pane = iso.new_session("bleed", root).unwrap();
    // Wide enough that the three-line staircase never soft-wraps.
    let _ = iso
        .cmd()
        .args(["resize-window", "-t", &pane, "-x", "200", "-y", "40"])
        .status();
    let exe = std::env::current_exe().unwrap();
    // stdout (libtest's own report) goes to /dev/null: only fd2 is under test.
    let script = format!(
        "stty raw -echo; {}={} {}={} {} --exact {} --ignored --test-threads=1 >/dev/null; \
         printf done > {}; sleep 30",
        HELPER_MODE_ENV,
        shell_quote(mode),
        HELPER_ROOT_ENV,
        shell_quote(&root.display().to_string()),
        shell_quote(&exe.display().to_string()),
        HELPER_TEST,
        shell_quote(&root.join("helper.done").display().to_string()),
    );
    let status = iso
        .cmd()
        .args(["respawn-pane", "-k", "-t", &pane, "sh", "-c", &script])
        .status()
        .unwrap();
    assert!(status.success(), "respawn-pane failed");
    let done = root.join("helper.done");
    for _ in 0..100 {
        if done.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(done.exists(), "helper never finished in the pane");
    std::thread::sleep(Duration::from_millis(200));
    let output = iso
        .cmd()
        .args(["capture-pane", "-p", "-t", &pane])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn supervisor_log(root: &Path) -> String {
    std::fs::read_to_string(root.join(".agent-doc/logs/supervisor-stderr.log")).unwrap_or_default()
}

fn assert_nothing_reached_the_pane(capture: &str, root: &Path) {
    assert!(
        !capture.contains("[perf]"),
        "no diagnostic byte may reach the pane:\n{capture}"
    );
    let log = supervisor_log(root);
    for line in LINES {
        assert!(log.contains(line), "{line:?} must land in the log:\n{log}");
    }
}

/// Pins the diagnosis: a bare-LF line on a raw-mode pane starts at the column
/// where the previous one ended (the "tabs inserted" staircase).
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn leaked_fd2_lines_staircase_on_a_raw_mode_pane() {
    let root = tempfile::TempDir::new().unwrap();
    let capture = run_helper_in_raw_pane("perfbleed-leak", "leak", root.path());
    let rows: Vec<&str> = capture
        .lines()
        .filter(|row| row.contains("[perf]"))
        .collect();
    assert_eq!(rows.len(), LINES.len(), "{capture}");
    let mut expected_indent = 0;
    for (row, line) in rows.iter().zip(LINES) {
        assert_eq!(
            *row,
            format!("{}{line}", " ".repeat(expected_indent)),
            "each leaked line must start where the previous one ended:\n{capture}"
        );
        expected_indent += line.len();
    }
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn hand_started_tui_supervisor_writes_nothing_to_its_raw_mode_pane() {
    let root = tempfile::TempDir::new().unwrap();
    let capture = run_helper_in_raw_pane("perfbleed-redirect", "redirect", root.path());
    assert_nothing_reached_the_pane(&capture, root.path());
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn execve_hot_reload_keeps_the_redirected_fd2() {
    let root = tempfile::TempDir::new().unwrap();
    let capture = run_helper_in_raw_pane(
        "perfbleed-reexec-redirected",
        "reexec_from_redirected",
        root.path(),
    );
    assert_nothing_reached_the_pane(&capture, root.path());
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn execve_hot_reload_from_an_unredirected_image_redirects_before_any_write() {
    let root = tempfile::TempDir::new().unwrap();
    let capture = run_helper_in_raw_pane(
        "perfbleed-reexec-unredirected",
        "reexec_from_unredirected",
        root.path(),
    );
    assert_nothing_reached_the_pane(&capture, root.path());
}
