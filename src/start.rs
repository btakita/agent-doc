//! # Module: start
//!
//! ## Spec
//! - `run(file)`: validates the file exists, then ensures a session UUID is
//!   present in the YAML frontmatter (generates and writes one if absent).
//! - Resolves agent args through the active harness:
//!   - Claude: frontmatter `agent_args` > frontmatter `claude_args` >
//!     config `agent_args` > config `claude_args` > `AGENT_DOC_CLAUDE_ARGS`
//!   - Codex: frontmatter `agent_args` > frontmatter `codex_args` >
//!     config `agent_args` > config `codex_args`
//!   - OpenCode: frontmatter `agent_args` > frontmatter `opencode_args` >
//!     config `agent_args` > config `opencode_args`
//! - Requires an active tmux session; bails immediately if not inside tmux.
//! - If another pane already owns the same document session, `start` fails
//!   closed instead of reusing, restarting, or superseding that pane. The
//!   error includes tmux inspection/cleanup commands so the user can decide
//!   which pane to keep and which pane(s) to kill manually.
//! - If the configured project tmux session is dead and a fresh start must
//!   register the current pane in another live tmux session, `start` updates
//!   `.agent-doc/config.toml` to that live session so later route/claim work
//!   follows the new binding instead of the stale dead session.
//! - If `sessions.json` points at an alive pane that is not the current pane,
//!   `start` must also fail closed instead of attempting a supervisor-driven
//!   reuse/restart or a registry rebind. Normal `start` is never allowed to
//!   decide which live pane should disappear.
//! - Registers the session UUID → current tmux pane ID in `sessions.json` so
//!   other subcommands (`route`, `focus`, etc.) can locate the pane.
//! - Runs the configured harness binary as a blocking child process inside a persistent restart loop
//!   so a normal tmux pane never dies on its own.
//! - When `--route-owned` is set by `route` auto-start, watches for the first
//!   new binary-owned document cycle to reach `committed`, stops the child, and
//!   reaps the tmux pane. Failed or interrupted cycles stay visible for debugging.
//! - On non-zero exit (context exhaustion, crash, etc.): auto-restarts after a
//!   2-second delay using `--continue` to resume the previous conversation.
//! - On clean exit (code 0): honors the active harness policy.
//!   Claude prompts on stderr and waits for Enter (fresh restart) or `q` + Enter (exit).
//!   Codex auto-restarts in resume mode so `codex exec` remains a persistent session.
//!   Exception: if a fresh/fresh-restart Codex child exits cleanly before it ever
//!   surfaces an idle prompt, treat that as failed startup provenance and restart
//!   fresh instead of chaining `--continue`.
//!   If stdin EOF/Ctrl-D was forwarded, Codex returns to the restart-or-quit
//!   prompt so the operator can intentionally restart fresh or exit the
//!   supervisor cleanly even when the previous run already committed. A
//!   stdin-forwarded Ctrl+C that terminates the child now uses that same quit
//!   prompt instead of being misclassified as a transient crash. Only
//!   promptless fresh/fresh-restart exits without a forwarded operator quit
//!   key still count as failed startup provenance.
//!   Prompt decisions are still logged explicitly, and the supervisor forces a
//!   canonical prompt tty mode for those `Enter`/`q` prompts instead of
//!   trusting the inherited parent harness stdin settings. Prompt-time stdin EOF on
//!   the remaining resume-failure prompt path restarts fresh instead of
//!   silently quitting, so routed or detached Codex sessions do not lose the
//!   claimed tmux pane just because the supervisor prompt had no readable
//!   stdin. Non-empty non-`q` input is rejected and re-prompted instead of
//!   silently restarting fresh.
//!   If the resume handoff just failed, the first failure restarts fresh and
//!   repeated failures escalate to that same prompt instead of looping blindly.
//! - Prints the truncated session UUID and pane ID to stderr on registration.
//! - Opens a persistent session log at `.agent-doc/logs/<session-uuid>.log`,
//!   appending timestamped events for session start, claude start/restart/exit,
//!   user quit, and session end.
//! - On `--continue` restarts, spawns a background thread that waits for the
//!   harness prompt to appear in the current child process's filtered pty
//!   output before injecting the harness-specific trigger command back through
//!   the claimed tmux pane input path to auto-trigger the skill workflow in
//!   the resumed conversation.
//!   This avoids the race where DSR (Device Status Report) escape sequences
//!   interleave with the injected command, corrupting Claude Code's input
//!   state, while also ensuring stale tmux scrollback cannot be mistaken for
//!   the new child's prompt and a stale worker cannot later type into the
//!   supervisor prompt or a replacement process in the tmux pane. If the
//!   prompt still has not appeared after 30 seconds, the thread logs a
//!   provisional timeout but keeps watching until the child exits or the
//!   prompt appears.
//!
//! ## Agentic Contracts
//! - The file path must exist before `run` is called; callers must not rely on
//!   `run` to create the document.
//! - After `run` returns `Ok(())`, the session has ended cleanly (user chose
//!   to quit); the sessions.json entry is not automatically removed.
//! - Session UUID in frontmatter is idempotent: calling `run` on a file that
//!   already has a UUID does not regenerate or overwrite it.
//! - Resolved harness args are prepended to every agent invocation inside the
//!   loop, including restarts; they are resolved once at startup and held for
//!   the lifetime of the loop.
//! - The module writes to the document file (UUID injection), `sessions.json`,
//!   and `.agent-doc/logs/<session-uuid>.log`; it does not touch snapshots,
//!   git, or claims.
//! - Must be called from within an active tmux session; violating this contract
//!   returns an immediate `Err`.
//!
//! ## Evals
//! - `start_missing_file`: call `run` with a non-existent path → returns `Err`
//!   containing "file not found".
//! - `start_outside_tmux`: call `run` with a valid file while `TMUX` env var is
//!   unset → returns `Err` containing "not running inside tmux".
//! - `start_generates_uuid`: call `run` on a file with no frontmatter UUID →
//!   UUID is injected into the file and a "Generated session UUID" line appears
//!   on stderr before `claude` is launched.
//! - `start_preserves_existing_uuid`: call `run` on a file that already has a
//!   `session:` key → file content is unchanged (no re-write), no "Generated"
//!   message on stderr.
//! - `start_registers_session`: after setup, `sessions.json` maps the session
//!   UUID to the current tmux pane ID.
//! - `start_claude_args_precedence`: Claude resolves frontmatter `claude_args`
//!   over config `claude_args`, with `AGENT_DOC_CLAUDE_ARGS` as fallback.
//! - `start_codex_uses_codex_specific_alias_chain`: Codex resolves `codex_args`
//!   after `agent_args` and ignores `claude_args`.
//! - `start_opencode_uses_opencode_specific_alias_chain`: OpenCode resolves
//!   `opencode_args` after `agent_args` and ignores Claude/Codex aliases.

use anyhow::{Context, Result};
use portable_pty::PtySize;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use crate::supervisor::{
    cwd,
    env::EnvSpec,
    ipc::{IpcMethod, IpcResponse, SupervisorIpc},
    pty::PtySpawnConfig,
    resize,
    state::{CrashPolicy, RestartAction, SupervisorState},
};
use crate::{config, frontmatter, sessions, snapshot};

/// Open (or create) the session log file at `.agent-doc/logs/<session-uuid>.log`.
/// Returns a writable file handle in append mode, or None if the directory can't be created.
fn open_session_log(file: &Path, session_id: &str) -> Option<std::fs::File> {
    // Walk up from the document to find the project root containing .agent-doc/
    let dir = file.parent()?;
    let mut search = Some(dir);
    let mut agent_doc_dir = None;
    while let Some(d) = search {
        let candidate = d.join(".agent-doc");
        if candidate.is_dir() {
            agent_doc_dir = Some(candidate);
            break;
        }
        search = d.parent();
    }
    let logs_dir = agent_doc_dir?.join("logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join(format!("{}.log", session_id));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO-ish: just use epoch seconds for simplicity in logs
    format!("{}", now)
}

fn log_event(log: &mut Option<std::fs::File>, msg: &str) {
    if let Some(f) = log {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

fn exit_provenance_fields(status: &portable_pty::ExitStatus) -> String {
    let rendered = status.to_string();
    if let Some(signal) = rendered.strip_prefix("Terminated by ") {
        format!("exit_kind=signal exit_signal={signal:?} exit_status={rendered:?}")
    } else if status.success() {
        format!("exit_kind=success exit_status={rendered:?}")
    } else {
        format!("exit_kind=exit_code exit_status={rendered:?}")
    }
}

const FAILED_RESUME_WINDOW: Duration = Duration::from_secs(15 * 60);
const FAILED_RESUME_THRESHOLD: usize = 2;
const AUTO_TRIGGER_INITIAL_DELAY: Duration = Duration::from_secs(2);
const AUTO_TRIGGER_POLL_INTERVAL: Duration = Duration::from_millis(500);
const AUTO_TRIGGER_TIMEOUT: Duration = Duration::from_secs(30);
const AUTO_TRIGGER_OUTPUT_BYTES_MAX: usize = 64 * 1024;
const ROUTE_OWNED_COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const SHARED_WRITER_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SHARED_WRITER_WRITE_POLL_INTERVAL_MS: i32 = 50;
const SHARED_WRITER_CHUNK_MAX: usize = 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum AutoTriggerOutcome {
    NotNeeded = 0,
    Pending = 1,
    Sent = 2,
    Timeout = 3,
    SendFailed = 4,
    Cancelled = 5,
}

impl AutoTriggerOutcome {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Sent,
            3 => Self::Timeout,
            4 => Self::SendFailed,
            5 => Self::Cancelled,
            _ => Self::NotNeeded,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::NotNeeded => "not_needed",
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Timeout => "timeout",
            Self::SendFailed => "send_failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum CapabilityProofGate {
    NotRequired = 0,
    Pending = 1,
    Proven = 2,
    Failed = 3,
}

impl CapabilityProofGate {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Proven,
            3 => Self::Failed,
            _ => Self::NotRequired,
        }
    }
}

#[derive(Debug)]
struct AutoTriggerMonitor {
    started_at: Instant,
    timeout: Duration,
    timed_out: bool,
}

impl AutoTriggerMonitor {
    fn new(started_at: Instant, timeout: Duration) -> Self {
        Self {
            started_at,
            timeout,
            timed_out: false,
        }
    }

    fn note_no_prompt(&mut self, now: Instant) -> bool {
        if self.timed_out || now.duration_since(self.started_at) < self.timeout {
            return false;
        }
        self.timed_out = true;
        true
    }

    fn stop_outcome(&self) -> AutoTriggerOutcome {
        if self.timed_out {
            AutoTriggerOutcome::Timeout
        } else {
            AutoTriggerOutcome::Cancelled
        }
    }
}

#[derive(Debug, Default)]
struct FailedResumeTracker {
    events: VecDeque<Instant>,
}

impl FailedResumeTracker {
    fn record(&mut self, now: Instant) -> usize {
        self.events.push_back(now);
        self.prune(now);
        self.events.len()
    }

    fn reset(&mut self) {
        self.events.clear();
    }

    fn prune(&mut self, now: Instant) {
        let cutoff = now.checked_sub(FAILED_RESUME_WINDOW).unwrap_or(now);
        while let Some(front) = self.events.front() {
            if *front < cutoff {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanExitResolution {
    PromptUser,
    RestartContinue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartContinueExitStrategy {
    Resume,
    RestartFresh,
    CtrlCPromptUser,
    CtrlDPromptUser,
    PromptUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptDecision {
    RestartFresh,
    Quit,
    QuitEof,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptOutcome {
    RestartFresh,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptEofPolicy {
    Quit,
    RestartFresh,
}

fn classify_prompt_decision(bytes_read: usize, input: &str) -> PromptDecision {
    if bytes_read == 0 {
        return PromptDecision::QuitEof;
    }
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("q") {
        return PromptDecision::Quit;
    }
    if trimmed.is_empty() {
        return PromptDecision::RestartFresh;
    }
    PromptDecision::Invalid
}

fn prompt_input_summary(input: &str) -> String {
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]);
    let mut summary = String::new();
    let mut count = 0usize;
    for ch in trimmed.chars() {
        count += 1;
        if count > 32 {
            summary.push_str("...");
            break;
        }
        match ch {
            '\r' => summary.push_str("\\r"),
            '\n' => summary.push_str("\\n"),
            '\t' => summary.push_str("\\t"),
            c if c.is_control() => summary.push('?'),
            c => summary.push(c),
        }
    }
    if summary.is_empty() {
        "<empty>".to_string()
    } else {
        summary
    }
}

fn prompt_for_restart_or_quit(
    session_log: &mut Option<std::fs::File>,
    prompt_kind: &str,
    prompt_text: &str,
    quit_event: &str,
    eof_policy: PromptEofPolicy,
) -> PromptOutcome {
    loop {
        eprintln!("{prompt_text}");
        let mut input = String::new();
        let bytes_read = match std::io::stdin().read_line(&mut input) {
            Ok(n) => n,
            Err(_) => {
                log_event(session_log, "stdin_read_failed — exiting loop");
                return PromptOutcome::Quit;
            }
        };
        match classify_prompt_decision(bytes_read, &input) {
            PromptDecision::Quit => {
                log_event(session_log, quit_event);
                return PromptOutcome::Quit;
            }
            PromptDecision::QuitEof => match eof_policy {
                PromptEofPolicy::Quit => {
                    log_event(
                        session_log,
                        &format!("user_quit_after_eof prompt={prompt_kind}"),
                    );
                    return PromptOutcome::Quit;
                }
                PromptEofPolicy::RestartFresh => {
                    log_event(
                        session_log,
                        &format!("user_restart_fresh_after_eof prompt={prompt_kind}"),
                    );
                    return PromptOutcome::RestartFresh;
                }
            },
            PromptDecision::RestartFresh => {
                log_event(
                    session_log,
                    &format!(
                        "user_restart_fresh prompt={} bytes_read={} input={}",
                        prompt_kind,
                        bytes_read,
                        prompt_input_summary(&input)
                    ),
                );
                return PromptOutcome::RestartFresh;
            }
            PromptDecision::Invalid => {
                eprintln!("Unrecognized input. Press Enter to restart fresh, or 'q' to exit.");
                log_event(
                    session_log,
                    &format!(
                        "prompt_input_invalid prompt={} bytes_read={} input={}",
                        prompt_kind,
                        bytes_read,
                        prompt_input_summary(&input)
                    ),
                );
            }
        }
    }
}

fn clean_exit_resolution(harness: &crate::harness::HarnessConfig) -> CleanExitResolution {
    match harness.clean_exit_behavior {
        crate::harness::CleanExitBehavior::PromptUser => CleanExitResolution::PromptUser,
        crate::harness::CleanExitBehavior::RestartContinue => CleanExitResolution::RestartContinue,
    }
}

fn restart_continue_exit_strategy(
    ctrl_c_forwarded_interrupt: bool,
    failed_resume: bool,
    ctrl_d_forwarded: bool,
    recent_failed_resumes: usize,
    clean_exit_before_prompt: bool,
) -> RestartContinueExitStrategy {
    if ctrl_c_forwarded_interrupt {
        return RestartContinueExitStrategy::CtrlCPromptUser;
    }
    if ctrl_d_forwarded {
        return RestartContinueExitStrategy::CtrlDPromptUser;
    }
    if clean_exit_before_prompt {
        return RestartContinueExitStrategy::RestartFresh;
    }
    if failed_resume && recent_failed_resumes >= FAILED_RESUME_THRESHOLD {
        return RestartContinueExitStrategy::PromptUser;
    }
    if failed_resume {
        return RestartContinueExitStrategy::RestartFresh;
    }
    RestartContinueExitStrategy::Resume
}

fn resume_handoff_failed(
    auto_trigger_enabled: bool,
    ctrl_d_forwarded: bool,
    outcome: AutoTriggerOutcome,
) -> bool {
    if !auto_trigger_enabled || ctrl_d_forwarded {
        return false;
    }
    matches!(
        outcome,
        AutoTriggerOutcome::Pending
            | AutoTriggerOutcome::Timeout
            | AutoTriggerOutcome::SendFailed
            | AutoTriggerOutcome::Cancelled
    )
}

fn clean_exit_before_prompt_seen(auto_trigger_enabled: bool, prompt_visible_once: bool) -> bool {
    !auto_trigger_enabled && !prompt_visible_once
}

fn route_owned_cycle_changed_after_start(
    current: &crate::cycle_state::CycleState,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

fn route_owned_cycle_completed_after_start(
    current: &crate::cycle_state::CycleState,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> bool {
    route_owned_cycle_changed_after_start(current, baseline)
        && current.phase == crate::cycle_state::CyclePhase::Committed
}

fn spawn_route_owned_completion_thread(
    shared: Arc<SupervisorShared>,
    file: PathBuf,
    baseline: Option<crate::cycle_state::CycleState>,
    completed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("route-owned-completion".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) && !completed.load(Ordering::Relaxed) {
                if let Ok(Some(state)) = crate::cycle_state::load(&file)
                    && route_owned_cycle_completed_after_start(&state, baseline.as_ref())
                {
                    completed.store(true, Ordering::Relaxed);
                    log_event(
                        &mut session_log,
                        &format!(
                            "route_owned_cycle_committed cycle={} event={}",
                            state.cycle_id, state.last_event
                        ),
                    );
                    shared.stop_requested.store(true, Ordering::Relaxed);
                    shared.kill_child();
                    return;
                }
                if !sleep_with_stop(&stop, ROUTE_OWNED_COMPLETION_POLL_INTERVAL) {
                    return;
                }
            }
        })
        .expect("spawn route-owned completion thread")
}

fn strip_stale_ctrl_d_before_prompt(
    data: &[u8],
    suppress_stale_ctrl_d_until_prompt: bool,
    prompt_visible_once: bool,
) -> Option<Vec<u8>> {
    if !suppress_stale_ctrl_d_until_prompt || prompt_visible_once || !data.contains(&0x04) {
        return None;
    }

    Some(data.iter().copied().filter(|byte| *byte != 0x04).collect())
}

fn is_forwarded_ctrl_c_interrupt_exit(
    status: &portable_pty::ExitStatus,
    ctrl_c_forwarded: bool,
) -> bool {
    if !ctrl_c_forwarded {
        return false;
    }

    let rendered = status.to_string();
    rendered
        .strip_prefix("Terminated by ")
        .is_some_and(|signal| {
            signal.eq_ignore_ascii_case("Interrupt") || signal.eq_ignore_ascii_case("SIGINT")
        })
        || status.exit_code() == 130
}

fn policy_exit_code_for_supervisor(exit_code: i32, ctrl_c_forwarded_interrupt: bool) -> i32 {
    if ctrl_c_forwarded_interrupt {
        0
    } else {
        exit_code
    }
}

fn sleep_with_stop(stop: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let remaining = deadline.saturating_duration_since(now);
        std::thread::sleep(std::cmp::min(remaining, Duration::from_millis(100)));
    }
}

struct SharedPtyWriter {
    writer: Box<dyn Write + Send>,
    #[cfg(unix)]
    raw_fd: Option<std::os::unix::io::RawFd>,
}

impl SharedPtyWriter {
    #[cfg(any(not(unix), test))]
    fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer,
            #[cfg(unix)]
            raw_fd: None,
        }
    }

    #[cfg(unix)]
    fn with_raw_fd(writer: Box<dyn Write + Send>, raw_fd: std::os::unix::io::RawFd) -> Self {
        Self {
            writer,
            raw_fd: Some(raw_fd),
        }
    }

    fn write_all_interruptibly(&mut self, bytes: &[u8], stop: &AtomicBool) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(fd) = self.raw_fd {
            return write_all_fd_interruptibly(fd, bytes, stop);
        }

        write_all_with_stop(self.writer.as_mut(), bytes, stop)
    }

    fn write_all_blocking(&mut self, bytes: &[u8]) -> io::Result<()> {
        let never_stop = AtomicBool::new(false);
        self.write_all_interruptibly(bytes, &never_stop)
    }
}

#[cfg(unix)]
impl Drop for SharedPtyWriter {
    fn drop(&mut self) {
        if let Some(fd) = self.raw_fd.take() {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

fn write_all_with_stop(writer: &mut dyn Write, bytes: &[u8], stop: &AtomicBool) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "writer cancelled",
            ));
        }
        let end = (written + SHARED_WRITER_CHUNK_MAX).min(bytes.len());
        let n = writer.write(&bytes[written..end])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "writer returned 0 bytes",
            ));
        }
        written += n;
    }
    if stop.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "writer cancelled",
        ));
    }
    writer.flush()
}

#[cfg(unix)]
fn write_all_fd_interruptibly(
    fd: std::os::unix::io::RawFd,
    bytes: &[u8],
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "writer cancelled",
            ));
        }

        let mut fds = [libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        }];
        let ret = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                SHARED_WRITER_WRITE_POLL_INTERVAL_MS,
            )
        };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if ret == 0 {
            continue;
        }

        let revents = fds[0].revents;
        if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("pty writer poll failed: revents=0x{revents:x}"),
            ));
        }
        if revents & libc::POLLOUT == 0 {
            continue;
        }

        let end = (written + SHARED_WRITER_CHUNK_MAX).min(bytes.len());
        let chunk = &bytes[written..end];
        let n = unsafe { libc::write(fd, chunk.as_ptr() as *const libc::c_void, chunk.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if matches!(
                err.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(err);
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "pty master write returned 0 bytes",
            ));
        }
        written += n as usize;
    }
    Ok(())
}

fn lock_writer_interruptibly<'a>(
    writer_arc: &'a Arc<Mutex<SharedPtyWriter>>,
    stop: &AtomicBool,
) -> Option<std::sync::MutexGuard<'a, SharedPtyWriter>> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match writer_arc.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::WouldBlock) => {
                if !sleep_with_stop(stop, SHARED_WRITER_LOCK_POLL_INTERVAL) {
                    return None;
                }
            }
            Err(TryLockError::Poisoned(err)) => return Some(err.into_inner()),
        }
    }
}

fn auto_trigger_inject_command(
    shared: &SupervisorShared,
    stop: &AtomicBool,
    trigger_cmd: &str,
) -> AutoTriggerOutcome {
    if stop.load(Ordering::Relaxed) {
        return AutoTriggerOutcome::Cancelled;
    }
    if let Some(reason) = shared.capability_dispatch_blocker() {
        eprintln!("[agent-doc] auto-trigger gated: {reason}");
        return AutoTriggerOutcome::SendFailed;
    }
    shared.transition_actor_state(
        crate::session_actor::ActorState::Busy,
        "dispatch",
        "auto_trigger_inject",
    );
    let submitted_text = crate::supervisor::ipc::normalize_submit_text(trigger_cmd);
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        return match dispatch_submit_text_to_pane(pane_id, &submitted_text) {
            Ok(()) => AutoTriggerOutcome::Sent,
            Err(_) => AutoTriggerOutcome::SendFailed,
        };
    }

    let Some(writer_arc) = shared.inject_writer.lock().unwrap().clone() else {
        return AutoTriggerOutcome::SendFailed;
    };
    if stop.load(Ordering::Relaxed) {
        return AutoTriggerOutcome::Cancelled;
    }

    let payload = crate::supervisor::ipc::submit_bytes(&submitted_text).into_bytes();

    let Some(mut writer) = lock_writer_interruptibly(&writer_arc, stop) else {
        return AutoTriggerOutcome::Cancelled;
    };
    if stop.load(Ordering::Relaxed) {
        return AutoTriggerOutcome::Cancelled;
    }
    match writer.write_all_interruptibly(&payload, stop) {
        Ok(()) => AutoTriggerOutcome::Sent,
        Err(err) if err.kind() == io::ErrorKind::Interrupted && stop.load(Ordering::Relaxed) => {
            AutoTriggerOutcome::Cancelled
        }
        Err(_) => AutoTriggerOutcome::SendFailed,
    }
}

fn normalize_supervisor_inject_bytes(bytes: &str) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let raw = bytes.as_bytes();
    let mut index = 0usize;
    while index < raw.len() {
        match raw[index] {
            b'\r' => {
                normalized.push(b'\r');
                if raw.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'\n' => normalized.push(b'\r'),
            byte => normalized.push(byte),
        }
        index += 1;
    }
    normalized
}

fn dispatch_submit_text_to_tmux(
    tmux: &crate::sessions::Tmux,
    pane: &str,
    text: &str,
) -> Result<()> {
    crate::sessions::send_submitted_text(tmux, pane, text)
        .with_context(|| format!("failed to inject submitted input into pane {}", pane))
}

fn dispatch_submit_text_to_pane(pane: &str, text: &str) -> Result<()> {
    let tmux = crate::sessions::Tmux::default_server();
    dispatch_submit_text_to_tmux(&tmux, pane, text)
}

fn record_recent_output(shared: &SupervisorShared, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut recent = shared.recent_output.lock().unwrap();
    recent.extend_from_slice(bytes);
    if recent.len() > AUTO_TRIGGER_OUTPUT_BYTES_MAX {
        let overflow = recent.len() - AUTO_TRIGGER_OUTPUT_BYTES_MAX;
        recent.drain(..overflow);
    }
}

fn prompt_visible_requires_ready_transition(shared: &SupervisorShared) -> bool {
    let first_prompt_for_child = !shared.prompt_visible_once.swap(true, Ordering::Relaxed);
    if first_prompt_for_child {
        return true;
    }
    shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state != crate::session_actor::ActorState::Ready)
}

fn latest_prompt_candidate_line(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> Option<String> {
    let recent = shared.recent_output.lock().unwrap();
    let output = String::from_utf8_lossy(&recent);
    harness.last_prompt_candidate(&output)
}

fn current_child_prompt_visible(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> bool {
    let Some(line) = latest_prompt_candidate_line(shared, harness) else {
        return false;
    };
    let stripped = crate::prompt::strip_ansi(&line);
    harness.matches_prompt(stripped.trim())
}

fn spawn_auto_trigger_thread(
    shared: Arc<SupervisorShared>,
    stop: Arc<AtomicBool>,
    file: String,
    harness: crate::harness::HarnessConfig,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("auto-trigger".into())
        .spawn(move || {
            let mut monitor = AutoTriggerMonitor::new(Instant::now(), AUTO_TRIGGER_TIMEOUT);
            for attempt in 0.. {
                let delay = if attempt == 0 {
                    AUTO_TRIGGER_INITIAL_DELAY
                } else {
                    AUTO_TRIGGER_POLL_INTERVAL
                };
                if !sleep_with_stop(&stop, delay) {
                    shared
                        .auto_trigger_outcome
                        .store(monitor.stop_outcome() as u8, Ordering::Relaxed);
                    return;
                }
                if current_child_prompt_visible(&shared, &harness) {
                    if shared.capability_proof_gate() == CapabilityProofGate::Pending {
                        if monitor.note_no_prompt(Instant::now()) {
                            shared
                                .auto_trigger_outcome
                                .store(AutoTriggerOutcome::Timeout as u8, Ordering::Relaxed);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "auto_trigger_timeout harness={} reason=capability_proof_pending_after_30s",
                                    harness.binary
                                ),
                            );
                            eprintln!(
                                "[agent-doc] auto-trigger: timed out waiting for managed Codex capability proof"
                            );
                            return;
                        }
                        continue;
                    }
                    let trigger_cmd = harness.trigger_command(&file);
                    match auto_trigger_inject_command(&shared, &stop, &trigger_cmd) {
                        AutoTriggerOutcome::Sent => {
                            shared
                                .auto_trigger_outcome
                                .store(AutoTriggerOutcome::Sent as u8, Ordering::Relaxed);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "auto_trigger_sent harness={} cmd=\"{}\"",
                                    harness.binary, trigger_cmd
                                ),
                            );
                            eprintln!("[agent-doc] auto-triggered: {}", trigger_cmd);
                        }
                        AutoTriggerOutcome::Cancelled => {
                            shared
                                .auto_trigger_outcome
                                .store(AutoTriggerOutcome::Cancelled as u8, Ordering::Relaxed);
                        }
                        AutoTriggerOutcome::SendFailed => {
                            shared
                                .auto_trigger_outcome
                                .store(AutoTriggerOutcome::SendFailed as u8, Ordering::Relaxed);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "auto_trigger_failed harness={} reason=pty_write",
                                    harness.binary
                                ),
                            );
                            eprintln!("[agent-doc] auto-trigger failed");
                        }
                        outcome => {
                            shared
                                .auto_trigger_outcome
                                .store(outcome as u8, Ordering::Relaxed);
                        }
                    }
                    return;
                }
                if monitor.note_no_prompt(Instant::now()) {
                    shared
                        .auto_trigger_outcome
                        .store(AutoTriggerOutcome::Timeout as u8, Ordering::Relaxed);
                    log_event(
                        &mut session_log,
                        &format!(
                            "auto_trigger_timeout harness={} reason=no_prompt_after_30s",
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[agent-doc] auto-trigger: timed out waiting for {} prompt",
                        harness.binary
                    );
                }
            }
        })
        .expect("spawn auto-trigger thread")
}

fn spawn_managed_capability_proof_thread(
    shared: Arc<SupervisorShared>,
    harness_binary: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    fm: frontmatter::Frontmatter,
    global_config: config::Config,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    let thread_name = format!("{harness_binary}-capability-proof");
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            match crate::agent::codex::prove_managed_session_capabilities(
                &harness_binary,
                &args,
                &env,
                &fm,
                &global_config,
                &harness_binary,
            ) {
                Ok(Some(event)) => {
                    eprintln!("[start] managed {} capability proof: {}", harness_binary, event);
                    shared.set_capability_proof_gate(CapabilityProofGate::Proven, None);
                    log_event(&mut session_log, &event);
                }
                Ok(None) => {
                    shared.set_capability_proof_gate(CapabilityProofGate::NotRequired, None);
                    log_event(
                        &mut session_log,
                        &format!("{}_capability_proof status=not_required", harness_binary),
                    );
                }
                Err(err) => {
                    let detail = err.to_string();
                    shared.set_capability_proof_gate(
                        CapabilityProofGate::Failed,
                        Some(detail.clone()),
                    );
                    shared.transition_actor_state(
                        crate::session_actor::ActorState::Blocked,
                        "supervisor",
                        &format!("{}_capability_proof_failed", harness_binary),
                    );
                    log_event(
                        &mut session_log,
                        &format!("{}_capability_proof status=failed error={detail:?}", harness_binary),
                    );
                    eprintln!(
                        "[start] managed {} capability proof failed; terminating unusable child session: {detail}",
                        harness_binary
                    );
                    shared.kill_child();
                }
            }
        })
        .expect("spawn capability proof thread")
}

#[derive(Debug, PartialEq, Eq)]
enum ExistingSessionPaneAction {
    Refuse(String),
}

fn existing_session_pane_action(
    tmux: &sessions::Tmux,
    session_id: &str,
    file: &Path,
    current_pane: &str,
) -> Result<Option<ExistingSessionPaneAction>> {
    let entry = sessions::lookup_entry(session_id)?;
    let live_owner = crate::sync::find_normal_path_owner_pane_excluding_quiet(
        tmux,
        file,
        session_id,
        Some(current_pane),
    );
    Ok(existing_session_pane_action_from_entry(
        tmux,
        current_pane,
        entry.as_ref(),
        live_owner.as_deref(),
    ))
}

fn existing_session_pane_action_from_entry(
    tmux: &sessions::Tmux,
    current_pane: &str,
    entry: Option<&sessions::SessionEntry>,
    live_owner: Option<&str>,
) -> Option<ExistingSessionPaneAction> {
    if let Some(owner) = live_owner
        && owner != current_pane
    {
        return Some(ExistingSessionPaneAction::Refuse(owner.to_string()));
    }

    let entry = entry?;
    if entry.pane == current_pane || !tmux.pane_alive(&entry.pane) {
        return None;
    }
    Some(ExistingSessionPaneAction::Refuse(entry.pane.clone()))
}

fn format_existing_pane_conflict_error(
    tmux: &sessions::Tmux,
    file: &Path,
    current_pane: &str,
    conflicting_pane: &str,
) -> String {
    let conflict_session = tmux.pane_session(conflicting_pane).unwrap_or_default();
    let conflict_window = tmux.pane_window(conflicting_pane).unwrap_or_default();
    let current_session = tmux.pane_session(current_pane).unwrap_or_default();
    let current_window = tmux.pane_window(current_pane).unwrap_or_default();
    format!(
        "refusing to start {} in pane {} because pane {} is already bound to this document.\n\
\n\
Existing owner:\n\
  pane={} session={} window={}\n\
\n\
Current launcher pane:\n\
  pane={} session={} window={}\n\
\n\
Inspect the conflicting panes first:\n\
  tmux list-panes -a -F '#{{session_name}} #{{window_name}} #{{pane_id}} #{{pane_current_command}} #{{pane_current_path}}'\n\
  tmux capture-pane -pt {} | tail -n 80\n\
  tmux capture-pane -pt {} | tail -n 80\n\
\n\
If you want to keep the existing owner, kill this launcher pane yourself and rerun from the owner pane:\n\
  tmux kill-pane -t {}\n\
\n\
If you want to replace the existing owner, kill it yourself first and then rerun `agent-doc start` from pane {}:\n\
  tmux kill-pane -t {}",
        file.display(),
        current_pane,
        conflicting_pane,
        conflicting_pane,
        conflict_session,
        conflict_window,
        current_pane,
        current_session,
        current_window,
        conflicting_pane,
        current_pane,
        current_pane,
        current_pane,
        conflicting_pane
    )
}

/// Put stdin into raw mode so the outer pty line discipline doesn't translate
/// input bytes (ICRNL converts \r → \n, breaking Enter for Claude Code's TUI).
/// Restores original termios on drop.
#[cfg(unix)]
fn prompt_termios_from_original(original: &libc::termios) -> libc::termios {
    let mut prompt = *original;
    prompt.c_iflag |= libc::ICRNL;
    prompt.c_iflag &= !(libc::IGNCR | libc::INLCR);
    prompt.c_oflag |= libc::OPOST | libc::ONLCR;
    prompt.c_lflag |= libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN;
    prompt.c_cc[libc::VMIN] = 1;
    prompt.c_cc[libc::VTIME] = 0;
    prompt
}

#[cfg(unix)]
struct RawMode {
    original: libc::termios,
}

#[cfg(unix)]
impl RawMode {
    fn enable() -> Self {
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            libc::tcgetattr(libc::STDIN_FILENO, &mut original);
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
            Self { original }
        }
    }

    /// Temporarily restore a canonical prompt mode so `read_line()` works even
    /// when the parent harness left stdin in a raw-ish state.
    fn suspend(&self) {
        unsafe {
            let prompt = prompt_termios_from_original(&self.original);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &prompt);
        }
    }

    /// Re-enable raw mode after a suspend.
    fn resume(&self) {
        unsafe {
            let mut raw = self.original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(not(unix))]
struct RawMode;

#[cfg(not(unix))]
impl RawMode {
    fn enable() -> Self {
        Self
    }
    fn suspend(&self) {}
    fn resume(&self) {}
}

/// Signal to stop the stdin→pty writer thread.
///
/// Uses a self-pipe: the writer thread polls both stdin and the pipe read end.
/// Calling `signal()` writes a byte to the pipe, waking the poll and causing
/// the writer thread to exit cleanly so stdin is available for `read_line()`.
#[cfg(unix)]
struct StopSignal {
    read_fd: std::os::unix::io::RawFd,
    write_fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl StopSignal {
    fn new() -> Result<Self> {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            anyhow::bail!("pipe() failed: {}", std::io::Error::last_os_error());
        }
        Ok(Self {
            read_fd: fds[0],
            write_fd: fds[1],
        })
    }

    /// Wake the writer thread so it exits.
    fn signal(&self) {
        unsafe {
            libc::write(self.write_fd, b"x".as_ptr() as *const libc::c_void, 1);
        }
    }

    fn read_fd(&self) -> std::os::unix::io::RawFd {
        self.read_fd
    }
}

#[cfg(unix)]
impl Drop for StopSignal {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[cfg(not(unix))]
struct StopSignal;

#[cfg(not(unix))]
impl StopSignal {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    fn signal(&self) {}
}

/// Shared writer handle: outer Mutex guards replace/clear, inner Mutex guards concurrent writes.
type SharedWriter = Mutex<Option<Arc<Mutex<SharedPtyWriter>>>>;

#[derive(Debug, Clone)]
struct SessionActorRuntime {
    project_root: PathBuf,
    file: PathBuf,
    session_id: String,
    pane_id: String,
    generation: u64,
}

impl SessionActorRuntime {
    fn transition(
        &self,
        state: crate::session_actor::ActorState,
        caller: &str,
        reason: &str,
    ) -> Result<crate::session_actor::ActorRecord> {
        crate::project_controller::mark_lifecycle(
            &self.project_root,
            crate::project_controller::LifecycleRequest {
                file: self.file.clone(),
                session_id: self.session_id.clone(),
                pane_id: self.pane_id.clone(),
                generation: self.generation,
                state,
                caller: caller.to_string(),
                reason: reason.to_string(),
            },
        )
    }
}

/// Shared state between the main supervisor loop and the IPC handler thread.
struct SupervisorShared {
    /// Current supervisor state for IPC `state` queries.
    supervisor_state: Mutex<SupervisorState>,
    /// Authoritative actor lifecycle context for this pane generation.
    actor_runtime: Option<SessionActorRuntime>,
    /// Best-known actor lifecycle state for IPC `state` responses.
    actor_state: Mutex<Option<crate::session_actor::ActorState>>,
    /// PID of the long-lived `agent-doc start` supervisor process.
    supervisor_pid: u32,
    /// Stable identity for this supervisor process across child restarts.
    supervisor_instance_id: String,
    /// Current restart count.
    restart_count: AtomicU32,
    /// Whether a child is currently running.
    running: AtomicBool,
    /// CWD source tag for IPC `state` responses.
    cwd_source: &'static str,
    /// Writer handle for IPC `inject`. Replaced on each spawn, cleared between restarts.
    inject_writer: SharedWriter,
    /// Claimed tmux pane that should receive supervisor-owned injected input.
    inject_pane: Option<String>,
    /// Filtered output emitted by the current child process.
    recent_output: Mutex<Vec<u8>>,
    /// Child PID for IPC `pid` queries and `kill` on restart/stop.
    child_pid: AtomicU32,
    /// Flag: IPC requested a restart.
    restart_requested: AtomicBool,
    /// Flag: IPC requested a stop.
    stop_requested: AtomicBool,
    /// Restart mode requested via IPC ("fresh" or "continue").
    restart_mode: Mutex<String>,
    /// Flag: stdin→pty writer forwarded \x04 (Ctrl+D) to the pty.
    ctrl_d_forwarded: AtomicBool,
    /// Flag: stdin→pty writer forwarded \x03 (Ctrl+C) to the pty.
    ctrl_c_forwarded: AtomicBool,
    /// Outcome of the most recent auto-trigger attempt after a restart.
    auto_trigger_outcome: AtomicU8,
    /// Whether the current child ever surfaced an idle harness prompt.
    prompt_visible_once: AtomicBool,
    /// Whether the current keepalive successor should ignore stale inherited
    /// Ctrl+D bytes until the child surfaces an idle prompt.
    suppress_stale_ctrl_d_until_prompt: AtomicBool,
    /// Gate for managed Codex launches that require live network/SSH/write-root proof.
    capability_proof_gate: AtomicU8,
    capability_proof_error: Mutex<Option<String>>,
}

impl SupervisorShared {
    #[cfg(test)]
    fn new(cwd_source: &'static str, supervisor_instance_id: String) -> Self {
        Self::with_actor_runtime(cwd_source, supervisor_instance_id, None, None, None)
    }

    fn with_actor_runtime(
        cwd_source: &'static str,
        supervisor_instance_id: String,
        actor_runtime: Option<SessionActorRuntime>,
        actor_state: Option<crate::session_actor::ActorState>,
        inject_pane: Option<String>,
    ) -> Self {
        Self {
            supervisor_state: Mutex::new(SupervisorState::Healthy),
            actor_runtime,
            actor_state: Mutex::new(actor_state),
            supervisor_pid: std::process::id(),
            supervisor_instance_id,
            restart_count: AtomicU32::new(0),
            running: AtomicBool::new(false),
            cwd_source,
            inject_writer: Mutex::new(None),
            inject_pane,
            recent_output: Mutex::new(Vec::new()),
            child_pid: AtomicU32::new(0),
            restart_requested: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            restart_mode: Mutex::new("continue".to_string()),
            ctrl_d_forwarded: AtomicBool::new(false),
            ctrl_c_forwarded: AtomicBool::new(false),
            auto_trigger_outcome: AtomicU8::new(AutoTriggerOutcome::NotNeeded as u8),
            prompt_visible_once: AtomicBool::new(false),
            suppress_stale_ctrl_d_until_prompt: AtomicBool::new(false),
            capability_proof_gate: AtomicU8::new(CapabilityProofGate::NotRequired as u8),
            capability_proof_error: Mutex::new(None),
        }
    }

    fn capability_proof_gate(&self) -> CapabilityProofGate {
        CapabilityProofGate::from_u8(self.capability_proof_gate.load(Ordering::Relaxed))
    }

    fn set_capability_proof_gate(&self, gate: CapabilityProofGate, error: Option<String>) {
        *self.capability_proof_error.lock().unwrap() = error;
        self.capability_proof_gate
            .store(gate as u8, Ordering::Relaxed);
    }

    fn capability_dispatch_blocker(&self) -> Option<String> {
        match self.capability_proof_gate() {
            CapabilityProofGate::NotRequired | CapabilityProofGate::Proven => None,
            CapabilityProofGate::Pending => Some(
                "managed Codex capability proof is still pending; prompt dispatch is gated until network/SSH/write-root proof succeeds"
                    .to_string(),
            ),
            CapabilityProofGate::Failed => {
                let detail = self
                    .capability_proof_error
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "unknown error".to_string());
                Some(format!(
                    "managed Codex capability proof failed; prompt dispatch is disabled: {detail}"
                ))
            }
        }
    }

    fn transition_actor_state(
        &self,
        state: crate::session_actor::ActorState,
        caller: &str,
        reason: &str,
    ) {
        let Some(runtime) = self.actor_runtime.as_ref() else {
            return;
        };
        match runtime.transition(state, caller, reason) {
            Ok(record) => {
                *self.actor_state.lock().unwrap() = Some(record.state);
            }
            Err(err) => {
                eprintln!(
                    "[session-actor] warning: failed to record {} transition for {}: {}",
                    state.as_str(),
                    runtime.file.display(),
                    err
                );
            }
        }
    }

    /// Send SIGTERM to the child process to unblock `wait()`.
    #[cfg(unix)]
    fn kill_child(&self) {
        let pid = self.child_pid.load(Ordering::Relaxed);
        if pid > 0 {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    #[cfg(not(unix))]
    fn kill_child(&self) {
        // On non-Unix, we can't send signals. The main loop will detect
        // the flags after the child exits naturally or via other means.
    }
}

fn handle_ipc(method: IpcMethod, shared: &SupervisorShared) -> IpcResponse {
    match method {
        IpcMethod::State => {
            let state = shared.supervisor_state.lock().unwrap();
            let actor_state = shared
                .actor_state
                .lock()
                .unwrap()
                .map(|state| state.as_str().to_string());
            let actor_session_id = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.session_id.clone());
            let actor_pane_id = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.pane_id.clone());
            let actor_generation = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.generation);
            IpcResponse::ok(serde_json::json!({
                "running": shared.running.load(Ordering::Relaxed),
                "state": state.as_str(),
                "actor_state": actor_state,
                "actor_session_id": actor_session_id,
                "actor_pane_id": actor_pane_id,
                "actor_generation": actor_generation,
                "restart_count": shared.restart_count.load(Ordering::Relaxed),
                "cwd_source": shared.cwd_source,
                "supervisor_pid": shared.supervisor_pid,
                "supervisor_instance_id": shared.supervisor_instance_id,
                "child_pid": shared.child_pid.load(Ordering::Relaxed),
            }))
        }
        IpcMethod::Pid => {
            if shared.supervisor_pid > 0 {
                IpcResponse::ok(serde_json::json!({
                    "pid": shared.supervisor_pid,
                    "supervisor_instance_id": shared.supervisor_instance_id,
                }))
            } else {
                IpcResponse::ok(serde_json::json!({ "pid": null }))
            }
        }
        IpcMethod::Inject { bytes } => {
            if let Some(reason) = shared.capability_dispatch_blocker() {
                return IpcResponse::err(reason);
            }
            let injected = if let Some(pane_id) = shared.inject_pane.as_deref() {
                dispatch_submit_text_to_pane(pane_id, &bytes).map_err(|e| e.to_string())
            } else {
                let guard = shared.inject_writer.lock().unwrap();
                match guard.as_ref() {
                    Some(writer_arc) => {
                        let mut w = writer_arc.lock().unwrap();
                        let normalized = normalize_supervisor_inject_bytes(&bytes);
                        w.write_all_blocking(&normalized)
                            .map_err(|e| format!("write error: {e}"))
                    }
                    None => Err("no active session".to_string()),
                }
            };
            match injected {
                Ok(()) => {
                    shared.transition_actor_state(
                        crate::session_actor::ActorState::Busy,
                        "dispatch",
                        "ipc_inject",
                    );
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                Err(err) => IpcResponse::err(err),
            }
        }
        IpcMethod::Restart { mode } => {
            shared.transition_actor_state(
                crate::session_actor::ActorState::Busy,
                "supervisor",
                "ipc_restart_requested",
            );
            *shared.restart_mode.lock().unwrap() = mode;
            shared.restart_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
        IpcMethod::Stop { graceful: _ } => {
            shared.stop_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
    }
}

/// Spawn the master→stdout forwarding thread with escape sequence filtering.
fn spawn_reader_thread(
    shared: Arc<SupervisorShared>,
    harness: crate::harness::HarnessConfig,
    mut reader: Box<dyn std::io::Read + Send>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pty->stdout".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filtered = Vec::with_capacity(8192);
            let stdout = std::io::stdout();
            let debug_filter = std::env::var("AGENT_DOC_DEBUG_FILTER").is_ok();
            // Stateful filter — carries partial escape sequences across reads
            let mut pty_filter = crate::supervisor::pty::PtyFilter::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if debug_filter {
                            // Log raw bytes, showing escape sequences as hex
                            let raw = &buf[..n];
                            let mut display = String::new();
                            for &b in raw {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!("[pty-filter] raw ({n} bytes): {display}");
                        }
                        filtered.clear();
                        pty_filter.filter(&buf[..n], &mut filtered);
                        if debug_filter {
                            let mut display = String::new();
                            for &b in &filtered {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!(
                                "[pty-filter] filtered ({} bytes): {display}",
                                filtered.len()
                            );
                        }
                        if filtered.is_empty() {
                            continue;
                        }
                        record_recent_output(&shared, &filtered);
                        if current_child_prompt_visible(&shared, &harness) {
                            if prompt_visible_requires_ready_transition(&shared) {
                                shared.transition_actor_state(
                                    crate::session_actor::ActorState::Ready,
                                    "supervisor",
                                    "prompt_ready",
                                );
                            }
                            shared
                                .suppress_stale_ctrl_d_until_prompt
                                .store(false, Ordering::Relaxed);
                        }
                        let mut lock = stdout.lock();
                        if lock.write_all(&filtered).is_err() || lock.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn pty->stdout thread")
}

/// Spawn the stdin→master forwarding thread using a shared writer.
///
/// Uses `poll()` on stdin + a stop pipe so the thread can be interrupted
/// cleanly before the supervisor needs stdin for the restart prompt.
#[cfg(unix)]
fn spawn_writer_thread(
    shared: Arc<SupervisorShared>,
    writer: Arc<Mutex<SharedPtyWriter>>,
    stop_fd: std::os::unix::io::RawFd,
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let debug = std::env::var("AGENT_DOC_DEBUG_STDIN").is_ok();
            if debug {
                eprintln!("[stdin->pty] thread started");
            }
            loop {
                // Poll stdin (fd 0) and the stop pipe
                let mut fds = [
                    libc::pollfd {
                        fd: libc::STDIN_FILENO,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: stop_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if ret <= 0 {
                    if debug {
                        eprintln!("[stdin->pty] poll returned {ret}, exiting");
                    }
                    break; // poll error or signal interrupt
                }
                // Stop signal received
                if fds[1].revents & libc::POLLIN != 0 {
                    if debug {
                        eprintln!("[stdin->pty] stop signal received, exiting");
                    }
                    break;
                }
                // stdin ready
                if fds[0].revents & libc::POLLIN != 0 {
                    let n = unsafe {
                        libc::read(
                            libc::STDIN_FILENO,
                            buf.as_mut_ptr() as *mut libc::c_void,
                            buf.len(),
                        )
                    };
                    if n <= 0 {
                        if debug {
                            eprintln!("[stdin->pty] read returned {n}, exiting");
                        }
                        break; // EOF or error
                    }
                    let data = &buf[..n as usize];
                    let maybe_filtered = strip_stale_ctrl_d_before_prompt(
                        data,
                        shared
                            .suppress_stale_ctrl_d_until_prompt
                            .load(Ordering::Relaxed),
                        shared.prompt_visible_once.load(Ordering::Relaxed),
                    );
                    let data = maybe_filtered.as_deref().unwrap_or(data);
                    if data.is_empty() {
                        if debug {
                            eprintln!(
                                "[stdin->pty] suppressed stale Ctrl+D before keepalive prompt"
                            );
                        }
                        continue;
                    }
                    // Detect Ctrl+D (\x04) — in raw mode this is a byte, not EOF.
                    // The pty slave's line discipline interprets it as EOF for the child.
                    if let Some(ref flag) = ctrl_d_flag
                        && data.contains(&0x04)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+D (\\x04) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    if let Some(ref flag) = ctrl_c_flag
                        && data.contains(&0x03)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+C (\\x03) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                        if debug {
                            eprintln!("[stdin->pty] stop requested while waiting for writer");
                        }
                        break;
                    };
                    if let Err(err) = w.write_all_interruptibly(data, stop.as_ref()) {
                        if debug {
                            eprintln!("[stdin->pty] pty write failed, exiting: {err}");
                        }
                        break;
                    }
                }
                // stdin hangup/error
                if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    if debug {
                        eprintln!(
                            "[stdin->pty] stdin hangup/error (revents=0x{:x}), exiting",
                            fds[0].revents
                        );
                    }
                    break;
                }
            }
            if debug {
                eprintln!("[stdin->pty] thread exiting");
            }
        })
        .expect("spawn stdin->pty thread")
}

/// Non-Unix fallback: blocking stdin read (no stop signal support).
#[cfg(not(unix))]
fn spawn_writer_thread(
    _shared: Arc<SupervisorShared>,
    writer: Arc<Mutex<SharedPtyWriter>>,
    _stop_fd: (),
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let stdin = std::io::stdin();
            loop {
                let mut lock = stdin.lock();
                match std::io::Read::read(&mut lock, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        drop(lock);
                        if let Some(ref flag) = ctrl_d_flag {
                            if buf[..n].contains(&0x04) {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                        if let Some(ref flag) = ctrl_c_flag {
                            if buf[..n].contains(&0x03) {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                        let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                            break;
                        };
                        if w.write_all_interruptibly(&buf[..n], stop.as_ref()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn stdin->pty thread")
}

pub fn run(file: &Path, force: bool, route_owned: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Ensure session UUID exists in frontmatter
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session_for_file(&content, file)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    let (fm, _body) = frontmatter::parse_for_file(&updated_content, file)?;
    let global_config = config::load().unwrap_or_default();
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project_root = snapshot::find_project_root(&canonical).unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf())
    });
    match crate::project_controller::close_stale_starting_actors_for_caller(
        &project_root,
        std::time::Duration::from_secs(3600),
        false,
        "start",
    ) {
        Ok((closed, kept)) if closed > 0 => eprintln!(
            "[start] actors: {} stale starting closed, {} still active",
            closed, kept
        ),
        Ok(_) => {}
        Err(e) => eprintln!("[start] actor gc warning: {}", e),
    }

    // Resolve harness config from frontmatter agent > config default_agent > claude
    let harness = crate::harness::HarnessConfig::from_context(&fm, &global_config);
    let resolved_agent_args = resolve_agent_args(&fm, &global_config, &harness);

    // Must be inside tmux
    if !sessions::in_tmux() {
        // Distinguish "tmux not installed" from "not inside a tmux session"
        let tmux_installed = std::process::Command::new("which")
            .arg("tmux")
            .output()
            .is_ok_and(|o| o.status.success());
        if !tmux_installed {
            let hint = if cfg!(target_os = "macos") {
                "brew install tmux"
            } else if cfg!(target_os = "linux") {
                "sudo apt install tmux  # or: sudo pacman -S tmux / sudo dnf install tmux"
            } else {
                "Install WSL first, then: sudo apt install tmux"
            };
            anyhow::bail!(
                "tmux is not installed.\n\n  Install it:\n    {}\n\n  Then start a tmux session:\n    tmux new-session -s dev",
                hint
            );
        }
        anyhow::bail!(
            "not running inside tmux — start a tmux session first:\n    tmux new-session -s dev"
        );
    }

    let pane_id = sessions::current_pane()?;
    let tmux = sessions::Tmux::default_server();
    if let Some((miss, supersession)) = crate::startup_miss::take_superseded_startup_miss(file)? {
        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
        eprintln!(
            "[start] clearing stale startup-miss on pane {} from {} for {} because newer registered owner {} already took over",
            miss.pane_id,
            miss_ts,
            file.display(),
            supersession.registered_pane
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "start_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                file.display(),
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_start_timestamp
            ),
        );
    }
    let unresolved_startup_miss = crate::startup_miss::load(file).ok().flatten();

    if !force {
        if let Some(action) = existing_session_pane_action(&tmux, &session_id, file, &pane_id)? {
            match action {
                ExistingSessionPaneAction::Refuse(conflicting_pane) => {
                    if let Some(miss) = unresolved_startup_miss.as_ref()
                        && miss.pane_id == conflicting_pane
                    {
                        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
                        anyhow::bail!(
                            "startup-miss from {} still belongs to alive pane {} for {}.\n\n{}",
                            miss_ts,
                            conflicting_pane,
                            file.display(),
                            format_existing_pane_conflict_error(
                                &tmux,
                                file,
                                &pane_id,
                                &conflicting_pane
                            )
                        );
                    }
                    anyhow::bail!(
                        "{}",
                        format_existing_pane_conflict_error(
                            &tmux,
                            file,
                            &pane_id,
                            &conflicting_pane
                        )
                    );
                }
            }
        }
    } else {
        eprintln!(
            "[start] --force: bypassing existing session pane reuse for {}",
            file.display()
        );
    }

    // Only relocate the current launcher pane when start is actually falling through to
    // a fresh session in this pane. If a live owner already exists elsewhere, moving the
    // launcher pane first can rip it out of its original tmux window/session before the
    // reuse path returns.
    if let Some(expected_session) = config::project_tmux_session()
        && !relocate_if_wrong_session(&tmux, &pane_id, &expected_session)
    {
        rebind_project_tmux_session_if_expected_dead(&tmux, &pane_id, &expected_session);
    }

    // Register session → pane (with relative file path)
    let file_str = file.to_string_lossy();
    let supervisor_instance_id = uuid::Uuid::new_v4().to_string();
    let prior_entry = sessions::lookup_entry(&session_id)?;
    let pane_window = sessions::pane_window(&pane_id).unwrap_or_default();
    sessions::register_supervisor(
        &session_id,
        &pane_id,
        &file_str,
        std::process::id(),
        &supervisor_instance_id,
    )?;
    eprintln!("Registered session {} → pane {}", &session_id[..8], pane_id);

    // Open session log
    let mut session_log = open_session_log(&canonical, &session_id);
    let start_generation = if prior_entry
        .as_ref()
        .is_some_and(|entry| entry.pane != pane_id)
    {
        crate::session_actor::infer_latest_generation(&canonical, &session_id).unwrap_or(1)
    } else {
        let generations = crate::session_actor::next_generation(&canonical, &session_id).unwrap_or(
            crate::session_actor::OwnershipGeneration {
                prior_generation: 0,
                new_generation: 1,
            },
        );
        log_event(
            &mut session_log,
            &crate::session_actor::format_transition_event(
                crate::session_actor::OwnershipTransitionEvent {
                    caller: "start",
                    reason: "session_start",
                    prior_generation: generations.prior_generation,
                    new_generation: generations.new_generation,
                    old_pane: prior_entry.as_ref().map(|entry| entry.pane.as_str()),
                    new_pane: &pane_id,
                    old_window: prior_entry.as_ref().and_then(|entry| {
                        (!entry.window.is_empty()).then_some(entry.window.as_str())
                    }),
                    new_window: Some(pane_window.as_str()),
                },
            ),
        );
        generations.new_generation
    };
    log_event(
        &mut session_log,
        &format!(
            "session_start file={} pane={} session={} generation={}",
            file.display(),
            pane_id,
            &session_id[..8],
            start_generation
        ),
    );
    crate::project_controller::ensure_controller_running(
        &project_root,
        crate::project_controller::LaunchMode::Lazy,
    )?;
    let actor_record = crate::project_controller::start_session(
        &project_root,
        crate::project_controller::StartSessionRequest {
            file: canonical.clone(),
            session_id: session_id.clone(),
            pane_id: pane_id.clone(),
            window_id: pane_window.clone(),
            generation: start_generation,
        },
    )?;
    log_event(
        &mut session_log,
        &format!(
            "controller_session_start generation={} state={}",
            actor_record.generation,
            actor_record.state.as_str()
        ),
    );

    // Fire document-level session_start hooks
    let harness_name = agent_doc::model_tier::detect_harness();
    let resolved_model = fm
        .resolve_harness_model(&harness_name)
        .map(|s| s.to_string());
    crate::hooks::fire_doc_hooks(
        &fm.hooks,
        "session_start",
        file,
        &session_id,
        &fm.agent,
        &resolved_model,
    );

    // --- Snapshot integrity validation ---
    // If file was moved (JB plugin respawn after rename), the old path hash
    // won't match — migrate state files or bootstrap a fresh snapshot before
    // the IPC listener starts. Prevents CRDT corruption from stale state.
    match crate::snapshot::ensure_initialized(file) {
        Ok(true) => {
            log_event(&mut session_log, "snapshot_validated action=initialized");
            eprintln!("[start] snapshot integrity validated (initialized)");
        }
        Ok(false) => {
            log_event(&mut session_log, "snapshot_validated action=already_valid");
        }
        Err(e) => {
            log_event(
                &mut session_log,
                &format!("snapshot_validation_failed error={}", e),
            );
            eprintln!("[start] warning: snapshot validation failed: {}", e);
        }
    }

    // --- Supervisor setup ---

    // Resolve CWD deterministically
    let resolved_cwd = cwd::resolve(None, fm.cwd.as_deref(), &canonical)?;
    log_event(
        &mut session_log,
        &format!(
            "cwd_resolved path={} source={}",
            resolved_cwd.path.display(),
            resolved_cwd.source.as_str()
        ),
    );

    // Resolve env once at startup — reused across all restarts for determinism
    let env_spec = EnvSpec::from_frontmatter(&fm);
    let mut resolved_env = env_spec.resolve()?;
    if harness.supports_enable_tool_search && fm.enable_tool_search.unwrap_or(false) {
        resolved_env.insert("ENABLE_TOOL_SEARCH".into(), "true".into());
    }

    // Build base args (resolved once, reused across restarts)
    let mut base_args: Vec<String> = Vec::new();
    if let Some(ref args) = resolved_agent_args {
        base_args.extend(args.split_whitespace().map(String::from));
    }
    // Inject --model from harness-specific model frontmatter when not already in args.
    if !base_args.iter().any(|a| a == "--model") {
        let harness_key = match harness.binary.as_str() {
            "claude" => "claude-code",
            other => other,
        };
        if let Some(model) = fm.resolve_harness_model(harness_key) {
            base_args.push("--model".into());
            base_args.push(model.to_string());
        }
    }
    crate::agent::append_workspace_access_args(&harness.binary, &mut base_args, &canonical);
    if harness.supports_no_mcp && fm.no_mcp.unwrap_or(false) {
        base_args.push("--no-mcp".into());
    }
    if harness.binary == "codex" {
        let codex_network_access = crate::agent::resolve_codex_network_access(&fm, &global_config);
        crate::agent::apply_codex_network_access_env_map(&mut resolved_env, codex_network_access);
        let status = crate::agent::codex_network_status_from_env_map(
            &base_args,
            codex_network_access,
            &resolved_env,
        );
        eprintln!("[start] codex network access: {}", status.summary());
        if let Some(err) = status.mismatch_error() {
            anyhow::bail!(err);
        }
    }
    let capability_proof_required = crate::agent::codex::managed_capability_contract_required(
        &base_args,
        &fm,
        &global_config,
    );
    if !capability_proof_required {
        log_event(
            &mut session_log,
            &format!("{}_capability_proof status=not_required", harness.binary),
        );
    }

    // Query initial terminal size
    let initial_size = {
        #[cfg(unix)]
        {
            resize::query_terminal_size(libc::STDIN_FILENO)
                .map(|(rows, cols)| PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap_or(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
        }
        #[cfg(not(unix))]
        {
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            }
        }
    };

    let actor_runtime = SessionActorRuntime {
        project_root: project_root.clone(),
        file: canonical.clone(),
        session_id: session_id.clone(),
        pane_id: pane_id.clone(),
        generation: actor_record.generation,
    };

    // Create shared state for IPC handler
    let shared = Arc::new(SupervisorShared::with_actor_runtime(
        resolved_cwd.source.as_str(),
        supervisor_instance_id,
        Some(actor_runtime),
        Some(crate::session_actor::ActorState::Starting),
        Some(pane_id.clone()),
    ));
    let capability_proof_thread = if capability_proof_required {
        shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
        log_event(
            &mut session_log,
            &format!("{}_capability_proof status=pending", harness.binary),
        );
        Some(spawn_managed_capability_proof_thread(
            shared.clone(),
            harness.binary.clone(),
            base_args.clone(),
            resolved_env.clone(),
            fm.clone(),
            global_config.clone(),
            session_log.as_ref().and_then(|f| f.try_clone().ok()),
        ))
    } else {
        None
    };

    // Start IPC listener
    let shared_for_ipc = shared.clone();
    let mut ipc = SupervisorIpc::start(&project_root, &session_id, move |method| {
        handle_ipc(method, &shared_for_ipc)
    })?;
    log_event(
        &mut session_log,
        &format!("ipc_started project_root={}", project_root.display()),
    );
    let supervisor_socket = crate::supervisor::ipc::socket_path(&project_root, &session_id)
        .to_string_lossy()
        .to_string();
    crate::project_controller::register_supervisor(
        &project_root,
        crate::project_controller::SupervisorRegistration {
            file: canonical.clone(),
            session_id: session_id.clone(),
            pane_id: pane_id.clone(),
            generation: actor_record.generation,
            supervisor_pid: std::process::id(),
            supervisor_socket,
            runtime_state: crate::session_actor::ActorState::Starting
                .as_str()
                .to_string(),
        },
    )?;
    log_event(
        &mut session_log,
        "controller_supervisor_registered state=starting",
    );

    // Crash policy state machine
    let mut policy = CrashPolicy::new();
    let route_owned_cycle_baseline = if route_owned {
        crate::cycle_state::load(file).unwrap_or(None)
    } else {
        None
    };
    let route_owned_completion = Arc::new(AtomicBool::new(false));
    let route_owned_completion_stop = Arc::new(AtomicBool::new(false));
    let route_owned_completion_thread = if route_owned {
        log_event(&mut session_log, "route_owned_start enabled=true");
        Some(spawn_route_owned_completion_thread(
            shared.clone(),
            canonical.clone(),
            route_owned_cycle_baseline,
            route_owned_completion.clone(),
            route_owned_completion_stop.clone(),
            session_log.as_ref().and_then(|f| f.try_clone().ok()),
        ))
    } else {
        None
    };

    // Put stdin into raw mode so the outer pty's line discipline doesn't
    // mangle input bytes (e.g. ICRNL converting \r→\n). Claude Code sets
    // the inner pty slave to raw mode and expects \r for Enter — without
    // this, the outer pty's cooked mode silently converts \r to \n before
    // we even read it, breaking Enter for Claude Code's TUI.
    let raw_mode = RawMode::enable();

    // --- Supervisor restart loop ---
    let mut first_run = true;
    let mut restart_count: u32 = 0;
    let mut resize_watcher: Option<resize::ResizeWatcher> = None;
    let mut failed_resume_tracker = FailedResumeTracker::default();
    let mut suppress_stale_ctrl_d_until_prompt = false;
    let mut child_launch_count: u32 = 0;
    let supervisor_exit_reason = loop {
        if child_launch_count > 0 {
            let restart_reason = if first_run {
                "restart_fresh_spawn"
            } else {
                "restart_continue_spawn"
            };
            shared.transition_actor_state(
                crate::session_actor::ActorState::Busy,
                "supervisor",
                restart_reason,
            );
        }
        // Build args for this iteration
        let auto_trigger;
        let args = if !first_run {
            auto_trigger = true;
            let restart_args = harness.restart_args(&base_args)?;
            eprintln!("Restarting {} (continue)...", harness.binary);
            log_event(
                &mut session_log,
                &format!(
                    "{}_restart mode=continue restart_count={}",
                    harness.binary, restart_count
                ),
            );
            restart_args
        } else {
            auto_trigger = false;
            let args = base_args.clone();
            eprintln!("Starting {}...", harness.binary);
            log_event(
                &mut session_log,
                &format!(
                    "{}_start mode={} restart_count={}",
                    harness.binary,
                    if restart_count == 0 {
                        "fresh"
                    } else {
                        "fresh_restart"
                    },
                    restart_count
                ),
            );
            args
        };

        // Build PtySpawnConfig and spawn child under pty
        let cfg = PtySpawnConfig {
            program: harness.binary.clone(),
            args,
            cwd: resolved_cwd.path.clone(),
            env: resolved_env.clone(),
            size: initial_size,
        };
        child_launch_count += 1;
        let mut session = crate::supervisor::pty::PtySession::spawn(cfg)
            .with_context(|| format!("failed to spawn {}", harness.binary))?;

        // Extract writer and reader for shared I/O
        #[cfg(unix)]
        let pty_write_fd = session.dup_write_fd()?;
        let pty_writer = session.take_writer()?;
        let pty_reader = session.clone_reader()?;
        #[cfg(unix)]
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::with_raw_fd(
            pty_writer,
            pty_write_fd,
        )));
        #[cfg(not(unix))]
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(pty_writer)));

        // Update shared state
        *shared.inject_writer.lock().unwrap() = Some(writer_arc.clone());
        shared
            .child_pid
            .store(session.process_id().unwrap_or(0), Ordering::Relaxed);
        shared.running.store(true, Ordering::Relaxed);
        shared.restart_count.store(restart_count, Ordering::Relaxed);
        shared.restart_requested.store(false, Ordering::Relaxed);
        shared.stop_requested.store(false, Ordering::Relaxed);
        shared.ctrl_d_forwarded.store(false, Ordering::Relaxed);
        shared.ctrl_c_forwarded.store(false, Ordering::Relaxed);
        shared
            .auto_trigger_outcome
            .store(AutoTriggerOutcome::NotNeeded as u8, Ordering::Relaxed);
        shared.prompt_visible_once.store(false, Ordering::Relaxed);
        shared
            .suppress_stale_ctrl_d_until_prompt
            .store(suppress_stale_ctrl_d_until_prompt, Ordering::Relaxed);
        shared.recent_output.lock().unwrap().clear();

        // Spawn I/O forwarding threads
        let reader_thread = spawn_reader_thread(shared.clone(), harness.clone(), pty_reader);
        let writer_stop = StopSignal::new().context("failed to create writer stop signal")?;
        let writer_stop_flag = Arc::new(AtomicBool::new(false));
        let ctrl_c_flag = Arc::new(AtomicBool::new(false));
        let ctrl_d_flag = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let writer_thread = spawn_writer_thread(
            shared.clone(),
            writer_arc.clone(),
            writer_stop.read_fd(),
            writer_stop_flag.clone(),
            Some(ctrl_c_flag.clone()),
            Some(ctrl_d_flag.clone()),
        );
        #[cfg(not(unix))]
        let writer_thread = spawn_writer_thread(
            shared.clone(),
            writer_arc.clone(),
            (),
            writer_stop_flag.clone(),
            Some(ctrl_c_flag.clone()),
            Some(ctrl_d_flag.clone()),
        );

        // Start resize watcher (stop previous one first)
        if let Some(mut rw) = resize_watcher.take() {
            rw.stop();
        }
        let resize_handle = session.resize_handle()?;
        resize_watcher = resize::ResizeWatcher::spawn(move |size| {
            if let Err(e) = resize_handle.resize(size) {
                eprintln!("[supervisor::resize] resize error: {e}");
            }
        })
        .ok();

        // For restarts, poll for agent prompt then re-send trigger command
        let mut auto_trigger_thread: Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)> = None;
        if auto_trigger {
            shared
                .auto_trigger_outcome
                .store(AutoTriggerOutcome::Pending as u8, Ordering::Relaxed);
            let trigger_stop = Arc::new(AtomicBool::new(false));
            let trigger_log = session_log.as_ref().and_then(|f| f.try_clone().ok());
            let handle = spawn_auto_trigger_thread(
                shared.clone(),
                trigger_stop.clone(),
                file.to_string_lossy().to_string(),
                harness.clone(),
                trigger_log,
            );
            auto_trigger_thread = Some((trigger_stop, handle));
        }

        // Block until child exits
        let status = session
            .wait()
            .with_context(|| format!("failed waiting on {}", harness.binary))?;
        first_run = false;

        if let Some((stop, _)) = auto_trigger_thread.as_ref() {
            stop.store(true, Ordering::Relaxed);
        }
        writer_stop_flag.store(true, Ordering::Relaxed);

        // Stop the stdin→pty writer thread so stdin is free for the restart
        // prompt (or for the next iteration's fresh writer thread).
        writer_stop.signal();
        let _ = writer_thread.join();
        if let Some((_, handle)) = auto_trigger_thread.take() {
            let _ = handle.join();
        }
        if ctrl_d_flag.load(Ordering::Relaxed) {
            shared.ctrl_d_forwarded.store(true, Ordering::Relaxed);
        }
        if ctrl_c_flag.load(Ordering::Relaxed) {
            shared.ctrl_c_forwarded.store(true, Ordering::Relaxed);
        }

        // Clean up shared state (must happen before dropping session so the
        // inject_writer Arc is released before the pty master closes).
        shared.running.store(false, Ordering::Relaxed);
        *shared.inject_writer.lock().unwrap() = None;
        shared.child_pid.store(0, Ordering::Relaxed);

        // Drop the session to close the pty master. The reader thread holds a
        // cloned reader fd — closing the master causes its read() to return
        // EOF so the thread can exit cleanly.
        drop(session);
        let _ = reader_thread.join();

        // Flush any stale stdin bytes that the writer thread consumed from the
        // kernel but couldn't forward (e.g., user pressed Enter during the
        // tiny race window between session.wait() and writer_stop.signal()).
        #[cfg(unix)]
        unsafe {
            libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
        }

        // Check IPC-requested stop
        if route_owned_completion.load(Ordering::Relaxed) {
            log_event(&mut session_log, "route_owned_cycle_complete_stop");
            break "route_owned_cycle_complete";
        }

        if shared.stop_requested.load(Ordering::Relaxed) {
            log_event(&mut session_log, "ipc_stop");
            break "ipc_stop";
        }

        // Check IPC-requested restart (override normal exit classification)
        if shared.restart_requested.load(Ordering::Relaxed) {
            let mode = shared.restart_mode.lock().unwrap().clone();
            first_run = mode == "fresh";
            restart_count += 1;
            log_event(
                &mut session_log,
                &format!("ipc_restart mode={} restart_count={}", mode, restart_count),
            );
            continue;
        }

        // Normal exit classification via CrashPolicy
        let code = status.exit_code() as i32;
        let exit_provenance = exit_provenance_fields(&status);
        log_event(
            &mut session_log,
            &format!(
                "{}_exit code={} restart_count={} {}",
                harness.binary, code, restart_count, exit_provenance
            ),
        );
        let auto_trigger_outcome =
            AutoTriggerOutcome::from_u8(shared.auto_trigger_outcome.load(Ordering::Relaxed));
        let prompt_visible_once = shared.prompt_visible_once.load(Ordering::Relaxed);

        let ctrl_c_forwarded_interrupt = is_forwarded_ctrl_c_interrupt_exit(
            &status,
            shared.ctrl_c_forwarded.load(Ordering::Relaxed),
        );
        let ctrl_d_forwarded = shared.ctrl_d_forwarded.load(Ordering::Relaxed);
        let failed_resume =
            resume_handoff_failed(auto_trigger, ctrl_d_forwarded, auto_trigger_outcome);
        let clean_exit_before_prompt =
            clean_exit_before_prompt_seen(auto_trigger, prompt_visible_once);
        if matches!(
            auto_trigger_outcome,
            AutoTriggerOutcome::Sent | AutoTriggerOutcome::NotNeeded
        ) {
            failed_resume_tracker.reset();
        }

        // Forwarded operator Ctrl+C is an intentional shutdown request, not a
        // supervisor crash signal, so keep the policy state on the clean-exit
        // path and surface the same restart/quit prompt as Ctrl+D.
        let policy_exit_code = policy_exit_code_for_supervisor(code, ctrl_c_forwarded_interrupt);
        let action = policy.on_exit(policy_exit_code);
        *shared.supervisor_state.lock().unwrap() = policy.state;
        let action_name = match &action {
            RestartAction::PromptUser => "prompt_user",
            RestartAction::RestartAfter { .. } => "restart_after",
            RestartAction::Halt => "halt",
        };
        log_event(
            &mut session_log,
            &format!(
                "restart_eval pane={} harness={} exit_code={} {} auto_trigger_outcome={} ctrl_d={} state={} action={}",
                pane_id,
                harness.binary,
                code,
                exit_provenance,
                auto_trigger_outcome.as_str(),
                ctrl_d_forwarded,
                policy.state.as_str(),
                action_name
            ),
        );

        match action {
            RestartAction::PromptUser => {
                match clean_exit_resolution(&harness) {
                    CleanExitResolution::PromptUser => {
                        shared.transition_actor_state(
                            crate::session_actor::ActorState::WaitingInput,
                            "supervisor",
                            "clean_exit_prompt",
                        );
                        // Temporarily restore cooked mode so read_line() works with
                        // normal line editing (echo, backspace, etc.)
                        raw_mode.suspend();
                        eprintln!("\n{} exited cleanly.", harness.binary);
                        match prompt_for_restart_or_quit(
                            &mut session_log,
                            "clean_exit",
                            "Press Enter to restart, or 'q' to exit.",
                            "user_quit",
                            PromptEofPolicy::Quit,
                        ) {
                            PromptOutcome::Quit => {
                                break "user_quit_clean_exit";
                            }
                            PromptOutcome::RestartFresh => {
                                raw_mode.resume();
                                first_run = true;
                                restart_count += 1;
                                suppress_stale_ctrl_d_until_prompt = false;
                            }
                        }
                    }
                    CleanExitResolution::RestartContinue => {
                        let recent_failures = if failed_resume {
                            let now = Instant::now();
                            let recent_failures = failed_resume_tracker.record(now);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "resume_restart_failed pane={} harness={} outcome={} recent_failures={} window_secs={} restart_count={}",
                                    pane_id,
                                    harness.binary,
                                    auto_trigger_outcome.as_str(),
                                    recent_failures,
                                    FAILED_RESUME_WINDOW.as_secs(),
                                    restart_count
                                ),
                            );
                            recent_failures
                        } else {
                            0
                        };

                        match restart_continue_exit_strategy(
                            ctrl_c_forwarded_interrupt,
                            failed_resume,
                            ctrl_d_forwarded,
                            recent_failures,
                            clean_exit_before_prompt,
                        ) {
                            RestartContinueExitStrategy::CtrlCPromptUser => {
                                shared.transition_actor_state(
                                    crate::session_actor::ActorState::WaitingInput,
                                    "supervisor",
                                    "ctrl_c_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!("\n{} exited after stdin Ctrl+C.", harness.binary);
                                log_event(
                                    &mut session_log,
                                    &format!("ctrl_c_prompt_user restart_count={}", restart_count),
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "ctrl_c",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_ctrl_c",
                                    PromptEofPolicy::Quit,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_ctrl_c";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            RestartContinueExitStrategy::CtrlDPromptUser => {
                                shared.transition_actor_state(
                                    crate::session_actor::ActorState::WaitingInput,
                                    "supervisor",
                                    "ctrl_d_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!("\n{} exited after stdin EOF/Ctrl-D.", harness.binary);
                                log_event(
                                    &mut session_log,
                                    &format!("ctrl_d_prompt_user restart_count={}", restart_count),
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "ctrl_d",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_ctrl_d",
                                    PromptEofPolicy::Quit,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_ctrl_d";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            RestartContinueExitStrategy::PromptUser => {
                                shared.transition_actor_state(
                                    crate::session_actor::ActorState::WaitingInput,
                                    "supervisor",
                                    "resume_failure_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!(
                                    "\n{} failed to re-establish a prompt after resume {} times in the last {}s.",
                                    harness.binary,
                                    recent_failures,
                                    FAILED_RESUME_WINDOW.as_secs()
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "resume_failure",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_resume_failure",
                                    PromptEofPolicy::RestartFresh,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_resume_failure";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            RestartContinueExitStrategy::RestartFresh => {
                                suppress_stale_ctrl_d_until_prompt = false;
                                if clean_exit_before_prompt {
                                    eprintln!(
                                        "\n{} exited cleanly before ever surfacing a prompt. Restarting fresh instead of resuming...",
                                        harness.binary
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "fresh_restart_before_prompt restart_count={}",
                                            restart_count + 1
                                        ),
                                    );
                                } else {
                                    eprintln!(
                                        "\n{} exited after a failed resume handoff ({}). Restarting fresh instead of resuming...",
                                        harness.binary,
                                        auto_trigger_outcome.as_str()
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "resume_restart_fresh outcome={} restart_count={}",
                                            auto_trigger_outcome.as_str(),
                                            restart_count + 1
                                        ),
                                    );
                                }
                                first_run = true;
                                restart_count += 1;
                            }
                            RestartContinueExitStrategy::Resume => {
                                suppress_stale_ctrl_d_until_prompt = false;
                                eprintln!(
                                    "\n{} exited cleanly. Restarting in resume mode to keep the session attached...",
                                    harness.binary
                                );
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "auto_restart_clean with_continue=true restart_count={}",
                                        restart_count + 1
                                    ),
                                );
                                restart_count += 1;
                                continue;
                            }
                        }
                    }
                }
            }
            RestartAction::RestartAfter {
                delay,
                with_continue,
            } => {
                eprintln!(
                    "\n{} exited with code {}. Restarting in {:?}...",
                    harness.binary, code, delay
                );
                log_event(
                    &mut session_log,
                    &format!(
                        "auto_restart delay={:?} with_continue={} restart_count={}",
                        delay,
                        with_continue,
                        restart_count + 1
                    ),
                );
                std::thread::sleep(delay);
                if !with_continue {
                    first_run = true;
                }
                restart_count += 1;
                suppress_stale_ctrl_d_until_prompt = false;
            }
            RestartAction::Halt => {
                shared.transition_actor_state(
                    crate::session_actor::ActorState::Blocked,
                    "supervisor",
                    "supervisor_halted",
                );
                eprintln!(
                    "\nSupervisor halted after {} restarts (flapping detected).",
                    restart_count
                );
                log_event(&mut session_log, "supervisor_halted");
                break "supervisor_halted";
            }
        }
    };

    // Restore terminal to original mode before cleanup
    drop(raw_mode);
    route_owned_completion_stop.store(true, Ordering::Relaxed);
    if let Some(handle) = route_owned_completion_thread {
        let _ = handle.join();
    }
    if let Some(handle) = capability_proof_thread {
        let _ = handle.join();
    }

    // Cleanup
    if let Some(mut rw) = resize_watcher.take() {
        rw.stop();
    }
    ipc.stop();
    shared.transition_actor_state(
        crate::session_actor::ActorState::Closed,
        "supervisor",
        supervisor_exit_reason,
    );
    log_event(
        &mut session_log,
        &format!(
            "supervisor_exit reason={} pane={} restart_count={}",
            supervisor_exit_reason, pane_id, restart_count
        ),
    );
    log_event(&mut session_log, "session_end");
    eprintln!("Session ended for {}", file.display());
    if route_owned && route_owned_completion.load(Ordering::Relaxed) {
        log_event(
            &mut session_log,
            &format!("route_owned_reap_pane pane={}", pane_id),
        );
        eprintln!(
            "[start] route-owned cycle committed for {}; reaping pane {}",
            file.display(),
            pane_id
        );
        let _ = tmux.raw_cmd(&["kill-pane", "-t", &pane_id]);
    }
    Ok(())
}

fn resolve_agent_args(
    fm: &frontmatter::Frontmatter,
    global_config: &config::Config,
    harness: &crate::harness::HarnessConfig,
) -> Option<String> {
    match harness.binary.as_str() {
        "claude" => fm
            .agent_args
            .clone()
            .or_else(|| fm.claude_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.claude_args.clone())
            .or_else(|| std::env::var("AGENT_DOC_CLAUDE_ARGS").ok()),
        "codex" => fm
            .agent_args
            .clone()
            .or_else(|| fm.codex_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.codex_args.clone()),
        "opencode" => fm
            .agent_args
            .clone()
            .or_else(|| fm.opencode_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.opencode_args.clone()),
        _ => fm
            .agent_args
            .clone()
            .or_else(|| global_config.agent_args.clone()),
    }
}

/// Auto-relocate `pane_id` to `expected_session` if it is currently in a different session.
/// Returns `true` if relocation succeeded or was unnecessary; `false` if relocation failed.
/// Falls back to warn-only on failure so the start isn't aborted.
pub(crate) fn relocate_if_wrong_session(
    tmux: &sessions::Tmux,
    pane_id: &str,
    expected_session: &str,
) -> bool {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(s) => s,
        Err(_) => return true, // can't determine — let registration proceed
    };
    if actual_session == expected_session {
        return true;
    }
    eprintln!(
        "[start] pane {} is in session '{}', expected '{}' — auto-relocating to project session",
        pane_id, actual_session, expected_session
    );
    if let Some(anchor) = tmux.active_pane(expected_session) {
        match sessions::PaneMoveOp::new(tmux, pane_id, &anchor)
            .allow_cross_session("auto-relocate to project session on start")
            .join("-dh")
        {
            Ok(()) => {
                eprintln!(
                    "[start] relocated pane {} → session '{}'",
                    pane_id, expected_session
                );
                true
            }
            Err(e) => {
                eprintln!(
                    "[start] WARNING: relocation failed ({}); pane {} will register in session '{}'",
                    e, pane_id, actual_session
                );
                false
            }
        }
    } else {
        eprintln!(
            "[start] WARNING: no active pane found in session '{}'; \
             pane {} will register in session '{}'",
            expected_session, pane_id, actual_session
        );
        false
    }
}

fn rebind_project_tmux_session_if_expected_dead(
    tmux: &sessions::Tmux,
    pane_id: &str,
    expected_session: &str,
) {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(session) => session,
        Err(_) => return,
    };
    if actual_session == expected_session || tmux.session_alive(expected_session) {
        return;
    }
    match config::update_project_tmux_session(&actual_session) {
        Ok(()) => eprintln!(
            "[start] configured project session '{}' is dead — rebound tmux_session to '{}'",
            expected_session, actual_session
        ),
        Err(e) => eprintln!(
            "[start] WARNING: configured project session '{}' is dead but failed to persist tmux_session '{}': {}",
            expected_session, actual_session, e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::Frontmatter;
    use crate::hooks::fire_doc_hooks;
    use crate::project_config;
    use crate::sessions::IsolatedTmux;
    use std::collections::HashMap;
    use tempfile::TempDir;

    struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: std::sync::MutexGuard<'static, ()>,
    }

    impl ScopedCurrentDir {
        fn set(path: &std::path::Path) -> Self {
            let env_guard = crate::test_support::env_lock();
            let prev_cwd = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: String) -> Self {
            let previous = std::env::var(key).ok();
            unsafe { std::env::set_var(key, &value) };
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            if let Some(value) = self.previous.as_deref() {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn tmux_env_for_server(iso: &IsolatedTmux) -> String {
        let output = iso
            .cmd()
            .args(["display-message", "-p", "#{socket_path}"])
            .output()
            .expect("tmux should report its socket path");
        assert!(
            output.status.success(),
            "failed to query tmux socket path: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let socket_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        format!("{socket_path},0,0")
    }

    #[test]
    fn resolve_agent_args_claude_prefers_claude_alias_chain() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn resolve_agent_args_claude_prefers_agent_args_over_claude_args() {
        let fm = Frontmatter {
            agent_args: Some("--model sonnet".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config::default();
        let harness = crate::harness::HarnessConfig::claude();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--model sonnet"));
    }

    #[test]
    fn resolve_agent_args_codex_prefers_codex_alias_chain() {
        let fm = Frontmatter {
            codex_args: Some("-s danger-full-access".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            codex_args: Some("-s workspace-write".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_agent_args_codex_ignores_claude_args_aliases() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_agent_args_codex_uses_agent_args_only() {
        let fm = Frontmatter {
            agent_args: Some("-s danger-full-access".into()),
            codex_args: Some("-s workspace-write".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            agent_args: Some("-s workspace-write".into()),
            codex_args: Some("-s read-only".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_agent_args_codex_uses_config_codex_args_fallback() {
        let fm = Frontmatter::default();
        let cfg = Config {
            codex_args: Some("-s danger-full-access".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_agent_args_opencode_prefers_opencode_alias_chain() {
        let fm = Frontmatter {
            opencode_args: Some("--dangerously-skip-permissions".into()),
            codex_args: Some("-s danger-full-access".into()),
            claude_args: Some("--old-claude".into()),
            ..Default::default()
        };
        let cfg = Config {
            opencode_args: Some("--from-config".into()),
            codex_args: Some("-s workspace-write".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::opencode();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn resolve_agent_args_opencode_ignores_claude_and_codex_aliases() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            codex_args: Some("-s danger-full-access".into()),
            ..Default::default()
        };
        let cfg = Config {
            claude_args: Some("--old-flag".into()),
            codex_args: Some("-s workspace-write".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::opencode();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_agent_args_opencode_uses_config_opencode_args_fallback() {
        let fm = Frontmatter::default();
        let cfg = Config {
            opencode_args: Some("--dangerously-skip-permissions".into()),
            claude_args: Some("--old-flag".into()),
            codex_args: Some("-s workspace-write".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::opencode();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--dangerously-skip-permissions"));
    }

    // --- model injection from frontmatter tests ---

    /// Helper: simulates the base_args construction logic from run() for testing
    /// model injection without spawning a real process.
    fn build_base_args_for_test(
        fm: &Frontmatter,
        harness: &crate::harness::HarnessConfig,
    ) -> Vec<String> {
        let cfg = Config::default();
        let resolved_agent_args = resolve_agent_args(fm, &cfg, harness);
        let mut base_args: Vec<String> = Vec::new();
        if let Some(ref args) = resolved_agent_args {
            base_args.extend(args.split_whitespace().map(String::from));
        }
        if !base_args.iter().any(|a| a == "--model") {
            let harness_key = match harness.binary.as_str() {
                "claude" => "claude-code",
                other => other,
            };
            if let Some(model) = fm.resolve_harness_model(harness_key) {
                base_args.push("--model".into());
                base_args.push(model.to_string());
            }
        }
        base_args
    }

    #[test]
    fn model_injected_from_claude_model_frontmatter() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            claude_model: Some("claude-opus-4-6".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"claude-opus-4-6".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn model_not_injected_when_already_in_claude_args() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions --model sonnet".into()),
            claude_model: Some("claude-opus-4-6".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let args = build_base_args_for_test(&fm, &harness);
        // Should use the explicit --model from claude_args, not inject from claude_model
        assert!(args.contains(&"sonnet".to_string()));
        assert!(!args.contains(&"claude-opus-4-6".to_string()));
    }

    #[test]
    fn model_injected_from_codex_model_frontmatter() {
        let fm = Frontmatter {
            codex_args: Some("-s danger-full-access".into()),
            codex_model: Some("o3-pro".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"o3-pro".to_string()));
    }

    #[test]
    fn model_injected_from_opencode_model_frontmatter() {
        let fm = Frontmatter {
            opencode_args: Some("--dangerously-skip-permissions".into()),
            opencode_model: Some("zai/glm-5".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::opencode();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"zai/glm-5".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn model_injected_from_generic_model_when_no_harness_specific() {
        let fm = Frontmatter {
            model: Some("gpt-5".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"gpt-5".to_string()));
    }

    #[test]
    fn no_model_injected_when_none_in_frontmatter() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(!args.contains(&"--model".to_string()));
    }

    // --- relocate_if_wrong_session tests ---

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_noop_when_already_correct_session() {
        let iso = IsolatedTmux::new("start-reloc-noop");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // pane is already in sess-a; no relocation needed
        let result = relocate_if_wrong_session(&iso, &pane, "sess-a");
        assert!(
            result,
            "should return true (noop — already in correct session)"
        );
        // Verify pane is still in sess-a
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(sess, "sess-a");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_succeeds_cross_session() {
        let iso = IsolatedTmux::new("start-reloc-cross");
        let _pane_a = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        let pane_b = iso
            .new_session("sess-b", std::path::Path::new("/tmp"))
            .unwrap();
        // pane_b is in sess-b; expected is sess-a — should auto-relocate
        let result = relocate_if_wrong_session(&iso, &pane_b, "sess-a");
        assert!(result, "should return true after successful relocation");
        let sess = iso.pane_session(&pane_b).unwrap();
        assert_eq!(sess, "sess-a", "pane should be in sess-a after relocation");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_fails_gracefully_when_no_anchor() {
        let iso = IsolatedTmux::new("start-reloc-noanchor");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // Expected session "sess-nonexistent" has no active pane — relocation should fail gracefully
        let result = relocate_if_wrong_session(&iso, &pane, "sess-nonexistent");
        assert!(
            !result,
            "should return false when no anchor pane exists in expected session"
        );
        // pane should still be in original session
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(
            sess, "sess-a",
            "pane should remain in original session on failure"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn start_rebinds_dead_project_session_to_current_pane_session() {
        let dir = TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("start-rebind-dead-session");
        let pane = iso.new_session("14", dir.path()).unwrap();

        let relocated = relocate_if_wrong_session(&iso, &pane, "0");
        assert!(
            !relocated,
            "missing anchor in dead configured session should fall back to current pane session"
        );

        rebind_project_tmux_session_if_expected_dead(&iso, &pane, "0");

        assert_eq!(
            project_config::project_tmux_session().as_deref(),
            Some("14")
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn start_does_not_rebind_live_project_session_pin() {
        let dir = TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("start-rebind-live-session");
        let _expected_pane = iso.new_session("0", dir.path()).unwrap();
        let pane = iso.new_session("14", dir.path()).unwrap();

        rebind_project_tmux_session_if_expected_dead(&iso, &pane, "0");

        assert_eq!(project_config::project_tmux_session().as_deref(), Some("0"));
    }

    #[test]
    fn fire_doc_hooks_substitutes_template_vars() {
        let tmp =
            std::env::temp_dir().join(format!("agent-doc-hook-test-{}.txt", std::process::id()));
        let cmd = format!(
            "echo '{{{{session_id}}}}:{{{{agent}}}}:{{{{model}}}}' > {}",
            tmp.display()
        );
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "abc-123",
            &Some("claude".to_string()),
            &Some("opus".to_string()),
        );
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(
            output.contains("abc-123"),
            "session_id not substituted: {}",
            output
        );
        assert!(
            output.contains("claude"),
            "agent not substituted: {}",
            output
        );
        assert!(output.contains("opus"), "model not substituted: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fire_doc_hooks_noop_for_missing_event() {
        let hooks: HashMap<String, Vec<String>> = HashMap::new();
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
    }

    #[test]
    fn fire_doc_hooks_noop_for_empty_event() {
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
    }

    #[test]
    fn fire_doc_hooks_handles_none_agent_model() {
        let tmp = std::env::temp_dir().join(format!(
            "agent-doc-hook-none-test-{}.txt",
            std::process::id()
        ));
        let cmd = format!("printf '{{{{agent}}}}:{{{{model}}}}' > {}", tmp.display());
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(output, ":", "expected empty agent+model, got: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_refuses_proven_live_owner() {
        let iso = IsolatedTmux::new("start-duplicate-live-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-live-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action =
            existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), Some(&pane_a));
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(pane_a.clone()))
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_refusal_keeps_launcher_pane_in_original_session() {
        let iso = IsolatedTmux::new("start-reuse-keeps-launcher-session");
        let tmp = tempfile::TempDir::new().unwrap();
        let owner_pane = iso.new_session("sess-a", tmp.path()).unwrap();
        let launcher_pane = iso.new_session("sess-b", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: owner_pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-reuse-keeps-launcher-session".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&owner_pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(
            &iso,
            &launcher_pane,
            Some(&entry),
            Some(&owner_pane),
        );
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(owner_pane.clone()))
        );
        assert_eq!(
            iso.pane_session(&launcher_pane).unwrap(),
            "sess-b",
            "refusing an existing live owner must not relocate the launcher pane"
        );
        assert_eq!(iso.pane_session(&owner_pane).unwrap(), "sess-a");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_ignores_same_pane() {
        let iso = IsolatedTmux::new("start-duplicate-same-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-same-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_refuses_alive_stale_registration_without_owner() {
        let iso = IsolatedTmux::new("start-stale-alive-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-stale-alive-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), None);
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(pane_a.clone()))
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_ignores_dead_registered_pane() {
        let iso = IsolatedTmux::new("start-duplicate-dead-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: "%999999".to_string(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-dead-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn format_existing_pane_conflict_error_includes_manual_tmux_commands() {
        let iso = IsolatedTmux::new("start-conflict-error");
        let tmp = TempDir::new().unwrap();
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        let launcher_pane = iso.split_window(&owner_pane, tmp.path(), "-dh").unwrap();
        let doc = tmp.path().join("tasks/software/corky.md");
        let rendered = format_existing_pane_conflict_error(&iso, &doc, &launcher_pane, &owner_pane);
        assert!(rendered.contains("tmux list-panes -a"));
        assert!(rendered.contains(&format!("tmux kill-pane -t {}", launcher_pane)));
        assert!(rendered.contains(&format!("tmux kill-pane -t {}", owner_pane)));
        assert!(rendered.contains(&owner_pane));
        assert!(rendered.contains(&launcher_pane));
    }

    #[test]
    fn clean_exit_resolution_prompts_for_claude() {
        assert_eq!(
            clean_exit_resolution(&crate::harness::HarnessConfig::claude()),
            CleanExitResolution::PromptUser
        );
    }

    #[test]
    fn clean_exit_resolution_auto_restarts_for_codex() {
        assert_eq!(
            clean_exit_resolution(&crate::harness::HarnessConfig::codex()),
            CleanExitResolution::RestartContinue
        );
    }

    #[test]
    fn clean_exit_resolution_auto_restarts_for_opencode() {
        assert_eq!(
            clean_exit_resolution(&crate::harness::HarnessConfig::opencode()),
            CleanExitResolution::RestartContinue
        );
    }

    #[test]
    fn start_invalid_frontmatter_returns_contextual_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("bad.md");
        std::fs::write(&file, "---\nprompt_presets:\n  key: [oops\n---\n").unwrap();

        let err = run(&file, false, false).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("invalid YAML frontmatter in"));
        assert!(message.contains("bad.md"));
        assert!(message.contains("Frontmatter excerpt:"));
        assert!(message.contains("> 2 |   key: [oops"));
        assert!(
            message.contains("Fix the frontmatter between the opening and closing --- markers")
        );
    }

    fn test_cycle(
        id: &str,
        phase: crate::cycle_state::CyclePhase,
        updated_at: u64,
    ) -> crate::cycle_state::CycleState {
        crate::cycle_state::CycleState {
            cycle_id: id.to_string(),
            file: "doc.md".to_string(),
            phase,
            last_event: format!("{:?}", phase),
            started_at: 1,
            updated_at,
            snapshot_hash: None,
            file_hash: None,
            normalized_snapshot_hash: None,
            normalized_file_hash: None,
            capture_id: None,
            response_sha256: None,
            had_pending_mutations: false,
            requires_backlog_capture: false,
            required_backlog_targets: Vec::new(),
            required_explicit_backlog_item_count: 0,
            required_plan_reference_count: 0,
            pending_done_ids: Vec::new(),
            reaped_pending_ids: Vec::new(),
        }
    }

    #[test]
    fn route_owned_cycle_completion_ignores_unchanged_committed_baseline() {
        let baseline = test_cycle("cycle-1", crate::cycle_state::CyclePhase::Committed, 10);
        let current = baseline.clone();

        assert!(
            !route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "a stale committed cycle from before route-owned start must not reap the pane"
        );
    }

    #[test]
    fn route_owned_cycle_completion_detects_new_committed_cycle() {
        let baseline = test_cycle("cycle-1", crate::cycle_state::CyclePhase::Committed, 10);
        let current = test_cycle("cycle-2", crate::cycle_state::CyclePhase::Committed, 20);

        assert!(
            route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "a newer committed cycle should stop and reap a route-owned pane"
        );
    }

    #[test]
    fn route_owned_cycle_completion_waits_while_new_cycle_open() {
        let baseline = test_cycle("cycle-1", crate::cycle_state::CyclePhase::Committed, 10);
        let current = test_cycle("cycle-2", crate::cycle_state::CyclePhase::WriteApplied, 20);

        assert!(
            !route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "route-owned panes should stay alive for debugging until the new cycle commits"
        );
    }

    #[test]
    fn restart_continue_strategy_prefers_resume_by_default() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, false, 0, false),
            RestartContinueExitStrategy::Resume
        );
    }

    #[test]
    fn forwarded_ctrl_c_uses_clean_exit_code_for_policy() {
        assert_eq!(policy_exit_code_for_supervisor(1, true), 0);
        assert_eq!(policy_exit_code_for_supervisor(130, true), 0);
        assert_eq!(policy_exit_code_for_supervisor(1, false), 1);
    }

    #[test]
    fn restart_continue_strategy_prompts_after_forwarded_ctrl_c_interrupt() {
        assert_eq!(
            restart_continue_exit_strategy(true, false, false, 0, false),
            RestartContinueExitStrategy::CtrlCPromptUser
        );
    }

    #[test]
    fn restart_continue_strategy_prompts_after_ctrl_d() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, true, 0, false),
            RestartContinueExitStrategy::CtrlDPromptUser
        );
    }

    #[test]
    fn restart_continue_strategy_still_prompts_after_ctrl_d_before_prompt() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, true, 0, true),
            RestartContinueExitStrategy::CtrlDPromptUser
        );
    }

    #[test]
    fn restart_continue_strategy_restarts_fresh_before_prompt_without_ctrl_d() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, false, 0, true),
            RestartContinueExitStrategy::RestartFresh
        );
    }

    #[test]
    fn strip_stale_ctrl_d_before_prompt_drops_inherited_ctrl_d_bytes() {
        let filtered =
            strip_stale_ctrl_d_before_prompt(b"\x04status\x04", true, false).expect("filtered");
        assert_eq!(filtered, b"status");
    }

    #[test]
    fn strip_stale_ctrl_d_before_prompt_keeps_ctrl_d_once_prompt_is_visible() {
        assert!(
            strip_stale_ctrl_d_before_prompt(b"\x04", true, true).is_none(),
            "prompt-visible children should still receive a fresh Ctrl+D"
        );
        assert!(
            strip_stale_ctrl_d_before_prompt(b"\x04", false, false).is_none(),
            "non-keepalive runs should not rewrite forwarded Ctrl+D"
        );
    }

    #[test]
    fn restart_continue_strategy_restarts_fresh_after_single_failed_resume() {
        assert_eq!(
            restart_continue_exit_strategy(false, true, false, 1, false),
            RestartContinueExitStrategy::RestartFresh
        );
    }

    #[test]
    fn restart_continue_strategy_prompts_after_repeated_failed_resumes() {
        assert_eq!(
            restart_continue_exit_strategy(false, true, false, FAILED_RESUME_THRESHOLD, false,),
            RestartContinueExitStrategy::PromptUser
        );
    }

    #[test]
    fn restart_continue_strategy_restarts_fresh_when_clean_exit_happens_before_prompt() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, false, 0, true),
            RestartContinueExitStrategy::RestartFresh
        );
    }

    #[test]
    fn ctrl_d_flag_initialized_false() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        assert!(!shared.ctrl_d_forwarded.load(Ordering::Relaxed));
    }

    #[test]
    fn ctrl_c_flag_initialized_false() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        assert!(!shared.ctrl_c_forwarded.load(Ordering::Relaxed));
    }

    #[test]
    fn auto_trigger_outcome_defaults_to_not_needed() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        assert_eq!(
            AutoTriggerOutcome::from_u8(shared.auto_trigger_outcome.load(Ordering::Relaxed)),
            AutoTriggerOutcome::NotNeeded
        );
    }

    #[test]
    fn failed_resume_tracker_prunes_old_events() {
        let mut tracker = FailedResumeTracker::default();
        let now = Instant::now();
        tracker
            .events
            .push_back(now - FAILED_RESUME_WINDOW - Duration::from_secs(1));
        tracker.events.push_back(now - Duration::from_secs(5));
        let count = tracker.record(now);
        assert_eq!(count, 2, "only recent failures should remain in the window");
    }

    #[test]
    fn ctrl_d_overrides_codex_auto_restart() {
        let harness = crate::harness::HarnessConfig::codex();
        assert_eq!(
            clean_exit_resolution(&harness),
            CleanExitResolution::RestartContinue
        );
        assert_eq!(
            restart_continue_exit_strategy(false, false, true, 0, false),
            RestartContinueExitStrategy::CtrlDPromptUser
        );
    }

    #[test]
    fn ctrl_c_interrupt_overrides_codex_auto_restart() {
        let harness = crate::harness::HarnessConfig::codex();
        assert_eq!(
            clean_exit_resolution(&harness),
            CleanExitResolution::RestartContinue
        );
        assert_eq!(
            restart_continue_exit_strategy(true, false, false, 0, false),
            RestartContinueExitStrategy::CtrlCPromptUser
        );
    }

    #[test]
    fn ctrl_d_with_failed_resume_still_prompts_when_run_did_not_commit() {
        assert_eq!(
            restart_continue_exit_strategy(false, true, true, 1, false),
            RestartContinueExitStrategy::CtrlDPromptUser
        );
    }

    #[test]
    fn ctrl_d_with_failed_resume_still_prompts_even_when_clean_exit_was_early() {
        assert_eq!(
            restart_continue_exit_strategy(false, true, true, 1, true),
            RestartContinueExitStrategy::CtrlDPromptUser
        );
    }

    #[test]
    fn clean_exit_before_prompt_seen_only_applies_to_fresh_runs() {
        assert!(clean_exit_before_prompt_seen(false, false));
        assert!(!clean_exit_before_prompt_seen(false, true));
        assert!(!clean_exit_before_prompt_seen(true, false));
    }

    #[test]
    fn resume_handoff_failed_treats_cancelled_resume_as_failure() {
        assert!(resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::Cancelled
        ));
        assert!(resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::Pending
        ));
        assert!(resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::Timeout
        ));
        assert!(resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::SendFailed
        ));
        assert!(!resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::Sent
        ));
        assert!(!resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::NotNeeded
        ));
    }

    #[test]
    fn resume_handoff_failed_ignores_ctrl_d_shutdown() {
        assert!(!resume_handoff_failed(
            true,
            true,
            AutoTriggerOutcome::Cancelled
        ));
        assert!(!resume_handoff_failed(
            false,
            false,
            AutoTriggerOutcome::Cancelled
        ));
    }

    #[test]
    fn classify_prompt_decision_quits_on_q() {
        assert_eq!(classify_prompt_decision(2, "q\n"), PromptDecision::Quit);
        assert_eq!(classify_prompt_decision(2, "Q\n"), PromptDecision::Quit);
    }

    #[test]
    fn classify_prompt_decision_restarts_on_blank_line() {
        assert_eq!(
            classify_prompt_decision(1, "\n"),
            PromptDecision::RestartFresh
        );
    }

    #[test]
    fn classify_prompt_decision_quits_on_eof() {
        assert_eq!(classify_prompt_decision(0, ""), PromptDecision::QuitEof);
    }

    #[test]
    fn classify_prompt_decision_rejects_unrecognized_input() {
        assert_eq!(
            classify_prompt_decision(4, "yes\n"),
            PromptDecision::Invalid
        );
    }

    #[cfg(unix)]
    #[test]
    fn prompt_termios_forces_canonical_enter_friendly_prompt_mode() {
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        original.c_iflag = libc::IGNCR;
        original.c_oflag = 0;
        original.c_lflag = 0;
        original.c_cflag = 0x1234;
        original.c_cc[libc::VMIN] = 0;
        original.c_cc[libc::VTIME] = 9;

        let prompt = prompt_termios_from_original(&original);

        assert_ne!(prompt.c_iflag & libc::ICRNL, 0);
        assert_eq!(prompt.c_iflag & libc::IGNCR, 0);
        assert_eq!(prompt.c_iflag & libc::INLCR, 0);
        assert_ne!(prompt.c_oflag & libc::OPOST, 0);
        assert_ne!(prompt.c_oflag & libc::ONLCR, 0);
        assert_ne!(prompt.c_lflag & libc::ICANON, 0);
        assert_ne!(prompt.c_lflag & libc::ECHO, 0);
        assert_ne!(prompt.c_lflag & libc::ISIG, 0);
        assert_ne!(prompt.c_lflag & libc::IEXTEN, 0);
        assert_eq!(prompt.c_cflag, original.c_cflag);
        assert_eq!(prompt.c_cc[libc::VMIN], 1);
        assert_eq!(prompt.c_cc[libc::VTIME], 0);
    }

    #[test]
    fn prompt_input_summary_escapes_and_truncates() {
        assert_eq!(prompt_input_summary("\n"), "<empty>");
        assert_eq!(prompt_input_summary("abc\tdef\n"), "abc\\tdef");
        assert_eq!(
            prompt_input_summary("abcdefghijklmnopqrstuvwxyz1234567890\n"),
            "abcdefghijklmnopqrstuvwxyz123456..."
        );
    }

    #[test]
    fn auto_trigger_monitor_cancels_before_timeout() {
        let monitor = AutoTriggerMonitor::new(Instant::now(), Duration::from_secs(30));
        assert_eq!(monitor.stop_outcome(), AutoTriggerOutcome::Cancelled);
    }

    #[test]
    fn auto_trigger_monitor_preserves_timeout_after_deadline() {
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));
        assert!(monitor.note_no_prompt(start + Duration::from_millis(5)));
        assert!(!monitor.note_no_prompt(start + Duration::from_millis(10)));
        assert_eq!(monitor.stop_outcome(), AutoTriggerOutcome::Timeout);
    }

    #[test]
    fn auto_trigger_thread_cancels_cleanly_before_prompt_poll() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared
            .auto_trigger_outcome
            .store(AutoTriggerOutcome::Pending as u8, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(true));
        let handle = spawn_auto_trigger_thread(
            shared.clone(),
            stop,
            "tasks/software/tsift.md".to_string(),
            crate::harness::HarnessConfig::codex(),
            None,
        );
        handle.join().unwrap();
        assert_eq!(
            AutoTriggerOutcome::from_u8(shared.auto_trigger_outcome.load(Ordering::Relaxed)),
            AutoTriggerOutcome::Cancelled
        );
    }

    #[derive(Clone)]
    struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "writer closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn auto_trigger_inject_command_writes_carriage_return() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let stop = AtomicBool::new(false);

        assert_eq!(
            auto_trigger_inject_command(&shared, &stop, "agent-doc tasks/software/tsift.md"),
            AutoTriggerOutcome::Sent
        );
        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn normalize_supervisor_inject_bytes_converts_line_feeds_to_carriage_returns() {
        assert_eq!(
            normalize_supervisor_inject_bytes("agent-doc tasks/software/tsift.md\n"),
            b"agent-doc tasks/software/tsift.md\r"
        );
        assert_eq!(
            normalize_supervisor_inject_bytes("line one\r\nline two\nline three\r"),
            b"line one\rline two\rline three\r"
        );
    }

    #[test]
    fn handle_ipc_inject_normalizes_submit_newline_before_writing() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));

        let response = handle_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            &shared,
        );

        assert!(response.ok);
        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn handle_ipc_inject_rejects_pending_capability_proof() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
        let response = handle_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            &shared,
        );

        assert!(!response.ok);
        assert!(
            response
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("capability proof is still pending"),
            "{response:?}"
        );
    }

    #[test]
    fn auto_trigger_inject_command_rejects_failed_capability_proof() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let stop = AtomicBool::new(false);

        assert_eq!(
            auto_trigger_inject_command(&shared, &stop, "agent-doc tasks/software/tsift.md"),
            AutoTriggerOutcome::SendFailed
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn dispatch_submit_text_to_tmux_uses_pane_submit_path() {
        let tmp = TempDir::new().unwrap();
        let iso = IsolatedTmux::new("start-ipc-submit-path");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("submit.txt");
        let done_path = tmp.path().join("done.txt");

        std::thread::sleep(Duration::from_millis(150));
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));

        dispatch_submit_text_to_tmux(&iso, &pane, "agent-doc tasks/software/tsift.md\n").unwrap();
        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(done_path.exists(), "expected submitted command to complete");
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "agent-doc tasks/software/tsift.md"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn supervisor_ipc_inject_reaches_live_tmux_pane_submit_path() {
        let _env_guard = crate::test_support::env_lock();
        let tmp = TempDir::new().unwrap();
        let iso = IsolatedTmux::new("start-ipc-live-supervisor-submit");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("submit.txt");
        let done_path = tmp.path().join("done.txt");

        std::thread::sleep(Duration::from_millis(150));
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));

        let _tmux_env = ScopedEnvVar::set("TMUX", tmux_env_for_server(&iso));
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            None,
            Some(crate::session_actor::ActorState::Ready),
            Some(pane.clone()),
        ));
        let shared_for_ipc = shared.clone();
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), "test-session", {
            move |method| handle_ipc(method, &shared_for_ipc)
        })
        .unwrap();

        let response = crate::supervisor::ipc::send_command(
            ipc.path(),
            &IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md".to_string(),
            },
        )
        .expect("supervisor IPC inject should succeed");
        assert!(response.ok, "supervisor IPC inject should return ok");

        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            done_path.exists(),
            "expected supervisor IPC inject to submit through the live tmux pane"
        );
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "agent-doc tasks/software/tsift.md"
        );

        ipc.stop();
    }

    #[test]
    fn auto_trigger_inject_command_honors_late_cancel_before_write() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let stop = AtomicBool::new(true);

        assert_eq!(
            auto_trigger_inject_command(&shared, &stop, "agent-doc tasks/software/tsift.md"),
            AutoTriggerOutcome::Cancelled
        );
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn auto_trigger_inject_command_cancels_while_waiting_for_busy_writer_lock() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(RecordingWriter(
            written.clone(),
        )))));
        let held = writer.lock().unwrap();
        *shared.inject_writer.lock().unwrap() = Some(writer.clone());

        let stop = Arc::new(AtomicBool::new(false));
        let shared_for_thread = shared.clone();
        let stop_for_thread = stop.clone();
        let handle = std::thread::spawn(move || {
            auto_trigger_inject_command(
                &shared_for_thread,
                stop_for_thread.as_ref(),
                "agent-doc tasks/software/tsift.md",
            )
        });

        std::thread::sleep(Duration::from_millis(50));
        stop.store(true, Ordering::Relaxed);
        drop(held);

        assert_eq!(handle.join().unwrap(), AutoTriggerOutcome::Cancelled);
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn auto_trigger_inject_command_reports_closed_writer_during_trigger_window() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(FailingWriter),
        ))));
        let stop = AtomicBool::new(false);

        assert_eq!(
            auto_trigger_inject_command(&shared, &stop, "agent-doc tasks/software/tsift.md"),
            AutoTriggerOutcome::SendFailed
        );
    }

    #[test]
    fn current_child_prompt_visible_uses_latest_nonempty_line() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, b"old output\n");
        record_recent_output(&shared, "❯\n".as_bytes());
        record_recent_output(&shared, b"resumed child still printing\n");
        assert!(
            !current_child_prompt_visible(&shared, &harness),
            "an earlier prompt in the current child transcript should not count once newer non-prompt output follows it"
        );
    }

    #[test]
    fn current_child_prompt_visible_accepts_prompt_from_current_child_output() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, b"resumed child ready\n");
        record_recent_output(&shared, "❯\n".as_bytes());
        assert!(current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn current_child_prompt_visible_handles_suffix_prompt_line() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, "/tmp/project ❯\n".as_bytes());
        assert!(current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn current_child_prompt_visible_skips_codex_footer_line() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n".as_bytes(),
        );
        assert!(current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn current_child_prompt_visible_rejects_busy_output_above_codex_footer() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"resumed child still printing\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );
        assert!(!current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn prompt_visible_requires_ready_transition_on_first_prompt() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        assert!(prompt_visible_requires_ready_transition(&shared));
        assert!(
            !prompt_visible_requires_ready_transition(&shared),
            "a repeated prompt without an intervening busy transition should not retrigger ready"
        );
    }

    #[test]
    fn prompt_visible_requires_ready_transition_after_busy_dispatch() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        shared.prompt_visible_once.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Busy);
        assert!(
            prompt_visible_requires_ready_transition(&shared),
            "a busy actor that surfaces the prompt again must return to ready"
        );
    }

    // --- StopSignal + writer thread tests ---

    #[cfg(unix)]
    #[test]
    fn stop_signal_wakes_poll() {
        // StopSignal should create a valid pipe and signal() should not panic
        let stop = StopSignal::new().unwrap();
        stop.signal();
        // Verify the read end is readable after signal
        let mut fds = [libc::pollfd {
            fd: stop.read_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 100) };
        assert_eq!(ret, 1, "poll should return 1 after signal");
        assert_ne!(fds[0].revents & libc::POLLIN, 0, "POLLIN should be set");
    }

    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_stop_signal() {
        // Create a pipe to act as the "pty writer" — we just need something
        // that accepts writes without blocking
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        let pty_write_fd = pty_fds[1];

        // Wrap the write end in a Box<dyn Write + Send> for spawn_writer_thread
        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_write_fd));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SupervisorShared::new("test", "writer-stop".to_string()));
        let handle = spawn_writer_thread(
            shared,
            writer_arc,
            stop.read_fd(),
            stop_flag.clone(),
            None,
            None,
        );

        // Writer thread should be alive, blocked in poll()
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Signal stop — thread should exit promptly
        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();
        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit cleanly on stop signal"
        );

        // Clean up pipe fds
        unsafe {
            libc::close(pty_fds[0]);
            libc::close(pty_fds[1]);
        }
    }

    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_pty_write_failure() {
        // Create a pipe as the "pty writer", then close the read end so
        // writes fail with EPIPE — simulating Claude exit closing the PTY
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        // Close read end immediately so writes produce EPIPE
        unsafe { libc::close(pty_fds[0]) };

        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_fds[1]));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_fd = stop.read_fd();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SupervisorShared::new("test", "writer-epipe".to_string()));
        let handle =
            spawn_writer_thread(shared, writer_arc, stop_fd, stop_flag.clone(), None, None);

        // Inject a byte into stdin to trigger a write attempt.
        // The write will fail (EPIPE) and the thread should exit.
        // We use the stop signal as a fallback timeout.
        std::thread::sleep(std::time::Duration::from_millis(50));
        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();

        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit on write failure or stop"
        );

        unsafe { libc::close(pty_fds[1]) };
    }

    #[cfg(unix)]
    #[test]
    fn reader_thread_exits_on_eof() {
        // Create a pipe as mock pty reader. Closing the write end
        // should cause the reader thread to see EOF and exit.
        let mut fds = [0i32; 2];
        unsafe { libc::pipe(fds.as_mut_ptr()) };

        struct FdReader(i32);
        impl std::io::Read for FdReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::read(self.0, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
        }

        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let reader: Box<dyn std::io::Read + Send> = Box::new(FdReader(fds[0]));
        let handle = spawn_reader_thread(shared, crate::harness::HarnessConfig::codex(), reader);

        // Close the write end → reader sees EOF → thread exits
        unsafe { libc::close(fds[1]) };

        let result = handle.join();
        assert!(result.is_ok(), "reader thread should exit cleanly on EOF");

        unsafe { libc::close(fds[0]) };
    }

    #[cfg(unix)]
    #[test]
    fn tcflush_discards_pending_input() {
        // Verify that tcflush(TCIFLUSH) discards buffered input.
        // This test uses a socketpair to avoid interfering with the
        // real stdin — it confirms the libc call doesn't panic.
        // (A full stdin test would require pty allocation.)
        unsafe {
            // Just verify the call doesn't error on STDIN_FILENO
            // (it may return -1 if stdin isn't a tty, which is fine in CI)
            let ret = libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
            // In CI / non-tty contexts, ret may be -1 (ENOTTY). That's OK —
            // the code uses tcflush as best-effort cleanup.
            let _ = ret;
        }
    }

    #[test]
    fn exit_provenance_fields_capture_signal_termination() {
        let status = portable_pty::ExitStatus::with_signal("Hangup");
        let rendered = exit_provenance_fields(&status);
        assert!(rendered.contains("exit_kind=signal"), "got: {rendered}");
        assert!(
            rendered.contains("exit_signal=\"Hangup\""),
            "got: {rendered}"
        );
        assert!(
            rendered.contains("exit_status=\"Terminated by Hangup\""),
            "got: {rendered}"
        );
    }

    #[test]
    fn exit_provenance_fields_capture_nonzero_exit_code() {
        let status = portable_pty::ExitStatus::with_exit_code(7);
        let rendered = exit_provenance_fields(&status);
        assert!(rendered.contains("exit_kind=exit_code"), "got: {rendered}");
        assert!(
            rendered.contains("exit_status=\"Exited with code 7\""),
            "got: {rendered}"
        );
    }

    #[test]
    fn forwarded_ctrl_c_interrupt_exit_requires_forwarded_ctrl_c_signal_exit() {
        let interrupt = portable_pty::ExitStatus::with_signal("Interrupt");
        assert!(is_forwarded_ctrl_c_interrupt_exit(&interrupt, true));
        assert!(!is_forwarded_ctrl_c_interrupt_exit(&interrupt, false));

        let clean = portable_pty::ExitStatus::with_exit_code(0);
        assert!(!is_forwarded_ctrl_c_interrupt_exit(&clean, true));
    }

    #[test]
    fn forwarded_ctrl_c_interrupt_exit_accepts_exit_code_130() {
        let status = portable_pty::ExitStatus::with_exit_code(130);
        assert!(is_forwarded_ctrl_c_interrupt_exit(&status, true));
    }
}
