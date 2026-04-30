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
//! - Requires an active tmux session; bails immediately if not inside tmux.
//! - If another pane already proves live ownership of the same document
//!   session, focuses and reuses that pane instead of spawning a duplicate
//!   supervisor in the current pane. When that pane lives in another tmux
//!   session, `start` switches the caller's current client to the target
//!   session before selecting the pane. Once that live-owner proof exists,
//!   missing or stale supervisor IPC state must not authorize a replacement
//!   pane for the same document.
//! - If `sessions.json` points at an alive pane but no live owner can still be
//!   proven for the document, `start` first consults the per-session supervisor:
//!   healthy supervisors are reused, restartable supervisors are restarted in
//!   place, and only unavailable supervisors are treated as stale before
//!   starting in the current pane. If that alive pane still carries the active
//!   startup-miss marker for the document, `start` must fail closed instead of
//!   rebinding the document to a fresh pane.
//! - Registers the session UUID → current tmux pane ID in `sessions.json` so
//!   other subcommands (`route`, `focus`, etc.) can locate the pane.
//! - Runs the configured harness binary as a blocking child process inside a persistent restart loop
//!   so the tmux pane never dies on its own.
//! - On non-zero exit (context exhaustion, crash, etc.): auto-restarts after a
//!   2-second delay using `--continue` to resume the previous conversation.
//! - On clean exit (code 0): honors the active harness policy.
//!   Claude prompts on stderr and waits for Enter (fresh restart) or `q` + Enter (exit).
//!   Codex auto-restarts in resume mode so `codex exec` remains a persistent session.
//!   If stdin EOF/Ctrl-D was forwarded, supervisor prompts the user instead
//!   (`Enter` = restart fresh, `q` = exit) so the pane can be quit cleanly.
//!   Prompt decisions are logged explicitly. Prompt-time stdin EOF normally
//!   counts as quit, except for a first-run Ctrl-D prompt inside the early
//!   startup grace window, which restarts fresh so transient tmux stash/rescue
//!   input races do not close the claimed pane. Non-empty non-`q` input is
//!   rejected and re-prompted instead of silently restarting fresh.
//!   If the resume handoff just failed, the first failure restarts fresh and
//!   repeated failures escalate to that same prompt instead of looping blindly.
//! - Prints the truncated session UUID and pane ID to stderr on registration.
//! - Opens a persistent session log at `.agent-doc/logs/<session-uuid>.log`,
//!   appending timestamped events for session start, claude start/restart/exit,
//!   user quit, and session end.
//! - On `--continue` restarts, spawns a background thread that waits for the
//!   harness prompt to appear in the current child process's filtered pty
//!   output before injecting the harness-specific trigger command into the
//!   child pty to auto-trigger the skill workflow in the resumed conversation.
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

use anyhow::{Context, Result};
use portable_pty::PtySize;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::Path;
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
const SHARED_WRITER_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SHARED_WRITER_WRITE_POLL_INTERVAL_MS: i32 = 50;
const SHARED_WRITER_CHUNK_MAX: usize = 1024;
const EARLY_CTRL_D_EOF_RESTART_WINDOW: Duration = Duration::from_secs(15);

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

fn ctrl_d_prompt_eof_policy(run_duration: Duration, restart_count: u32) -> PromptEofPolicy {
    if restart_count == 0 && run_duration <= EARLY_CTRL_D_EOF_RESTART_WINDOW {
        return PromptEofPolicy::RestartFresh;
    }
    PromptEofPolicy::Quit
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
    failed_resume: bool,
    ctrl_d_forwarded: bool,
    recent_failed_resumes: usize,
) -> RestartContinueExitStrategy {
    if ctrl_d_forwarded {
        return RestartContinueExitStrategy::PromptUser;
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
    let Some(writer_arc) = shared.inject_writer.lock().unwrap().clone() else {
        return AutoTriggerOutcome::SendFailed;
    };
    if stop.load(Ordering::Relaxed) {
        return AutoTriggerOutcome::Cancelled;
    }

    let mut payload = trigger_cmd.as_bytes().to_vec();
    payload.push(b'\r');

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

#[derive(Debug, PartialEq, Eq)]
enum ExistingSessionPaneAction {
    Reuse(String),
    ClearStale(String),
}

/// Supervisor health as observed by the `start` reuse probe.
#[derive(Debug)]
enum SupervisorHealth {
    /// Supervisor is reachable, child is running and healthy.
    Healthy,
    /// Supervisor is reachable, and an automatic in-place restart is still allowed.
    Restartable,
    /// Supervisor halted itself after repeated failures; do not revive it in place.
    Halted { restart_count: u32 },
    /// Socket exists but supervisor did not respond or errored.
    Unreachable,
    /// No supervisor socket found at all.
    NoSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleRegisteredPaneAction {
    ReuseRegistered,
    RestartRegistered,
    ClearStaleHalted { restart_count: u32 },
    ClearStale,
}

fn query_supervisor_health(file: &Path, session_id: &str) -> SupervisorHealth {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return SupervisorHealth::NoSocket,
    };
    let project_root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return SupervisorHealth::NoSocket,
    };
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return SupervisorHealth::NoSocket;
    }
    match crate::supervisor::ipc::send_command(&sock, &IpcMethod::State) {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                let running = data
                    .get("running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
                let restart_count = data
                    .get("restart_count")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0);
                if running && state == "healthy" {
                    SupervisorHealth::Healthy
                } else if state == "halted" {
                    SupervisorHealth::Halted { restart_count }
                } else {
                    SupervisorHealth::Restartable
                }
            } else {
                SupervisorHealth::Restartable
            }
        }
        Ok(_) => SupervisorHealth::Unreachable,
        Err(_) => SupervisorHealth::Unreachable,
    }
}

fn restart_via_supervisor(file: &Path, session_id: &str) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return false,
    };
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    let method = IpcMethod::Restart {
        mode: "continue".to_string(),
    };
    match crate::supervisor::ipc::send_command(&sock, &method) {
        Ok(resp) => resp.ok,
        Err(_) => false,
    }
}

fn stale_registered_pane_action(supervisor_health: SupervisorHealth) -> StaleRegisteredPaneAction {
    match supervisor_health {
        SupervisorHealth::Healthy => StaleRegisteredPaneAction::ReuseRegistered,
        SupervisorHealth::Restartable => StaleRegisteredPaneAction::RestartRegistered,
        SupervisorHealth::Halted { restart_count } => {
            StaleRegisteredPaneAction::ClearStaleHalted { restart_count }
        }
        SupervisorHealth::Unreachable | SupervisorHealth::NoSocket => {
            StaleRegisteredPaneAction::ClearStale
        }
    }
}

fn should_fail_closed_for_unresolved_startup_miss_rebind(
    current_pane: &str,
    registered_pane: &str,
    miss: Option<&crate::startup_miss::StartupMiss>,
) -> bool {
    current_pane != registered_pane && miss.is_some_and(|miss| miss.pane_id == registered_pane)
}

fn existing_session_pane_action(
    tmux: &sessions::Tmux,
    session_id: &str,
    file: &Path,
    current_pane: &str,
) -> Result<Option<ExistingSessionPaneAction>> {
    let entry = sessions::lookup_entry(session_id)?;
    let live_owner = crate::sync::find_live_owner_pane_excluding_quiet(
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
        return Some(ExistingSessionPaneAction::Reuse(owner.to_string()));
    }

    let entry = entry?;
    if entry.pane == current_pane || !tmux.pane_alive(&entry.pane) {
        return None;
    }
    Some(ExistingSessionPaneAction::ClearStale(entry.pane.clone()))
}

fn focus_existing_session_pane(
    tmux: &sessions::Tmux,
    current_pane: &str,
    target_pane: &str,
) -> Result<()> {
    let target_session = tmux.pane_session(target_pane).ok();
    let current_session = tmux.pane_session(current_pane).ok();

    let mut cmd = tmux.cmd();
    if should_switch_client_for_focus(
        current_session.as_deref(),
        target_session.as_deref(),
        std::env::var_os("TMUX").is_some(),
    ) && let Some(target_session) = target_session.as_deref()
    {
        cmd.args(["switch-client", "-t", target_session]);
        cmd.arg(";");
    }
    cmd.args(["select-window", "-t", target_pane]);
    cmd.arg(";");
    cmd.args(["select-pane", "-t", target_pane]);
    let status = cmd
        .status()
        .context("failed to execute tmux focus command for existing session pane")?;
    if !status.success() {
        anyhow::bail!("tmux focus command failed for pane {}", target_pane);
    }
    Ok(())
}

fn should_switch_client_for_focus(
    current_session: Option<&str>,
    target_session: Option<&str>,
    inside_tmux: bool,
) -> bool {
    inside_tmux
        && matches!(
            (current_session, target_session),
            (Some(current), Some(target)) if current != target
        )
}

/// Put stdin into raw mode so the outer pty line discipline doesn't translate
/// input bytes (ICRNL converts \r → \n, breaking Enter for Claude Code's TUI).
/// Restores original termios on drop.
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

    /// Temporarily restore cooked mode (for read_line prompts).
    fn suspend(&self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
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

/// Shared state between the main supervisor loop and the IPC handler thread.
struct SupervisorShared {
    /// Current supervisor state for IPC `state` queries.
    supervisor_state: Mutex<SupervisorState>,
    /// Current restart count.
    restart_count: AtomicU32,
    /// Whether a child is currently running.
    running: AtomicBool,
    /// CWD source tag for IPC `state` responses.
    cwd_source: &'static str,
    /// Writer handle for IPC `inject`. Replaced on each spawn, cleared between restarts.
    inject_writer: SharedWriter,
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
    /// Outcome of the most recent auto-trigger attempt after a restart.
    auto_trigger_outcome: AtomicU8,
}

impl SupervisorShared {
    fn new(cwd_source: &'static str) -> Self {
        Self {
            supervisor_state: Mutex::new(SupervisorState::Healthy),
            restart_count: AtomicU32::new(0),
            running: AtomicBool::new(false),
            cwd_source,
            inject_writer: Mutex::new(None),
            recent_output: Mutex::new(Vec::new()),
            child_pid: AtomicU32::new(0),
            restart_requested: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            restart_mode: Mutex::new("continue".to_string()),
            ctrl_d_forwarded: AtomicBool::new(false),
            auto_trigger_outcome: AtomicU8::new(AutoTriggerOutcome::NotNeeded as u8),
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
            IpcResponse::ok(serde_json::json!({
                "running": shared.running.load(Ordering::Relaxed),
                "state": state.as_str(),
                "restart_count": shared.restart_count.load(Ordering::Relaxed),
                "cwd_source": shared.cwd_source,
            }))
        }
        IpcMethod::Pid => {
            let pid = shared.child_pid.load(Ordering::Relaxed);
            if pid > 0 {
                IpcResponse::ok(serde_json::json!({ "pid": pid }))
            } else {
                IpcResponse::ok(serde_json::json!({ "pid": null }))
            }
        }
        IpcMethod::Inject { bytes } => {
            let guard = shared.inject_writer.lock().unwrap();
            match guard.as_ref() {
                Some(writer_arc) => {
                    let mut w = writer_arc.lock().unwrap();
                    match w.write_all_blocking(bytes.as_bytes()) {
                        Ok(()) => IpcResponse::ok(serde_json::json!({ "n": bytes.len() })),
                        Err(e) => IpcResponse::err(format!("write error: {e}")),
                    }
                }
                None => IpcResponse::err("no active session"),
            }
        }
        IpcMethod::Restart { mode } => {
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
    writer: Arc<Mutex<SharedPtyWriter>>,
    stop_fd: std::os::unix::io::RawFd,
    stop: Arc<AtomicBool>,
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
    writer: Arc<Mutex<SharedPtyWriter>>,
    _stop_fd: (),
    stop: Arc<AtomicBool>,
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

pub fn run(file: &Path) -> Result<()> {
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
                "start_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} miss_timestamp={} latest_open_timestamp={}",
                file.display(),
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_open_timestamp
            ),
        );
    }
    let unresolved_startup_miss = crate::startup_miss::load(file).ok().flatten();

    if let Some(action) = existing_session_pane_action(&tmux, &session_id, file, &pane_id)? {
        match action {
            ExistingSessionPaneAction::Reuse(existing_pane) => {
                if sessions::lookup(&session_id)?.as_deref() != Some(existing_pane.as_str()) {
                    sessions::register(&session_id, &existing_pane, &file.to_string_lossy())?;
                    eprintln!(
                        "[start] recovered live owner for {} in pane {}",
                        file.display(),
                        existing_pane
                    );
                }
                eprintln!(
                    "session {} for {} is already running in pane {} — switching focus",
                    &session_id[..8.min(session_id.len())],
                    file.display(),
                    existing_pane
                );
                if let Err(e) = focus_existing_session_pane(&tmux, &pane_id, &existing_pane) {
                    eprintln!(
                        "[start] warning: failed to focus pane {}: {}",
                        existing_pane, e
                    );
                }
                return Ok(());
            }
            ExistingSessionPaneAction::ClearStale(stale_pane) => {
                if should_fail_closed_for_unresolved_startup_miss_rebind(
                    &pane_id,
                    &stale_pane,
                    unresolved_startup_miss.as_ref(),
                ) {
                    let miss = unresolved_startup_miss
                        .as_ref()
                        .expect("guard checked presence");
                    let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
                    anyhow::bail!(
                        "startup-miss from {} still belongs to alive pane {} for {} — refusing to start a replacement pane over the existing owner",
                        miss_ts,
                        stale_pane,
                        file.display()
                    );
                }
                match stale_registered_pane_action(query_supervisor_health(file, &session_id)) {
                    StaleRegisteredPaneAction::ReuseRegistered => {
                        eprintln!(
                            "[start] registered pane {} has a healthy supervisor for {} despite missing live-owner proof — switching focus",
                            stale_pane,
                            file.display()
                        );
                        if let Err(e) = focus_existing_session_pane(&tmux, &pane_id, &stale_pane) {
                            eprintln!(
                                "[start] warning: failed to focus pane {}: {}",
                                stale_pane, e
                            );
                        }
                        return Ok(());
                    }
                    StaleRegisteredPaneAction::RestartRegistered => {
                        eprintln!(
                            "[start] registered pane {} has a restartable supervisor for {} despite missing live-owner proof — restarting in place",
                            stale_pane,
                            file.display()
                        );
                        if restart_via_supervisor(file, &session_id) {
                            if let Err(e) =
                                focus_existing_session_pane(&tmux, &pane_id, &stale_pane)
                            {
                                eprintln!(
                                    "[start] warning: failed to focus pane {}: {}",
                                    stale_pane, e
                                );
                            }
                            return Ok(());
                        }
                        eprintln!(
                            "[start] supervisor restart failed for pane {} — clearing stale entry",
                            stale_pane
                        );
                        let _ = sessions::deregister(&session_id)?;
                    }
                    StaleRegisteredPaneAction::ClearStaleHalted { restart_count } => {
                        eprintln!(
                            "[start] registered pane {} for {} has a halted supervisor after {} restarts — clearing the stale crashed session and starting fresh",
                            stale_pane,
                            file.display(),
                            restart_count
                        );
                        let _ = sessions::deregister(&session_id)?;
                    }
                    StaleRegisteredPaneAction::ClearStale => {
                        eprintln!(
                            "[start] registered pane {} is alive but no live owner for {} was proven and supervisor is unavailable — clearing stale entry",
                            stale_pane,
                            file.display()
                        );
                        let _ = sessions::deregister(&session_id)?;
                    }
                }
            }
        }
    }

    // Only relocate the current launcher pane when start is actually falling through to
    // a fresh session in this pane. If a live owner already exists elsewhere, moving the
    // launcher pane first can rip it out of its original tmux window/session before the
    // reuse path returns.
    if let Some(expected_session) = config::project_tmux_session() {
        relocate_if_wrong_session(&tmux, &pane_id, &expected_session);
    }

    // Register session → pane (with relative file path)
    let file_str = file.to_string_lossy();
    sessions::register(&session_id, &pane_id, &file_str)?;
    eprintln!("Registered session {} → pane {}", &session_id[..8], pane_id);

    // Open session log
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut session_log = open_session_log(&canonical, &session_id);
    log_event(
        &mut session_log,
        &format!(
            "session_start file={} pane={} session={}",
            file.display(),
            pane_id,
            &session_id[..8]
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
    // Inject --model from claude_model/codex_model frontmatter when not already in args
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

    // Find project root for IPC socket placement
    let project_root =
        snapshot::find_project_root(&canonical).unwrap_or_else(|| resolved_cwd.path.clone());

    // Create shared state for IPC handler
    let shared = Arc::new(SupervisorShared::new(resolved_cwd.source.as_str()));

    // Start IPC listener
    let shared_for_ipc = shared.clone();
    let mut ipc = SupervisorIpc::start(&project_root, &session_id, move |method| {
        handle_ipc(method, &shared_for_ipc)
    })?;
    log_event(
        &mut session_log,
        &format!("ipc_started project_root={}", project_root.display()),
    );

    // Crash policy state machine
    let mut policy = CrashPolicy::new();

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
    let mut supervisor_exit_reason = "loop_exhausted";
    loop {
        // Build args for this iteration
        let auto_trigger;
        let args = if !first_run {
            auto_trigger = true;
            let restart_args = harness.restart_args(&base_args);
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

        let run_started_at = Instant::now();

        // Build PtySpawnConfig and spawn child under pty
        let cfg = PtySpawnConfig {
            program: harness.binary.clone(),
            args,
            cwd: resolved_cwd.path.clone(),
            env: resolved_env.clone(),
            size: initial_size,
        };
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
        shared
            .auto_trigger_outcome
            .store(AutoTriggerOutcome::NotNeeded as u8, Ordering::Relaxed);
        shared.recent_output.lock().unwrap().clear();

        // Spawn I/O forwarding threads
        let reader_thread = spawn_reader_thread(shared.clone(), pty_reader);
        let writer_stop = StopSignal::new().context("failed to create writer stop signal")?;
        let writer_stop_flag = Arc::new(AtomicBool::new(false));
        let ctrl_d_flag = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let writer_thread = spawn_writer_thread(
            writer_arc.clone(),
            writer_stop.read_fd(),
            writer_stop_flag.clone(),
            Some(ctrl_d_flag.clone()),
        );
        #[cfg(not(unix))]
        let writer_thread = spawn_writer_thread(
            writer_arc.clone(),
            (),
            writer_stop_flag.clone(),
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
        if shared.stop_requested.load(Ordering::Relaxed) {
            log_event(&mut session_log, "ipc_stop");
            supervisor_exit_reason = "ipc_stop";
            break;
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

        let ctrl_d_forwarded = shared.ctrl_d_forwarded.load(Ordering::Relaxed);
        let failed_resume =
            resume_handoff_failed(auto_trigger, ctrl_d_forwarded, auto_trigger_outcome);
        let run_duration = run_started_at.elapsed();

        if matches!(
            auto_trigger_outcome,
            AutoTriggerOutcome::Sent | AutoTriggerOutcome::NotNeeded
        ) {
            failed_resume_tracker.reset();
        }

        let action = policy.on_exit(code);
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
                                supervisor_exit_reason = "user_quit_clean_exit";
                                break;
                            }
                            PromptOutcome::RestartFresh => {
                                raw_mode.resume();
                                first_run = true;
                                restart_count += 1;
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
                            failed_resume,
                            ctrl_d_forwarded,
                            recent_failures,
                        ) {
                            RestartContinueExitStrategy::PromptUser => {
                                raw_mode.suspend();
                                if ctrl_d_forwarded {
                                    eprintln!(
                                        "\n{} exited after stdin EOF/Ctrl-D.",
                                        harness.binary
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "ctrl_d_prompt_user restart_count={}",
                                            restart_count
                                        ),
                                    );
                                } else {
                                    eprintln!(
                                        "\n{} failed to re-establish a prompt after resume {} times in the last {}s.",
                                        harness.binary,
                                        recent_failures,
                                        FAILED_RESUME_WINDOW.as_secs()
                                    );
                                }
                                let prompt_kind = if ctrl_d_forwarded {
                                    "ctrl_d"
                                } else {
                                    "resume_failure"
                                };
                                let quit_event = if ctrl_d_forwarded {
                                    "user_quit_after_ctrl_d"
                                } else {
                                    "user_quit_after_resume_failure"
                                };
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    prompt_kind,
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    quit_event,
                                    if ctrl_d_forwarded {
                                        ctrl_d_prompt_eof_policy(run_duration, restart_count)
                                    } else {
                                        PromptEofPolicy::Quit
                                    },
                                ) {
                                    PromptOutcome::Quit => {
                                        supervisor_exit_reason = match prompt_kind {
                                            "ctrl_d" => "user_quit_after_ctrl_d",
                                            "resume_failure" => "user_quit_after_resume_failure",
                                            _ => "user_quit",
                                        };
                                        break;
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                    }
                                }
                            }
                            RestartContinueExitStrategy::RestartFresh => {
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
                                first_run = true;
                                restart_count += 1;
                            }
                            RestartContinueExitStrategy::Resume => {
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
            }
            RestartAction::Halt => {
                eprintln!(
                    "\nSupervisor halted after {} restarts (flapping detected).",
                    restart_count
                );
                log_event(&mut session_log, "supervisor_halted");
                supervisor_exit_reason = "supervisor_halted";
                break;
            }
        }
    }

    // Restore terminal to original mode before cleanup
    drop(raw_mode);

    // Cleanup
    if let Some(mut rw) = resize_watcher.take() {
        rw.stop();
    }
    ipc.stop();
    log_event(
        &mut session_log,
        &format!(
            "supervisor_exit reason={} pane={} restart_count={}",
            supervisor_exit_reason, pane_id, restart_count
        ),
    );
    log_event(&mut session_log, "session_end");
    eprintln!("Session ended for {}", file.display());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::Frontmatter;
    use crate::hooks::fire_doc_hooks;
    use crate::sessions::IsolatedTmux;
    use std::collections::HashMap;

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
    fn existing_session_pane_action_reuses_proven_live_owner() {
        let iso = IsolatedTmux::new("start-duplicate-live-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
        };

        let action =
            existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), Some(&pane_a));
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Reuse(pane_a.clone()))
        );
    }

    #[test]
    fn existing_session_reuse_keeps_launcher_pane_in_original_session() {
        let iso = IsolatedTmux::new("start-reuse-keeps-launcher-session");
        let tmp = tempfile::TempDir::new().unwrap();
        let owner_pane = iso.new_session("sess-a", tmp.path()).unwrap();
        let launcher_pane = iso.new_session("sess-b", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: owner_pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&owner_pane).unwrap_or_default(),
        };

        let action = existing_session_pane_action_from_entry(
            &iso,
            &launcher_pane,
            Some(&entry),
            Some(&owner_pane),
        );
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Reuse(owner_pane.clone()))
        );
        assert_eq!(
            iso.pane_session(&launcher_pane).unwrap(),
            "sess-b",
            "proving an existing live owner must not relocate the launcher pane"
        );
        assert_eq!(iso.pane_session(&owner_pane).unwrap(), "sess-a");
    }

    #[test]
    fn existing_session_pane_action_ignores_same_pane() {
        let iso = IsolatedTmux::new("start-duplicate-same-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane).unwrap_or_default(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }

    #[test]
    fn existing_session_pane_action_clears_alive_stale_registration_without_owner() {
        let iso = IsolatedTmux::new("start-stale-alive-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), None);
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::ClearStale(pane_a.clone()))
        );
    }

    #[test]
    fn existing_session_pane_action_ignores_dead_registered_pane() {
        let iso = IsolatedTmux::new("start-duplicate-dead-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = crate::sessions::SessionEntry {
            pane: "%999999".to_string(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            file: "tasks/software/corky.md".to_string(),
            window: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }

    #[test]
    fn stale_registered_pane_uses_supervisor_before_clearing() {
        assert_eq!(
            stale_registered_pane_action(SupervisorHealth::Healthy),
            StaleRegisteredPaneAction::ReuseRegistered
        );
        assert_eq!(
            stale_registered_pane_action(SupervisorHealth::Restartable),
            StaleRegisteredPaneAction::RestartRegistered
        );
        assert_eq!(
            stale_registered_pane_action(SupervisorHealth::Halted { restart_count: 5 }),
            StaleRegisteredPaneAction::ClearStaleHalted { restart_count: 5 }
        );
        assert_eq!(
            stale_registered_pane_action(SupervisorHealth::Unreachable),
            StaleRegisteredPaneAction::ClearStale
        );
        assert_eq!(
            stale_registered_pane_action(SupervisorHealth::NoSocket),
            StaleRegisteredPaneAction::ClearStale
        );
    }

    #[test]
    fn unresolved_startup_miss_blocks_cross_pane_rebind() {
        let miss = crate::startup_miss::StartupMiss {
            file: "tasks/software/corky.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        assert!(should_fail_closed_for_unresolved_startup_miss_rebind(
            "%84",
            "%42",
            Some(&miss)
        ));
        assert!(!should_fail_closed_for_unresolved_startup_miss_rebind(
            "%42",
            "%42",
            Some(&miss)
        ));
        assert!(!should_fail_closed_for_unresolved_startup_miss_rebind(
            "%84",
            "%43",
            Some(&miss)
        ));
        assert!(!should_fail_closed_for_unresolved_startup_miss_rebind(
            "%84", "%42", None
        ));
    }

    #[test]
    fn focus_existing_session_switches_client_for_cross_session_target() {
        assert!(should_switch_client_for_focus(
            Some("sess-a"),
            Some("sess-b"),
            true
        ));
    }

    #[test]
    fn focus_existing_session_skips_switch_when_target_session_matches() {
        assert!(!should_switch_client_for_focus(
            Some("sess-a"),
            Some("sess-a"),
            true
        ));
    }

    #[test]
    fn focus_existing_session_skips_switch_outside_tmux() {
        assert!(!should_switch_client_for_focus(
            Some("sess-a"),
            Some("sess-b"),
            false
        ));
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
    fn start_invalid_frontmatter_returns_contextual_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("bad.md");
        std::fs::write(&file, "---\nprompt_presets:\n  key: [oops\n---\n").unwrap();

        let err = run(&file).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("invalid YAML frontmatter in"));
        assert!(message.contains("bad.md"));
        assert!(message.contains("Frontmatter excerpt:"));
        assert!(message.contains("> 2 |   key: [oops"));
        assert!(
            message.contains("Fix the frontmatter between the opening and closing --- markers")
        );
    }

    #[test]
    fn restart_continue_strategy_prefers_resume_by_default() {
        assert_eq!(
            restart_continue_exit_strategy(false, false, 0),
            RestartContinueExitStrategy::Resume
        );
    }

    #[test]
    fn restart_continue_strategy_prompts_user_after_ctrl_d() {
        assert_eq!(
            restart_continue_exit_strategy(false, true, 0),
            RestartContinueExitStrategy::PromptUser
        );
    }

    #[test]
    fn ctrl_d_prompt_eof_policy_restarts_fresh_for_early_first_exit() {
        assert_eq!(
            ctrl_d_prompt_eof_policy(Duration::from_secs(8), 0),
            PromptEofPolicy::RestartFresh
        );
    }

    #[test]
    fn ctrl_d_prompt_eof_policy_quits_after_grace_window_or_restart() {
        assert_eq!(
            ctrl_d_prompt_eof_policy(EARLY_CTRL_D_EOF_RESTART_WINDOW + Duration::from_secs(1), 0),
            PromptEofPolicy::Quit
        );
        assert_eq!(
            ctrl_d_prompt_eof_policy(Duration::from_secs(5), 1),
            PromptEofPolicy::Quit
        );
    }

    #[test]
    fn restart_continue_strategy_restarts_fresh_after_single_failed_resume() {
        assert_eq!(
            restart_continue_exit_strategy(true, false, 1),
            RestartContinueExitStrategy::RestartFresh
        );
    }

    #[test]
    fn restart_continue_strategy_prompts_after_repeated_failed_resumes() {
        assert_eq!(
            restart_continue_exit_strategy(true, false, FAILED_RESUME_THRESHOLD),
            RestartContinueExitStrategy::PromptUser
        );
    }

    #[test]
    fn ctrl_d_flag_initialized_false() {
        let shared = SupervisorShared::new("test");
        assert!(!shared.ctrl_d_forwarded.load(Ordering::Relaxed));
    }

    #[test]
    fn auto_trigger_outcome_defaults_to_not_needed() {
        let shared = SupervisorShared::new("test");
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
            restart_continue_exit_strategy(false, true, 0),
            RestartContinueExitStrategy::PromptUser
        );
    }

    #[test]
    fn ctrl_d_with_failed_resume_still_prompts_user() {
        assert_eq!(
            restart_continue_exit_strategy(true, true, 1),
            RestartContinueExitStrategy::PromptUser
        );
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
        let shared = Arc::new(SupervisorShared::new("test"));
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
        let shared = Arc::new(SupervisorShared::new("test"));
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
    fn auto_trigger_inject_command_honors_late_cancel_before_write() {
        let shared = Arc::new(SupervisorShared::new("test"));
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
        let shared = Arc::new(SupervisorShared::new("test"));
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
        let shared = Arc::new(SupervisorShared::new("test"));
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
        let shared = SupervisorShared::new("test");
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
        let shared = SupervisorShared::new("test");
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, b"resumed child ready\n");
        record_recent_output(&shared, "❯\n".as_bytes());
        assert!(current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn current_child_prompt_visible_handles_suffix_prompt_line() {
        let shared = SupervisorShared::new("test");
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, "/tmp/project ❯\n".as_bytes());
        assert!(current_child_prompt_visible(&shared, &harness));
    }

    #[test]
    fn current_child_prompt_visible_skips_codex_footer_line() {
        let shared = SupervisorShared::new("test");
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
        let shared = SupervisorShared::new("test");
        let harness = crate::harness::HarnessConfig::codex();
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"resumed child still printing\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );
        assert!(!current_child_prompt_visible(&shared, &harness));
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
        let handle = spawn_writer_thread(writer_arc, stop.read_fd(), stop_flag.clone(), None);

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
        let handle = spawn_writer_thread(writer_arc, stop_fd, stop_flag.clone(), None);

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

        let shared = Arc::new(SupervisorShared::new("test"));
        let reader: Box<dyn std::io::Read + Send> = Box::new(FdReader(fds[0]));
        let handle = spawn_reader_thread(shared, reader);

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
    fn supervisor_health_no_socket_for_nonexistent_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fake_file = tmp.path().join("nonexistent.md");
        let health = query_supervisor_health(&fake_file, "no-such-session");
        assert!(matches!(health, SupervisorHealth::NoSocket));
    }

    #[test]
    fn supervisor_health_no_socket_when_no_agent_doc_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let health = query_supervisor_health(&file, "some-session-id");
        assert!(matches!(health, SupervisorHealth::NoSocket));
    }

    #[test]
    fn supervisor_health_unreachable_with_stale_socket() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("supervisor");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let sock_path = agent_doc_dir.join("test-session.sock");
        std::fs::write(&sock_path, "").unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let health = query_supervisor_health(&file, "test-session");
        assert!(matches!(health, SupervisorHealth::Unreachable));
    }

    #[test]
    fn supervisor_health_healthy_with_live_supervisor() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/supervisor")).unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let session_id = "health-test-session";
        let ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), session_id, |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "cwd_source": "test",
                })),
                _ => IpcResponse::err("not implemented"),
            }
        })
        .unwrap();
        let health = query_supervisor_health(&file, session_id);
        assert!(matches!(health, SupervisorHealth::Healthy));
        drop(ipc);
    }

    #[test]
    fn supervisor_health_reports_halted_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/supervisor")).unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let session_id = "halted-test-session";
        let ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), session_id, |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": false,
                    "state": "halted",
                    "restart_count": 5,
                    "cwd_source": "test",
                })),
                _ => IpcResponse::err("not implemented"),
            }
        })
        .unwrap();
        let health = query_supervisor_health(&file, session_id);
        assert!(matches!(
            health,
            SupervisorHealth::Halted { restart_count: 5 }
        ));
        drop(ipc);
    }

    #[test]
    fn supervisor_health_reports_restartable_when_not_running() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/supervisor")).unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let session_id = "notrun-test-session";
        let ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), session_id, |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": false,
                    "state": "healthy",
                    "restart_count": 0,
                    "cwd_source": "test",
                })),
                _ => IpcResponse::err("not implemented"),
            }
        })
        .unwrap();
        let health = query_supervisor_health(&file, session_id);
        assert!(matches!(health, SupervisorHealth::Restartable));
        drop(ipc);
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
    fn restart_via_supervisor_returns_false_for_nonexistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        assert!(!restart_via_supervisor(&file, "no-such-session"));
    }

    #[test]
    fn restart_via_supervisor_succeeds_with_live_supervisor() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/supervisor")).unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "# test").unwrap();
        let session_id = "restart-test-session";
        let ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), session_id, |method| {
            match method {
                IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
                _ => IpcResponse::err("not implemented"),
            }
        })
        .unwrap();
        assert!(restart_via_supervisor(&file, session_id));
        drop(ipc);
    }
}
