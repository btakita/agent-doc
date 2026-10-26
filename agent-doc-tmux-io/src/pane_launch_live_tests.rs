//! Live tmux regressions for [`crate::launch_command_in_pane`].
//!
//! Each test runs a private tmux server with a clean `HOME` and a
//! `default-shell` chosen by the test, so the operator's server, shell, and rc
//! files are never involved. The stand-in shell reproduces the defect class
//! deterministically: like zsh with no `~/.zshrc` (`zsh-newuser-install`), it
//! reads the terminal before it reads its first command line, and it never
//! shows a prompt until it gets input, so no readiness wait could ever end.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::isolated_test_server::{
    IsolatedTestTmux, isolated_test_tmux_with_default_shell, write_terminal_reading_test_shell,
};

/// Generous ceiling for a positive signal that normally arrives in
/// milliseconds; it bounds a broken run, it never decides an outcome.
const SIGNAL_DEADLINE: Duration = Duration::from_secs(20);

struct Fixture {
    tmux: IsolatedTestTmux,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// A new pane in `main`, started in the fixture directory with the
    /// server's `default-shell`, exactly as a start pane is provisioned.
    fn new_shell_pane(&self) -> String {
        self.tmux
            .raw_cmd(&[
                "new-window",
                "-d",
                "-t",
                "main:",
                "-c",
                &self.dir.path().to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .expect("new-window")
    }

    fn pane_value(&self, pane: &str, format: &str) -> String {
        crate::display_message_value(&*self.tmux, Some(pane), format).unwrap_or_default()
    }
}

/// Start a private server whose `default-shell` is `shell` and whose `HOME` is
/// an empty directory.
fn fixture(label: &str, shell: impl FnOnce(&Path) -> PathBuf) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let shell = shell(dir.path());
    let tmux = isolated_test_tmux_with_default_shell(label, &shell, &home);
    Fixture { tmux, dir }
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < SIGNAL_DEADLINE,
            "no positive signal within {SIGNAL_DEADLINE:?}: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn file_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|text| !text.is_empty())
}

/// The command a launch test runs: an absolute path first (the byte that was
/// lost), it records its working directory, then blocks on a FIFO so the test
/// decides when it exits.
fn blocking_command(fixture: &Fixture) -> (String, PathBuf, PathBuf) {
    let launched = fixture.path("launched");
    let release = fixture.path("release");
    let status = std::process::Command::new("mkfifo")
        .arg(&release)
        .status()
        .expect("mkfifo");
    assert!(status.success());
    let command = format!(
        "/bin/sh -c 'pwd > {launched}; exec cat {release}'",
        launched = launched.display(),
        release = release.display(),
    );
    (command, launched, release)
}

/// Why a readiness wait cannot fix typing: the shell eats the first typed
/// byte whenever it arrives. This pins the reproduction the fix removes.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_launch_live_typed_command_loses_its_first_byte_to_a_terminal_reading_rc() {
    let fixture = fixture("startshellready-typed", write_terminal_reading_test_shell);
    let pane = fixture.new_shell_pane();

    fixture
        .tmux
        .send_keys(&pane, "/bin/true")
        .expect("send-keys");

    let consumed = fixture.path("consumed");
    wait_for("the stand-in rc reads the terminal", || {
        file_text(&consumed).is_some()
    });
    assert_eq!(file_text(&consumed).as_deref(), Some("/"));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_launch_live_runs_the_command_intact_before_any_shell_reads_the_terminal() {
    let fixture = fixture("startshellready-launch", write_terminal_reading_test_shell);
    let pane = fixture.new_shell_pane();
    let (command, launched, release) = blocking_command(&fixture);

    crate::launch_command_in_pane(&*fixture.tmux, &pane, &command).expect("launch");

    wait_for("the launched command writes its directory", || {
        file_text(&launched).is_some()
    });
    let expected_dir = std::fs::canonicalize(fixture.dir.path()).unwrap();
    assert_eq!(
        file_text(&launched).and_then(|dir| std::fs::canonicalize(dir.trim()).ok()),
        Some(expected_dir),
        "the command runs whole, in the pane's directory"
    );
    // Job control: tmux names the command, not the `sh` wrapper, as the
    // pane's foreground command while it runs.
    wait_for("the command is the pane's foreground process", || {
        fixture.pane_value(&pane, "#{pane_current_command}") == "cat"
    });
    // The pane's original shell was already blocked in its terminal read when
    // the launch replaced it; it was killed without receiving a byte.
    assert_eq!(
        file_text(&fixture.path("consumed")),
        None,
        "nothing was typed for a shell to swallow"
    );
    let pane_pid_before_handoff = fixture.pane_value(&pane, "#{pane_pid}");

    // Release the command: the pane hands off to the operator's shell in the
    // same process, so the pane stays alive with the same pane id and pid.
    std::fs::write(&release, b"").expect("release the command");
    wait_for("the pane hands off to the operator's shell", || {
        fixture.pane_value(&pane, "#{pane_current_command}") != "cat"
    });
    assert_eq!(fixture.pane_value(&pane, "#{pane_dead}"), "0");
    assert_eq!(
        fixture.pane_value(&pane, "#{pane_pid}"),
        pane_pid_before_handoff
    );
}

/// The reported environment itself: zsh with no `~/.zshrc`. Skipped (with a
/// note) on hosts without zsh; the stand-in shell above covers the class.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_launch_live_zsh_without_rc_runs_the_absolute_start_path_intact() {
    let Some(zsh) = ["/usr/bin/zsh", "/bin/zsh", "/usr/local/bin/zsh"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
    else {
        eprintln!("skipping: zsh is not installed");
        return;
    };
    let fixture = fixture("startshellready-zsh", |_| zsh);
    let pane = fixture.new_shell_pane();
    let (command, launched, release) = blocking_command(&fixture);

    crate::launch_command_in_pane(&*fixture.tmux, &pane, &command).expect("launch");

    wait_for("the launched command writes its directory", || {
        file_text(&launched).is_some()
    });
    wait_for("the command is the pane's foreground process", || {
        fixture.pane_value(&pane, "#{pane_current_command}") == "cat"
    });
    std::fs::write(&release, b"").expect("release the command");
    wait_for("the pane hands off to zsh", || {
        fixture.pane_value(&pane, "#{pane_current_command}") == "zsh"
    });
    assert_eq!(fixture.pane_value(&pane, "#{pane_dead}"), "0");
}
