//! `#clearqueuedliveturn` live end-to-end: Clear Session Context pressed while a
//! route-dispatched turn is running is QUEUED behind the turn, and the REAL
//! supervisor idle watch delivers exactly one `/clear` when the turn ends.
//!
//! Each test runs the real `agent-doc start` supervisor (idle watch, controller,
//! actor registration) in its own isolated tmux server against a scratch
//! project. The harness is a fake `claude` (a small Python script placed first
//! on the pane's `PATH`) that renders a Claude Code idle/busy frame and records
//! every line it receives. The harness turn hooks are the real
//! `agent-doc turn-status active|idle` CLI, invoked the way Claude Code's
//! `UserPromptSubmit` / `Stop` hooks invoke it.
//!
//! The turn under test is proven live ONLY by its dispatch receipt at the moment
//! Clear is pressed (the pane `%149` case): a route dispatch submits straight to
//! the pane and never moves the actor, its receipt is promoted to `running` by
//! the turn's own preflight, and the hook has not yet written a turn lease.
//!
//! A Clear at an idle prompt is typed into the same live harness, never a
//! restart (`#idleclearrestart`): the fake harness records its PID and owns a
//! background job, standing in for Claude Code background agents/shells.
//!
//! Queue auto-clear (`#bgclearpolicy`) runs on the same harness: between two
//! `go` queue items the supervisor types `/clear` into the same live process
//! (once, after the first item's committed closeout), and `disable-auto-clear`
//! drains both items without one.
//!
//! Live tmux: `#[ignore]`, run through `make tmux-ci`.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const DOC: &str = "doc.md";
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
# Fake Claude Code harness (#clearqueuedliveturn live test). Renders an idle or
# busy Claude frame, records every submitted line, and ends a turn when the
# test creates `end_turn`.
import os, select, subprocess, sys, termios
# Like Claude Code's raw-mode TUI, never echo typed input into the frame.
try:
    attrs = termios.tcgetattr(0)
    attrs[3] &= ~termios.ECHO
    termios.tcsetattr(0, termios.TCSANOW, attrs)
except termios.error:
    pass
d = os.environ["FAKE_CLAUDE_DIR"]
open(os.path.join(d, "started"), "w").close()
log = open(os.path.join(d, "input.log"), "a", buffering=1)
# A fresh harness process. An idle-prompt clear types `/clear` into this same
# process (#idleclearrestart); a start after the first is a harness restart.
log.write("<started>\n")
with open(os.path.join(d, "pids"), "a") as f:
    f.write("%d\n" % os.getpid())
# A long-running background job owned by the harness, standing in for Claude
# Code background agents/shells: a harness restart kills it with the process.
bg = subprocess.Popen(["sleep", "600"], stdin=subprocess.DEVNULL)
with open(os.path.join(d, "bg.pids"), "a") as f:
    f.write("%d\n" % bg.pid)
RULE = "─" * 40 + "\n"
def footer():
    # `background` mirrors haiven-dev api.md pane %165 (2026-10-11): an idle
    # prompt while Claude Code background subagents and a shell keep running.
    if os.path.exists(os.path.join(d, "background")):
        return ("  Opus 4.8 ctx:40% ~/fake main\n"
                "  ⏵⏵ bypass permissions on · 1 shell · ← 4 agents\n"
                "  ◯ general-purpose  Checking DecisionFeed.jsx for chat-safety status                         6m 10s · ↓ 95.4k tokens\n")
    return "  Opus 4.8 ctx:40% ~/fake main\n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n"
def idle():
    sys.stdout.write("\x1b[2J\x1b[H" + RULE + "❯ \n" + RULE + footer() + "\x1b[2;3H")
    sys.stdout.flush()
def busy():
    sys.stdout.write("\x1b[2J\x1b[H✶ Generating… (3s · esc to interrupt)\n"
                     + RULE + "❯ \n" + RULE + footer() + "\x1b[3;3H")
    sys.stdout.flush()
state = "idle"
idle()
end_turn = os.path.join(d, "end_turn")
ticks = 0
while True:
    ready, _, _ = select.select([sys.stdin], [], [], 0.1)
    ticks += 1
    if not ready and ticks % 10 == 0:
        # Like a TUI, keep repainting so stray terminal output never reads as
        # composer input.
        (busy if state == "busy" else idle)()
    if ready:
        line = sys.stdin.readline()
        if not line:
            break
        line = line.strip("\r\n")
        log.write(line + "\n")
        if line.strip() == "/clear":
            state = "idle"
            idle()
        elif line.strip():
            state = "busy"
            busy()
        else:
            (busy if state == "busy" else idle)()
    if state == "busy" and os.path.exists(end_turn):
        os.unlink(end_turn)
        state = "idle"
        idle()
"#;

fn agent_doc_bin() -> PathBuf {
    std::env::var_os("AGENT_DOC_BIN")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_agent-doc")))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// One live scenario: a scratch project, an isolated tmux server running the
/// real supervisor in pane `pane`, and the fake harness's record directory.
/// Field order is drop order: the tmux server (and with it the supervisor and
/// fake harness) dies first, then the project controller, then the temp tree.
struct LiveClearScenario {
    tmux: agent_doc_tmux_io::IsolatedTestTmux,
    _controllers: agent_doc_test_support::ProjectControllerReaper,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    doc: PathBuf,
    fake: PathBuf,
    socket: String,
    pane: String,
    path_env: String,
    ceiling: PathBuf,
}

const PLAIN_DOC: &str = "---\nagent_doc_session: 00000000-0000-4000-8000-0000000000c1\nagent: claude\n---\n\n# Session\n\n## User\n\nhello\n";

impl LiveClearScenario {
    fn start(label: &str) -> Self {
        Self::start_with_document(label, PLAIN_DOC)
    }

    fn start_with_document(label: &str, document: &str) -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let controllers = agent_doc_test_support::ProjectControllerReaper::under(tmp.path());
        let root = tmp.path().canonicalize().unwrap().join("project");
        let bin_dir = root.join(".fake-bin");
        let fake = root.join(".fake-claude");
        let home = tmp.path().canonicalize().unwrap().join("home");
        for dir in [
            root.join(".agent-doc"),
            bin_dir.clone(),
            fake.clone(),
            home.clone(),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let fake_claude = bin_dir.join("claude");
        std::fs::write(&fake_claude, FAKE_CLAUDE).unwrap();
        let agent_doc = agent_doc_bin();
        std::os::unix::fs::symlink(&agent_doc, bin_dir.join("agent-doc")).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let doc = root.join(DOC);
        std::fs::write(&doc, document).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "init"],
        ] {
            let status = Command::new("git")
                .current_dir(&root)
                .args(&args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }

        let tmux = agent_doc_tmux_io::isolated_test_tmux(label);
        let socket = tmux
            .socket_path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let path_env = format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let ceiling = std::env::var_os("AGENT_DOC_ROOT_CEILING_DIRECTORIES")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        // The supervisor resolves `claude` from PATH. Refuse to start at all
        // unless that resolves to the fake: this test must never launch a real
        // harness.
        let run = root.join(".fake-bin/run-supervisor.sh");
        std::fs::write(
            &run,
            format!(
                "#!/bin/sh\nexport PATH={path}\nexport HOME={home}\nexport FAKE_CLAUDE_DIR={fake}\nexport AGENT_DOC_TMUX_SOCKET={socket}\nexport AGENT_DOC_ROOT_CEILING_DIRECTORIES={ceiling}\nunset CLAUDECODE\nif [ \"$(command -v claude)\" != {claude} ]; then echo 'refusing: claude is not the fake harness'; sleep 600; exit 99; fi\ncd {root}\nexec agent-doc start {doc}\n",
                path = shell_quote(&path_env),
                home = shell_quote(&home.to_string_lossy()),
                fake = shell_quote(&fake.to_string_lossy()),
                socket = shell_quote(&socket),
                ceiling = shell_quote(&ceiling.to_string_lossy()),
                claude = shell_quote(&fake_claude.to_string_lossy()),
                root = shell_quote(&root.to_string_lossy()),
                doc = DOC,
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = tmux
            .cmd()
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args([
                "new-session",
                "-d",
                "-s",
                "main",
                "-x",
                "160",
                "-y",
                "40",
                "-c",
                &root.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                &run.to_string_lossy(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "tmux new-session: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let scenario = Self {
            tmux,
            _controllers: controllers,
            _tmp: tmp,
            root,
            doc,
            fake,
            socket,
            pane,
            path_env,
            ceiling,
        };
        // `#queuedclearflake`: `actor_state = 'ready'` is a sound readiness
        // signal only because the controller releases the startup `Ready`
        // edge's dispatch receipts in the same transaction that projects it.
        // When the release was a later commit, a route dispatch started right
        // here had its own `running` receipt released by that edge, and the
        // next Clear went straight into the turn (`make tmux-ci`, 2026-10-12).
        scenario.wait_until(
            "the supervisor launched the fake harness and went ready",
            || scenario.fake.join("started").exists() && scenario.actor_ready(),
        );
        scenario
    }

    fn wait_until(&self, what: &str, condition: impl FnMut() -> bool) {
        self.wait_until_within(what, Duration::from_secs(45), condition);
    }

    fn wait_until_within(&self, what: &str, budget: Duration, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!(
            "timed out waiting until {what}\n--- pane {} ---\n{}\n--- inputs ---\n{:?}\n--- ops.log tail ---\n{}\n--- supervisor idle-watch tail ---\n{}",
            self.pane,
            self.tmux.capture_pane(&self.pane, None).unwrap_or_default(),
            self.inputs(),
            self.ops_log_tail(),
            self.idle_watch_tail(),
        );
    }

    fn actor_ready(&self) -> bool {
        let Ok(conn) = agent_doc_sqlite::state_store::open_state_db(&self.root) else {
            return false;
        };
        conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE pane_id = ?1 AND actor_state = 'ready'",
            [&self.pane],
            |row| row.get::<_, i64>(0),
        )
        .is_ok_and(|count| count > 0)
    }

    fn agent_doc(&self, args: &[&str], envs: &[(&str, &str)], stdin: Option<&str>) -> Output {
        let mut command = Command::new(agent_doc_bin());
        command
            .current_dir(&self.root)
            .args(args)
            .env("PATH", &self.path_env)
            .env("AGENT_DOC_TMUX_SOCKET", &self.socket)
            .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", &self.ceiling)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("CLAUDECODE");
        for (key, value) in envs {
            command.env(key, value);
        }
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        {
            use std::io::Write;
            let mut pipe = child.stdin.take().unwrap();
            if let Some(stdin) = stdin {
                pipe.write_all(stdin.as_bytes()).unwrap();
            }
        }
        child.wait_with_output().unwrap()
    }

    /// Clear Session Context, as the editor action runs it.
    fn press_clear(&self) -> Output {
        let output = self.agent_doc(&["session", "clear", DOC], &[], None);
        assert!(
            output.status.success(),
            "Clear Session Context must exit 0 during the turn: {}\n--- pane ---\n{}\n--- inputs ---\n{:?}",
            stderr(&output),
            self.tmux.capture_pane(&self.pane, None).unwrap_or_default(),
            self.inputs(),
        );
        output
    }

    /// Assert a Clear press was QUEUED behind the live turn. On failure, show
    /// why it was not: a bare stderr is empty when the clear went straight
    /// through (`#queuedclearflake`), so include the pane, the inputs the
    /// harness received, and the ops.log tail (the guard's `flow=operator_clear`
    /// verdict and any `dispatch_in_flight_released` that took the receipt).
    fn assert_clear_queued(&self, pressed: &Output) {
        assert!(
            stderr(pressed).contains("ui_outcome=queued"),
            "the clear was not queued behind the live turn\n--- stderr ---\n{}\n--- stdout ---\n{}\n--- pane {} ---\n{}\n--- inputs ---\n{:?}\n--- ops.log tail ---\n{}",
            stderr(pressed),
            String::from_utf8_lossy(&pressed.stdout),
            self.pane,
            self.tmux.capture_pane(&self.pane, None).unwrap_or_default(),
            self.inputs(),
            self.ops_log_tail(),
        );
    }

    /// The real harness turn hook (`UserPromptSubmit` → `active`,
    /// `Stop` → `idle`), run in the harness pane's environment.
    fn hook(&self, active: bool, event: &str) {
        let payload = format!(r#"{{"hook_event_name":"{event}","session_id":"fake-session"}}"#);
        let output = self.agent_doc(
            &["turn-status", if active { "active" } else { "idle" }],
            &[("TMUX_PANE", self.pane.as_str())],
            Some(&payload),
        );
        assert!(
            output.status.success(),
            "turn-status hook failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A route dispatch: record the open receipt, submit the trigger straight
    /// into the pane (no supervisor IPC, so the actor never moves), then the
    /// dispatched turn's own preflight promotes the receipt to `running`.
    fn route_dispatch_turn(&self) {
        self.accept_dispatch();
        self.submit_dispatched_trigger();
    }

    /// Run Agent Doc accepted by the controller: an open route dispatch receipt
    /// whose trigger has not been submitted yet.
    fn accept_dispatch(&self) {
        let project_root = agent_doc_project_root_io::project_root_containing(&self.doc).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            &project_root,
            &self.doc.to_string_lossy(),
        );
        let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 1,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "route dispatch, no actor transition",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
    }

    /// The route submits the trigger straight into the pane (no supervisor
    /// IPC, so the actor never moves), then the dispatched turn's own preflight
    /// promotes its receipt to `running`.
    fn submit_dispatched_trigger(&self) {
        let trigger = format!("/agent-doc {DOC}");
        let before = self
            .inputs()
            .iter()
            .filter(|line| **line == trigger)
            .count();
        self.tmux.send_keys(&self.pane, &trigger).unwrap();
        self.wait_until("the fake harness received the dispatched trigger", || {
            self.inputs()
                .iter()
                .filter(|line| **line == trigger)
                .count()
                > before
        });
        assert_eq!(
            agent_doc_controller_io::project_controller::mark_dispatch_turn_started_for_file(
                &self.doc
            )
            .unwrap(),
            1,
            "the dispatched turn's preflight promotes its receipt"
        );
        assert!(
            agent_doc_controller_io::project_controller::dispatch_turn_running_for_file(&self.doc)
                .unwrap()
        );
    }

    /// The harness finishes its turn: `Stop` hook, then the idle frame.
    fn end_turn(&self) {
        self.hook(false, "Stop");
        std::fs::write(self.fake.join("end_turn"), "").unwrap();
    }

    fn inputs(&self) -> Vec<String> {
        std::fs::read_to_string(self.fake.join("input.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// PIDs of every harness process the supervisor launched, in order.
    fn harness_pids(&self) -> Vec<u32> {
        read_pids(&self.fake.join("pids"))
    }

    /// PIDs of the background jobs those harness processes started, in order.
    fn background_pids(&self) -> Vec<u32> {
        read_pids(&self.fake.join("bg.pids"))
    }

    /// `/clear`s typed into the harness, plus harness restarts (Codex, a
    /// waiting-input or dead child: the cases that still replace the child).
    fn clears(&self) -> usize {
        let inputs = self.inputs();
        let restarts = inputs
            .iter()
            .filter(|line| *line == "<started>")
            .count()
            .saturating_sub(1);
        inputs.iter().filter(|line| *line == "/clear").count() + restarts
    }

    fn ops_log(&self) -> String {
        std::fs::read_to_string(self.root.join(".agent-doc/logs/ops.log")).unwrap_or_default()
    }

    /// The supervisor session logs' idle-queue-watch decisions, for a timeout
    /// diagnosis (the watch logs there, not to `ops.log`).
    fn idle_watch_tail(&self) -> String {
        let mut lines = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.root.join(".agent-doc/logs")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name().is_some_and(|name| name != "ops.log")
                    && path.extension().is_some_and(|ext| ext == "log")
                {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    lines.extend(
                        text.lines()
                            .filter(|line| {
                                line.contains("idle_queue_watch")
                                    || line.contains("idle-queue watch")
                            })
                            .map(str::to_string),
                    );
                }
            }
        }
        lines[lines.len().saturating_sub(40)..].join("\n")
    }

    fn ops_log_tail(&self) -> String {
        let log = self.ops_log();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(25)..].join("\n")
    }
}

impl Drop for LiveClearScenario {
    fn drop(&mut self) {
        // The background jobs are the fake harness's own `sleep`s; never leave
        // one behind when a harness survived to the end of the test.
        for pid in self.background_pids() {
            if process_alive(pid)
                && let Err(err) = Command::new("kill").arg(pid.to_string()).status()
            {
                eprintln!("kill background job {pid}: {err}");
            }
        }
    }
}

fn read_pids(path: &std::path::Path) -> Vec<u32> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// A live (non-zombie) process.
fn process_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .and_then(|(_, tail)| tail.split_whitespace().next())
            .is_some_and(|state| !matches!(state, "Z" | "X" | "x"))
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `#idleclearrestart` (2026-10-11 live test: a Claude background agent stopped
/// at 12/40 ticks): Clear Session Context at an idle prompt TYPES `/clear` into
/// the live harness, exactly like the queued delivery. The harness process is
/// the same one (no restart), the background job it owns keeps running, and no
/// document trigger follows the clear because no work is pending.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn an_idle_clear_types_clear_into_the_same_harness_and_background_jobs_survive() {
    let scenario = LiveClearScenario::start("clear-idle-same-child");
    // Background subagents and a shell are running at the idle prompt.
    std::fs::write(scenario.fake.join("background"), "").unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let harness = scenario.harness_pids();
    let background = scenario.background_pids();
    assert_eq!(harness.len(), 1, "one harness launched: {harness:?}");
    assert_eq!(background.len(), 1, "one background job: {background:?}");
    assert!(process_alive(harness[0]) && process_alive(background[0]));

    let pressed = scenario.press_clear();
    assert!(
        !stderr(&pressed).contains("session_clear queued for"),
        "an idle prompt clears at once: {}",
        stderr(&pressed)
    );
    scenario.wait_until("the typed /clear reached the harness", || {
        scenario.inputs().iter().any(|line| line == "/clear")
    });
    // Long enough for a restart (and its post-restart trigger) to show.
    std::thread::sleep(Duration::from_secs(5));

    let inputs = scenario.inputs();
    assert_eq!(scenario.clears(), 1, "exactly one clear: {inputs:?}");
    assert_eq!(
        scenario.harness_pids(),
        harness,
        "the clear reached the SAME harness process, never a restart: {inputs:?}"
    );
    assert!(
        process_alive(harness[0]),
        "the harness child is still running"
    );
    assert!(
        process_alive(background[0]),
        "the harness's background job survived the clear"
    );
    assert!(
        !inputs.iter().any(|line| line.starts_with("/agent-doc")),
        "no document trigger follows an idle clear with no pending work: {inputs:?}"
    );
    let ops = scenario.ops_log();
    assert!(
        ops.lines().any(|line| line.contains("session_clear_sent")),
        "{}",
        scenario.ops_log_tail()
    );
    assert!(
        !ops.contains("delivery=supervisor_restart_fresh"),
        "an idle clear is never a restart:\n{}",
        scenario.ops_log_tail()
    );
}

/// The pane `%149` case: a clear pressed while a route-dispatched turn is
/// proven live only by its dispatch receipt is queued (exit 0, nothing typed),
/// a repeat press re-arms the one queued clear, and exactly one `/clear`
/// reaches the pane once the turn ends — the `Stop` hook settles the receipt,
/// so the turn-ended projection arrives at the turn end instead of after the
/// 30-minute in-flight horizon.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn queued_clear_lands_once_when_a_dispatch_receipt_only_turn_ends() {
    let scenario = LiveClearScenario::start("clear-queued-live-turn");
    scenario.route_dispatch_turn();

    let first = scenario.press_clear();
    assert!(
        first.status.success(),
        "queued clear exits 0: {}",
        stderr(&first)
    );
    let first_stderr = stderr(&first);
    assert!(
        first_stderr.contains("session_clear queued for"),
        "{first_stderr}"
    );
    assert!(
        first_stderr.contains("evidence=dispatch_turn_running"),
        "{first_stderr}"
    );
    scenario.assert_clear_queued(&first);
    assert!(
        first_stderr.contains("evidence_age_secs="),
        "the notice cites how old its evidence is: {first_stderr}"
    );

    let repeat = scenario.press_clear();
    assert!(
        repeat.status.success(),
        "repeat press exits 0: {}",
        stderr(&repeat)
    );
    assert!(
        stderr(&repeat).contains("rearmed=true") && stderr(&repeat).contains("still one clear"),
        "a repeat press re-arms the one queued clear: {}",
        stderr(&repeat)
    );

    // The hook writes the turn lease late, mid-turn; the clear still waits.
    scenario.hook(true, "UserPromptSubmit");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        scenario.clears(),
        0,
        "nothing is typed into the running turn"
    );

    scenario.end_turn();
    scenario.wait_until("the queued clear reached the pane", || {
        scenario.clears() >= 1
    });
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(
        scenario.clears(),
        1,
        "exactly one /clear: {:?}",
        scenario.inputs()
    );
    assert!(
        !agent_doc_controller_io::project_controller::dispatch_turn_running_for_file(&scenario.doc)
            .unwrap(),
        "the Stop hook settled the route-dispatched turn"
    );
    assert!(
        scenario.ops_log().lines().any(|line| {
            line.contains("queue_context_clear_projection_started")
                && line.contains("source=operator_live_turn_deferred_clear")
                && line.contains("phase=InFlight")
        }),
        "the deferred clear was promoted to in-flight on delivery:\n{}",
        scenario.ops_log_tail()
    );
}

/// A newer prompt that starts on the pane before the turn ends retires the
/// queued clear: the operator continued the conversation, so zero `/clear`
/// reaches the pane even after the (newer) turn ends.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn queued_clear_is_dropped_when_a_newer_prompt_starts_first() {
    let scenario = LiveClearScenario::start("clear-queued-newer-prompt");
    scenario.route_dispatch_turn();

    let pressed = scenario.press_clear();
    assert!(pressed.status.success(), "{}", stderr(&pressed));
    scenario.assert_clear_queued(&pressed);

    // The dispatched turn's lease, adopted by the watch as the clear's turn.
    scenario.hook(true, "UserPromptSubmit");
    std::thread::sleep(Duration::from_secs(3));
    // A newer prompt starts (the harness runs a message the operator queued).
    scenario.hook(true, "UserPromptSubmit");
    scenario.wait_until("the watch retired the clear for the newer prompt", || {
        scenario
            .ops_log()
            .contains("idle_queue_watch_deferred_clear_superseded")
    });
    assert!(scenario.ops_log().contains("reason=newer_prompt"));
    // `#clearsupersedenotice`: the controller folded the drop into the
    // projection the editor's document-turn authority stream carries, so the
    // operator who pressed Clear is told it was not sent.
    scenario.wait_until("the controller projected the dropped-clear notice", || {
        scenario.ops_log().lines().any(|line| {
            line.contains("queue_context_clear_projection_settled")
                && line.contains("retired_reason=newer_prompt")
                && !line.contains("superseded_epoch=none")
        })
    });

    scenario.end_turn();
    std::thread::sleep(Duration::from_secs(6));
    assert_eq!(
        scenario.clears(),
        0,
        "a superseded clear is never delivered: {:?}",
        scenario.inputs()
    );
}

/// The haiven-dev `tasks/sdk.md` incident (2026-10-11, pane %180): a
/// route-dispatched turn ended normally but its receipt stayed `running`, so
/// Clear #1 at an idle prompt was queued behind a turn that had already ended,
/// Clear #2 (pressed after Run Agent Doc was accepted) was absorbed into it, and
/// no `/clear` was ever sent. Now the `Stop` hook settles the ended turn, so
/// Clear #1 goes straight through; Clear #2 is queued behind the accepted
/// dispatch (it would otherwise race the trigger into the composer) and is
/// delivered once that turn ends. Clear → Run → Clear ends with two clears in
/// the order the operator pressed them.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn clear_run_clear_after_a_normally_ended_dispatch_turn_delivers_both_clears_in_order() {
    let scenario = LiveClearScenario::start("clear-run-clear");
    // Background subagents and a shell keep running throughout (the api.md
    // pane): they are not a live foreground turn and never hold a clear.
    std::fs::write(scenario.fake.join("background"), "").unwrap();
    // A route-dispatched turn that ends normally; the actor never moves.
    scenario.route_dispatch_turn();
    scenario.hook(true, "UserPromptSubmit");
    std::thread::sleep(Duration::from_millis(1200));
    scenario.end_turn();
    assert!(
        !agent_doc_controller_io::project_controller::dispatch_turn_running_for_file(&scenario.doc)
            .unwrap(),
        "the Stop hook settled the ended dispatch turn (no stale `running` receipt)"
    );
    std::thread::sleep(Duration::from_secs(1));

    // Clear #1 at an idle prompt is not queued behind a turn that ended.
    let first = scenario.press_clear();
    assert!(
        !stderr(&first).contains("session_clear queued for"),
        "an idle prompt clears at once: {}",
        stderr(&first)
    );
    scenario.wait_until("clear #1 reached the pane", || scenario.clears() == 1);
    // `#idleclearrestart`: clear #1 was TYPED into the live harness — no
    // restart, so no post-restart document trigger either.
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        scenario.harness_pids().len(),
        1,
        "clear #1 must not restart the harness: {:?}",
        scenario.inputs()
    );
    assert_eq!(
        scenario
            .inputs()
            .iter()
            .filter(|line| *line == "/agent-doc doc.md")
            .count(),
        1,
        "no trigger follows an idle clear: {:?}",
        scenario.inputs()
    );

    // Run Agent Doc is accepted; its trigger has not reached the pane yet.
    scenario.accept_dispatch();
    let second = scenario.press_clear();
    let second_stderr = stderr(&second);
    assert!(
        second_stderr.contains("session_clear queued for"),
        "{second_stderr}"
    );
    assert!(
        second_stderr.contains("evidence=dispatch_turn_pending"),
        "{second_stderr}"
    );
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(scenario.clears(), 1, "clear #2 does not race the trigger");

    scenario.submit_dispatched_trigger();
    scenario.hook(true, "UserPromptSubmit");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        scenario.clears(),
        1,
        "clear #2 waits for the dispatched turn"
    );
    scenario.end_turn();
    scenario.wait_until("clear #2 reached the pane", || scenario.clears() == 2);
    std::thread::sleep(Duration::from_secs(3));
    let inputs = scenario.inputs();
    assert_eq!(scenario.clears(), 2, "{inputs:?}");
    assert_eq!(
        inputs[inputs.len() - 2..],
        ["/agent-doc doc.md".to_string(), "/clear".to_string()],
        "clear #2 lands after the run the operator started before it: {inputs:?}"
    );
}

/// Clear during turn A, then Run Agent Doc (B) accepted, then Clear again: the
/// second press re-arms the one queued clear, so B — which the operator started
/// before pressing — is adopted as the clear's turn rather than superseding
/// it. Exactly one `/clear`, after B.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn a_repeat_press_rearms_the_queued_clear_behind_the_run_started_before_it() {
    let scenario = LiveClearScenario::start("clear-rearm");
    scenario.route_dispatch_turn();
    scenario.hook(true, "UserPromptSubmit");
    let first = scenario.press_clear();
    scenario.assert_clear_queued(&first);
    std::thread::sleep(Duration::from_secs(2));

    scenario.accept_dispatch();
    std::thread::sleep(Duration::from_millis(1100));
    let second = scenario.press_clear();
    assert!(
        stderr(&second).contains("rearmed=true"),
        "a repeat press re-arms the queued clear: {}",
        stderr(&second)
    );

    // Turn A is still live while the watch re-baselines the re-armed clear on
    // it; then A ends and B's trigger lands. B's lease is newer than A's, so
    // only the dispatched-before-the-press adoption keeps the clear alive.
    std::thread::sleep(Duration::from_secs(3));
    scenario.end_turn();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        scenario.clears(),
        0,
        "B was accepted before the press: wait for it"
    );
    std::thread::sleep(Duration::from_millis(1100));
    scenario.submit_dispatched_trigger();
    scenario.hook(true, "UserPromptSubmit");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        scenario.clears(),
        0,
        "nothing is typed into B's running turn"
    );
    assert!(
        !scenario
            .ops_log()
            .contains("idle_queue_watch_deferred_clear_superseded"),
        "{}",
        scenario.ops_log_tail()
    );
    scenario.end_turn();
    scenario.wait_until("the re-armed clear reached the pane", || {
        scenario.clears() == 1
    });
    std::thread::sleep(Duration::from_secs(3));
    let inputs = scenario.inputs();
    assert_eq!(scenario.clears(), 1, "{inputs:?}");
    assert_eq!(
        inputs.last().map(String::as_str),
        Some("/clear"),
        "{inputs:?}"
    );
}

/// A two-item `go` queue for the `#bgclearpolicy` auto-clear scenarios.
fn auto_clear_queue_doc(marker: &str) -> String {
    format!(
        "---\nagent_doc_session: 00000000-0000-4000-8000-0000000000c1\nagent: claude\nagent_doc_format: template\n---\n\n# Session\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nhello\n<!-- /agent:exchange -->\n\n## Queue\n\n{marker}\n- answer item one\n- answer item two\n<!-- /agent:queue -->\n"
    )
}

impl LiveClearScenario {
    /// Document triggers the supervisor typed into the harness.
    fn triggers(&self) -> usize {
        self.inputs()
            .iter()
            .filter(|line| line.contains("agent-doc") && line.contains(DOC))
            .count()
    }

    /// The agent's cycle for the dispatched head, as the real harness hooks and
    /// the agent run it: `UserPromptSubmit`, preflight, a committed response that
    /// answers (and so consumes) the free-text head, then `Stop` and the idle
    /// frame.
    fn answer_head(&self, head: &str) {
        self.hook(true, "UserPromptSubmit");
        let pane = [("TMUX_PANE", self.pane.as_str())];
        let preflight = self.agent_doc(&["preflight", DOC], &pane, None);
        assert!(
            preflight.status.success(),
            "preflight: {}\n{}",
            String::from_utf8_lossy(&preflight.stdout),
            stderr(&preflight)
        );
        let response = format!(
            "<!-- patch:exchange -->\n### Re: {head} — test-model\n\n> **Queue prompt:** {head}\n\nDone: {head}.\n<!-- /patch:exchange -->\n"
        );
        let write = self.agent_doc(&["write", "--commit", DOC], &pane, Some(&response));
        assert!(
            write.status.success(),
            "write --commit: {}\n{}",
            String::from_utf8_lossy(&write.stdout),
            stderr(&write)
        );
        self.end_turn();
    }
}

/// `#bgclearpolicy`: with queue auto-clear on (the default), the supervisor
/// resets the context between queue items. After item one's committed
/// closeout it types `/clear` into the SAME live harness — the process and its
/// background job survive — and only then dispatches item two.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn queue_auto_clear_types_clear_between_items_into_the_same_harness() {
    let scenario = LiveClearScenario::start_with_document(
        "queue-auto-clear",
        &auto_clear_queue_doc("<!-- agent:queue go -->"),
    );
    std::fs::write(scenario.fake.join("background"), "").unwrap();
    // Run Agent Doc starts item one.
    scenario.route_dispatch_turn();
    let harness = scenario.harness_pids();
    let background = scenario.background_pids();
    assert_eq!(scenario.clears(), 0, "no clear before the first item");

    scenario.answer_head("answer item one");
    // No editor is attached, so no document-delivery edge wakes the watch; it
    // re-observes the queue on its 60s safety revision interval.
    scenario.wait_until_within(
        "the supervisor dispatched item two",
        Duration::from_secs(240),
        || scenario.triggers() == 2,
    );
    std::thread::sleep(Duration::from_secs(2));

    let inputs = scenario.inputs();
    let first = inputs
        .iter()
        .position(|line| line.contains("agent-doc"))
        .unwrap();
    let second = inputs
        .iter()
        .rposition(|line| line.contains("agent-doc"))
        .unwrap();
    assert_eq!(
        inputs[first + 1..second]
            .iter()
            .filter(|line| *line == "/clear")
            .count(),
        1,
        "exactly one /clear between the two items: {inputs:?}"
    );
    assert_eq!(scenario.clears(), 1, "{inputs:?}");
    assert_eq!(
        scenario.harness_pids(),
        harness,
        "the clear was typed into the SAME harness process, never a restart: {inputs:?}"
    );
    assert!(
        process_alive(harness[0]) && process_alive(background[0]),
        "the harness and its background job survived the clear"
    );
    let ops = scenario.ops_log();
    assert!(
        ops.lines().any(|line| line.contains("] queue_auto_clear ")
            && line.contains("delivery=native_clear")
            && line.contains("result=sent")),
        "{}",
        scenario.ops_log_tail()
    );
}

/// `#bgclearpolicy`: `disable-auto-clear` drains both items in one context.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn queue_disable_auto_clear_drains_without_a_clear() {
    let scenario = LiveClearScenario::start_with_document(
        "queue-no-auto-clear",
        &auto_clear_queue_doc("<!-- agent:queue go disable-auto-clear -->"),
    );
    scenario.route_dispatch_turn();
    scenario.answer_head("answer item one");
    // No editor is attached, so no document-delivery edge wakes the watch; it
    // re-observes the queue on its 60s safety revision interval.
    scenario.wait_until_within(
        "the supervisor dispatched item two",
        Duration::from_secs(240),
        || scenario.triggers() == 2,
    );
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        scenario.clears(),
        0,
        "disable-auto-clear never clears: {:?}",
        scenario.inputs()
    );
    assert!(
        !scenario
            .ops_log()
            .lines()
            .any(|line| line.contains("] queue_auto_clear ")),
        "{}",
        scenario.ops_log_tail()
    );
}
