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
//! - When `--route-owned` is set by `route` auto-start, watches for new
//!   binary-owned document cycles to reach `committed`. It reaps only one-shot
//!   panes; multi-turn documents with live backlog, queue, dirty edits, or an
//!   unresolved exchange-tail prompt stay alive for continued interaction.
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
//!   prompt still has not appeared after a hard 30-second deadline
//!   (`AUTO_TRIGGER_TIMEOUT`), the thread fails closed (`#startupdeadline`):
//!   it records a `startup_miss` marker against the owned pane and surfaces an
//!   actionable "session did not become dispatch-ready in Ns" diagnostic on
//!   stderr instead of watching the hung child forever. The same hard-deadline
//!   fail-closed path covers the clear-cooldown and managed capability-proof
//!   waits so no startup branch can hang silently.
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
//! - `auto_trigger_no_prompt_continues_before_deadline_then_fails_closed`: the
//!   no-prompt auto-trigger wait keeps polling before `AUTO_TRIGGER_TIMEOUT`
//!   and fails closed exactly once at the hard deadline so the caller records a
//!   `startup_miss` and returns instead of watching the child forever
//!   (`#startupdeadline`).

use anyhow::{Context, Result};
use portable_pty::PtySize;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use crate::supervisor::{
    cwd,
    env::EnvSpec,
    in_process::{InProcessSupervisor, PtySupervisedChild, TickOutcome},
    ipc::{IpcMethod, IpcResponse, SupervisorIpc},
    pty::PtySpawnConfig,
    resize,
    state::{CrashPolicy, RestartAction, SupervisorState},
};
use agent_doc_frontmatter::frontmatter;
#[cfg(test)]
use agent_doc_queue::queue::{
    IdleQueueContextResetDecision, IdleQueueDrainDecision, clean_session_head_forces_context_reset,
    idle_queue_context_reset_decision, idle_queue_drain_decision,
};
#[cfg(unix)]
use agent_doc_supervisor_process::ReexecState;

use crate::{config, project_config_io, sessions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum RouteOwnedReapPolicy {
    Auto,
    ReapAfterCommit,
    KeepAlive,
}

impl RouteOwnedReapPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap_after_commit",
            Self::KeepAlive => "keep_alive",
        }
    }
}

impl std::fmt::Display for RouteOwnedReapPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap-after-commit",
            Self::KeepAlive => "keep-alive",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteOwnedReapDecision {
    reap: bool,
    reason: String,
}

struct RouteOwnedCompletionConfig {
    file: PathBuf,
    baseline: Option<crate::cycle_state::CycleState>,
    reap_policy: RouteOwnedReapPolicy,
    harness: crate::harness::HarnessConfig,
}

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
    // `#opslogts` — human-readable ISO-8601 UTC so operators reading the
    // supervisor session log can correlate events to wall-clock time. The
    // staleness/startup-miss parsers read this back via
    // `agent_doc_log_time::parse_log_timestamp`, which still accepts bare epoch lines.
    agent_doc_log_time::format_log_timestamp(now)
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
/// Auto-trigger no-prompt dispatch-ready deadline (`#startupdeadline` /
/// `#waitmachine2` / `#contrestartdispatch`).
///
/// Bounds how long the auto-trigger thread waits for a freshly (re)launched
/// harness child to finish starting up and show a dispatch-ready prompt before it
/// gives up on re-injecting `agent-doc <FILE>`. It is deliberately NOT clamped to
/// [`agent_doc_turn::wait_machine::GLOBAL_HANG_CEILING`] (10s): that ceiling bounds waits
/// on peers expected to respond near-instantly (an IPC ack, an already-running
/// shell prompt), but waiting on a cold-(re)starting harness *process* is a
/// legitimately slow startup wait. A `claude --continue` resuming a large session
/// plus heavy SessionStart hooks routinely needs well over 10s to reach its first
/// prompt, so clamping to 10s made every continue-mode `restart-supervisor` time
/// out before the prompt appeared: the re-dispatch never fired and the relaunched
/// operator came up unclaimed, leaving the controller parked at `operator_ready`
/// (`#contrestartdispatch`). Like the bounded [`agent_doc_turn::wait_machine::REINSTALL_BUDGET`]
/// exemption, this stays bounded — a child that never becomes dispatch-ready
/// still fails closed at this budget and records a `startup_miss`, never hanging
/// forever. The auto-trigger runs on its own `AutoTriggerMonitor`, not the
/// Lean-proofed `wait_machine::tick`, so the global `no_hang` theorem is
/// unaffected.
const AUTO_TRIGGER_TIMEOUT: Duration = Duration::from_secs(60);
/// Consecutive idle-over-busy polls the idle-queue watch must observe before it
/// reconciles a stale-busy actor back to ready (`#stale-busy-after-auto-inject-no-clear`).
/// At `AUTO_TRIGGER_POLL_INTERVAL` (500ms) this is ~2s of proven idle pane
/// evidence — long enough that a turn still spinning up is never cut short.
const STALE_BUSY_RECONCILE_TICKS: u32 = 4;
/// Consecutive idle-prompt polls the idle-queue watch must observe after a
/// lingering *manual* clear cooldown before it auto-expires the cooldown and
const AUTO_TRIGGER_OUTPUT_BYTES_MAX: usize = 64 * 1024;
const ROUTE_OWNED_COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS: u32 = STALE_BUSY_RECONCILE_TICKS;
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
    SkippedClearCooldown = 6,
}

impl AutoTriggerOutcome {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Sent,
            3 => Self::Timeout,
            4 => Self::SendFailed,
            5 => Self::Cancelled,
            6 => Self::SkippedClearCooldown,
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
            Self::SkippedClearCooldown => "skipped_clear_cooldown",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoTriggerCooldownAction {
    Wait,
    Timeout,
}

fn auto_trigger_clear_cooldown_action(
    monitor: &mut AutoTriggerMonitor,
    now: Instant,
) -> AutoTriggerCooldownAction {
    if monitor.note_no_prompt(now) {
        AutoTriggerCooldownAction::Timeout
    } else {
        AutoTriggerCooldownAction::Wait
    }
}

/// Hard-deadline decision for the auto-trigger no-prompt wait (`#startupdeadline`).
///
/// The auto-trigger thread used to log a *provisional* `no_prompt_after_30s`
/// timeout and then keep watching the child forever (until it exited or a prompt
/// finally appeared). A harness that never becomes dispatch-ready (hung TUI,
/// auth wall, stuck network) therefore left the session silently hanging with no
/// recoverable signal. This makes the deadline hard: once the monitor's timeout
/// expires without a dispatch-ready prompt, the thread fails closed instead of
/// continuing to poll, mirroring the existing clear-cooldown / capability-proof
/// timeout branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoTriggerNoPromptAction {
    Continue,
    FailClosed,
}

fn auto_trigger_no_prompt_action(
    monitor: &mut AutoTriggerMonitor,
    now: Instant,
) -> AutoTriggerNoPromptAction {
    if monitor.note_no_prompt(now) {
        AutoTriggerNoPromptAction::FailClosed
    } else {
        AutoTriggerNoPromptAction::Continue
    }
}

/// Fail-closed handler for an expired session-startup deadline: record a
/// `startup_miss` marker against the owned pane and surface an actionable
/// "session did not become dispatch-ready in Ns" diagnostic on stderr, so a hung
/// harness child becomes a recoverable, dogfoodable error instead of an
/// indefinite hang (`#startupdeadline`). `reason` is the timeout provenance
/// (`no_prompt`, `capability_proof`, `clear_cooldown`).
fn record_session_startup_miss(
    path: &Path,
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    session_log: &mut Option<std::fs::File>,
    reason: &str,
) {
    let pane = shared.inject_pane.as_deref().unwrap_or("child_pty");
    let session_id = crate::frontmatter_io::read_session_id(path).unwrap_or_default();
    let deadline_secs = AUTO_TRIGGER_TIMEOUT.as_secs();
    match crate::startup_miss::record(
        path,
        pane,
        &session_id,
        &harness.binary,
        crate::startup_miss::StartupMissOrigin::FreshStart,
        None,
    ) {
        Ok(_) => log_event(
            session_log,
            &format!(
                "startup_miss_recorded harness={} pane={} reason={} deadline_secs={}",
                harness.binary, pane, reason, deadline_secs
            ),
        ),
        Err(e) => log_event(
            session_log,
            &format!(
                "startup_miss_record_failed harness={} pane={} reason={} error={}",
                harness.binary, pane, reason, e
            ),
        ),
    }
    eprintln!(
        "[agent-doc] session did not become dispatch-ready in {}s ({}) for {}; recorded startup-miss and failing closed instead of hanging. Run 'agent-doc start {}' to retry.",
        deadline_secs,
        reason,
        harness.binary,
        path.display()
    );
}

mod idle_watch;

fn idle_queue_head_slash_command(active_head: &str) -> Option<String> {
    agent_doc_queue::queue_command::slash_command_text(active_head)
}

fn turn_active_for_owned_pane(file: &Path, shared: &SupervisorShared) -> bool {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return false;
    };
    let Some(marker) = crate::turn_status::read_turn_active_marker(&root) else {
        return false;
    };
    let owned_pane = shared.inject_pane.as_deref().or_else(|| {
        shared
            .actor_runtime
            .as_ref()
            .map(|runtime| runtime.pane_id.as_str())
    });
    match owned_pane {
        Some(pane) => marker.pane == pane,
        None => true,
    }
}

fn complete_idle_queue_slash_command_head(
    file: &Path,
    expected_head: &str,
    command: &str,
    session_log: &mut Option<std::fs::File>,
) -> bool {
    match crate::write::consume_queue_prompt_force_disk(file) {
        Ok(Some(outcome)) => {
            if outcome.consumed_text.trim() != expected_head.trim() {
                log_event(
                    session_log,
                    &format!(
                        "idle_queue_watch_slash_command_consumed_unexpected_head expected={:?} consumed={:?} cmd={:?}",
                        expected_head, outcome.consumed_text, command
                    ),
                );
            }
            match crate::git::commit(file) {
                Ok(did_commit) => {
                    log_event(
                        session_log,
                        &format!(
                            "idle_queue_watch_slash_command_head_completed cmd={:?} remaining={} drained={} committed={}",
                            command, outcome.remaining, outcome.drained, did_commit
                        ),
                    );
                    true
                }
                Err(err) => {
                    log_event(
                        session_log,
                        &format!(
                            "idle_queue_watch_slash_command_commit_failed cmd={:?} error={:?}",
                            command,
                            err.to_string()
                        ),
                    );
                    eprintln!(
                        "[agent-doc] idle-queue watch: submitted {command:?} but failed to commit queue command completion for {}: {err:#}",
                        file.display()
                    );
                    false
                }
            }
        }
        Ok(None) => {
            log_event(
                session_log,
                &format!(
                    "idle_queue_watch_slash_command_no_head_to_consume cmd={:?}",
                    command
                ),
            );
            false
        }
        Err(err) => {
            log_event(
                session_log,
                &format!(
                    "idle_queue_watch_slash_command_consume_failed cmd={:?} error={:?}",
                    command,
                    err.to_string()
                ),
            );
            eprintln!(
                "[agent-doc] idle-queue watch: submitted {command:?} but failed to consume queue command head for {}: {err:#}",
                file.display()
            );
            false
        }
    }
}

fn idle_queue_drain_payload(
    file: &str,
    harness: &crate::harness::HarnessConfig,
    active_head: &str,
) -> String {
    if let Some(command) = idle_queue_head_slash_command(active_head) {
        return command;
    }
    harness.trigger_command(file)
}

fn idle_queue_drain_payload_kind(
    _harness: &crate::harness::HarnessConfig,
    active_head: &str,
) -> &'static str {
    if idle_queue_head_slash_command(active_head).is_some() {
        "slash_command"
    } else {
        "trigger"
    }
}

fn idle_queue_submit_mode(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> &'static str {
    if shared.inject_pane.is_some() {
        crate::sessions::tmux_submit_mode_for_harness(&harness.binary)
    } else {
        "pty_cr"
    }
}

fn log_idle_queue_drain_submit(
    file: &Path,
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    payload_kind: &str,
    active_head: &str,
    drain_payload: &str,
) {
    let target = shared.inject_pane.as_deref().unwrap_or("child_pty");
    crate::ops_log::log_op(
        file,
        &format!(
            "idle_queue_watch_drain file={} harness={} payload_kind={} submit_mode={} target={} head_bytes={} head_sha256={} payload_bytes={} proof=go_drain_dispatch",
            file.display(),
            harness.binary,
            payload_kind,
            idle_queue_submit_mode(shared, harness),
            target,
            active_head.len(),
            crate::ops_log::content_hash(active_head),
            drain_payload.len(),
        ),
    );
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

fn clean_exit_resolution_for_start(
    harness: &crate::harness::HarnessConfig,
    route_owned: bool,
) -> CleanExitResolution {
    if route_owned {
        return CleanExitResolution::PromptUser;
    }
    clean_exit_resolution(harness)
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
        && current.phase == agent_doc_turn::CyclePhase::Committed
}

fn route_owned_file_dirty_after_commit(
    content: &str,
    state: &crate::cycle_state::CycleState,
) -> bool {
    state
        .file_hash
        .as_ref()
        .is_some_and(|hash| crate::ops_log::content_hash(content) != *hash)
}

fn route_owned_backlog_has_live_items(body: &str) -> bool {
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
    items
        .iter()
        .any(|item| item.state != agent_doc_element_backlog::backlog::PendingState::Done)
}

fn route_owned_queue_has_prompts(body: &str) -> bool {
    match agent_doc_queue::document_queue::parse(body) {
        Ok(entries) => !agent_doc_queue::document_queue::prompts(&entries).is_empty(),
        Err(_) => !body.trim().is_empty(),
    }
}

fn route_owned_exchange_tail_has_unresolved_prompt(body: &str) -> bool {
    let mut tail_start = 0usize;
    let mut line_start = 0usize;
    for line in body.split_inclusive('\n') {
        if route_owned_line_is_response_heading(line.trim()) {
            tail_start = line_start + line.len();
        }
        line_start += line.len();
    }
    if line_start < body.len() && route_owned_line_is_response_heading(body[line_start..].trim()) {
        tail_start = body.len();
    }

    body[tail_start..]
        .lines()
        .any(agent_doc_diff::text_line_looks_like_prompt_target)
}

fn route_owned_line_is_response_heading(line: &str) -> bool {
    line == "## Assistant"
        || line.starts_with("### Re:")
        || line.starts_with("#### Re:")
        || line.starts_with("##### Re:")
}

fn route_owned_liveness_reason(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Option<String> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(err) => return Some(format!("read_failed:{err}")),
    };
    let dirty_after_commit = route_owned_file_dirty_after_commit(&content, state);
    if dirty_after_commit && route_owned_exchange_tail_has_unresolved_prompt(&content) {
        return Some("post_commit_user_follow_up".to_string());
    }

    let components = match agent_doc_element::element::parse(&content) {
        Ok(components) => components,
        Err(err) => {
            return Some(if dirty_after_commit {
                "document_dirty_after_commit".to_string()
            } else {
                format!("component_parse_failed:{err}")
            });
        }
    };

    for component in &components {
        let body = component.content(&content);
        if agent_doc_element::element::is_backlog_component(&component.name)
            && route_owned_backlog_has_live_items(body)
        {
            return Some("backlog_non_empty".to_string());
        }
        if component.name == "queue" && route_owned_queue_has_prompts(body) {
            return Some("queue_non_empty".to_string());
        }
        if component.name == "exchange" && route_owned_exchange_tail_has_unresolved_prompt(body) {
            return Some(if dirty_after_commit {
                "post_commit_user_follow_up".to_string()
            } else {
                "exchange_tail_unresolved_prompt".to_string()
            });
        }
    }

    if dirty_after_commit {
        return Some("document_dirty_after_commit".to_string());
    }

    None
}

fn route_owned_reap_decision(
    file: &Path,
    state: &crate::cycle_state::CycleState,
    policy: RouteOwnedReapPolicy,
) -> RouteOwnedReapDecision {
    match policy {
        RouteOwnedReapPolicy::KeepAlive => RouteOwnedReapDecision {
            reap: false,
            reason: "explicit_keep_alive".to_string(),
        },
        RouteOwnedReapPolicy::ReapAfterCommit => RouteOwnedReapDecision {
            reap: true,
            reason: "explicit_reap_after_commit".to_string(),
        },
        RouteOwnedReapPolicy::Auto => {
            if let Some(reason) = route_owned_liveness_reason(file, state) {
                RouteOwnedReapDecision {
                    reap: false,
                    reason,
                }
            } else {
                RouteOwnedReapDecision {
                    reap: true,
                    reason: "no_liveness_signals".to_string(),
                }
            }
        }
    }
}

fn route_owned_live_pane_busy_reason(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> Option<String> {
    if !shared.running.load(Ordering::Relaxed) {
        return None;
    }
    let output = child_output_for_detection(shared);
    if let Some(reason) = harness.dispatch_blocker_reason(&output) {
        return Some(format!("live_pane_busy_blocked_prompt reason={reason}"));
    }
    if shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state == crate::session_actor::ActorState::Ready)
    {
        return None;
    }
    if current_child_prompt_visible(shared, harness) {
        return None;
    }
    let tail = harness
        .last_prompt_candidate(&output)
        .map(|line| line.chars().take(80).collect::<String>())
        .unwrap_or_else(|| "no_recent_output".to_string());
    Some(format!("live_pane_busy_no_idle_prompt tail={tail:?}"))
}

fn owned_pane_label(shared: &SupervisorShared) -> &str {
    shared.inject_pane.as_deref().unwrap_or_else(|| {
        shared
            .actor_runtime
            .as_ref()
            .map(|runtime| runtime.pane_id.as_str())
            .unwrap_or("<pty>")
    })
}

fn spawn_route_owned_completion_thread(
    shared: Arc<SupervisorShared>,
    config: RouteOwnedCompletionConfig,
    completed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("route-owned-completion".into())
        .spawn(move || {
            let RouteOwnedCompletionConfig {
                file,
                mut baseline,
                reap_policy,
                harness,
            } = config;
            let mut logged_busy_cycle: Option<String> = None;
            let mut ready_busy_ticks: u32 = 0;
            let mut ready_busy_key: Option<(String, String)> = None;
            let mut ready_busy_logged_key: Option<(String, String)> = None;
            while !stop.load(Ordering::Relaxed) && !completed.load(Ordering::Relaxed) {
                if let Ok(Some(state)) = crate::cycle_state::load(&file)
                    && route_owned_cycle_completed_after_start(&state, baseline.as_ref())
                {
                    let actor_ready = actor_state_is_ready(&shared);
                    let ready_busy_reason = if actor_ready {
                        ready_busy_blocker_reason(&shared, &harness)
                    } else {
                        None
                    };
                    let key = ready_busy_reason
                        .as_ref()
                        .map(|reason| (state.cycle_id.clone(), reason.clone()));
                    if key.is_some() && key == ready_busy_key {
                        ready_busy_ticks = ready_busy_ticks.saturating_add(1);
                    } else {
                        ready_busy_key = key.clone();
                        ready_busy_ticks = u32::from(key.is_some());
                    }
                    let ready_busy_reconciled = ready_busy_conflict_reconcile_decision(
                        actor_ready,
                        ready_busy_reason.as_deref(),
                        false,
                        ready_busy_ticks,
                    );
                    if ready_busy_reconciled
                        && key.is_some()
                        && ready_busy_logged_key.as_ref() != key.as_ref()
                    {
                        let reason = ready_busy_reason.as_deref().unwrap_or("unknown");
                        let event = format!(
                            "owned_pane_ready_busy_conflict source=route_owned_completion harness={} pane={} reason={:?} after_ticks={} cycle={} event={}",
                            harness.binary,
                            owned_pane_label(&shared),
                            reason,
                            ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS,
                            state.cycle_id,
                            state.last_event
                        );
                        log_event(&mut session_log, &event);
                        crate::ops_log::log_op(&file, &event);
                        ready_busy_logged_key = key.clone();
                    }

                    let live_pane_busy_reason = if ready_busy_reconciled {
                        None
                    } else {
                        route_owned_live_pane_busy_reason(&shared, &harness)
                    };

                    let decision = if let Some(reason) = live_pane_busy_reason {
                        RouteOwnedReapDecision {
                            reap: false,
                            reason,
                        }
                    } else {
                        route_owned_reap_decision(&file, &state, reap_policy)
                    };
                    let event = format!(
                        "route_owned_reap_decision policy={} decision={} reason={} cycle={} event={}",
                        reap_policy.as_str(),
                        if decision.reap { "reap" } else { "keep_alive" },
                        decision.reason,
                        state.cycle_id,
                        state.last_event
                    );
                    let busy_guard = decision.reason.starts_with("live_pane_busy_no_idle_prompt");
                    if !busy_guard || logged_busy_cycle.as_deref() != Some(&state.cycle_id) {
                        log_event(&mut session_log, &event);
                        crate::ops_log::log_op(&file, &event);
                    }
                    if decision.reap {
                        completed.store(true, Ordering::Relaxed);
                        shared.stop_requested.store(true, Ordering::Relaxed);
                        shared.kill_child();
                        return;
                    }
                    if busy_guard {
                        logged_busy_cycle = Some(state.cycle_id.clone());
                    } else {
                        logged_busy_cycle = None;
                        baseline = Some(state);
                    }
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

pub(crate) struct SharedPtyWriter {
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
        let profile = crate::sessions::tmux_submit_profile_for_harness(&shared.harness_binary);
        crate::input_diag::log_text_submit(
            None,
            "supervisor.auto_trigger",
            &format!("pane:{pane_id}"),
            &submitted_text,
            Some(&shared.harness_binary),
            profile.transform(),
            profile.submit_key(),
        );
        return match dispatch_submit_text_to_pane(pane_id, &submitted_text, &shared.harness_binary)
        {
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
    crate::input_diag::log_text_submit(
        None,
        "supervisor.auto_trigger",
        "child_pty",
        &submitted_text,
        Some(&shared.harness_binary),
        "raw_pty_submit_enter_byte",
        "Enter",
    );

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

fn auto_trigger_clear_command(
    shared: &SupervisorShared,
    stop: &AtomicBool,
    clear_cmd: &str,
) -> AutoTriggerOutcome {
    if stop.load(Ordering::Relaxed) {
        return AutoTriggerOutcome::Cancelled;
    }
    shared.transition_actor_state(
        crate::session_actor::ActorState::Busy,
        "operator",
        "auto_trigger_clear",
    );
    let submitted_text = crate::supervisor::ipc::normalize_submit_text(clear_cmd);
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        let profile = crate::sessions::tmux_submit_profile_for_harness(&shared.harness_binary);
        crate::input_diag::log_text_submit(
            None,
            "supervisor.auto_trigger_clear",
            &format!("pane:{pane_id}"),
            &submitted_text,
            Some(&shared.harness_binary),
            profile.transform(),
            profile.submit_key(),
        );
        return match dispatch_submit_text_to_pane(pane_id, &submitted_text, &shared.harness_binary)
        {
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
    crate::input_diag::log_text_submit(
        None,
        "supervisor.auto_trigger_clear",
        "child_pty",
        &submitted_text,
        Some(&shared.harness_binary),
        "raw_pty_clear_enter_byte",
        "Enter",
    );

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

fn auto_trigger_submit_queue_command(
    shared: &SupervisorShared,
    stop: &AtomicBool,
    command: &str,
) -> AutoTriggerOutcome {
    if agent_doc_queue::queue_command::is_context_clear_command(command) {
        auto_trigger_clear_command(shared, stop, command)
    } else {
        auto_trigger_inject_command(shared, stop, command)
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
    harness: &str,
) -> Result<()> {
    crate::sessions::send_submitted_text_for_harness(tmux, pane, text, harness)
        .with_context(|| format!("failed to inject submitted input into pane {}", pane))
}

fn dispatch_submit_text_to_pane(pane: &str, text: &str, harness: &str) -> Result<()> {
    let tmux = crate::sessions::Tmux::default_server();
    dispatch_submit_text_to_tmux(&tmux, pane, text, harness)
}

mod detection;
pub(crate) use detection::*;

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
            let path = PathBuf::from(&file);
            let mut clear_cooldown_logged = false;
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
                if clear_cooldown_blocks_auto_dispatch(
                    &path,
                    &harness,
                    "auto_trigger",
                    &mut session_log,
                    &mut clear_cooldown_logged,
                ) {
                    match auto_trigger_clear_cooldown_action(&mut monitor, Instant::now()) {
                        AutoTriggerCooldownAction::Wait => continue,
                        AutoTriggerCooldownAction::Timeout => {
                            shared
                                .auto_trigger_outcome
                                .store(AutoTriggerOutcome::Timeout as u8, Ordering::Relaxed);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "auto_trigger_timeout harness={} reason=clear_cooldown_after_{}s",
                                    harness.binary, AUTO_TRIGGER_TIMEOUT.as_secs()
                                ),
                            );
                            record_session_startup_miss(
                                &path,
                                &shared,
                                &harness,
                                &mut session_log,
                                "clear_cooldown",
                            );
                            return;
                        }
                    }
                }
                if current_child_prompt_visible(&shared, &harness) {
                    // `#capproofbg`: do NOT stall the auto-trigger waiting for the
                    // managed-capability proof to finish. Dispatch proceeds as soon
                    // as the child prompt is visible; the proof runs in the
                    // background and only a proven FAILURE (surfaced async via the
                    // session log + tmux `display-message`) gates subsequent
                    // dispatch through `auto_trigger_inject_command` →
                    // `capability_dispatch_blocker`.
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
                            // Already in session_log; gate stderr so repeated
                            // drain-cycle triggers don't bleed in front of a
                            // full-screen harness TUI. (#opencode-stdout-bleed)
                            if crate::input_diag::verbose_enabled() {
                                eprintln!("[agent-doc] auto-triggered: {}", trigger_cmd);
                            }
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
                            // Already in session_log; gate stderr (#opencode-stdout-bleed).
                            if crate::input_diag::verbose_enabled() {
                                eprintln!("[agent-doc] auto-trigger failed");
                            }
                        }
                        outcome => {
                            shared
                                .auto_trigger_outcome
                                .store(outcome as u8, Ordering::Relaxed);
                        }
                    }
                    return;
                }
                if is_help_screen_visible(&shared, &harness) {
                    shared
                        .auto_trigger_outcome
                        .store(AutoTriggerOutcome::Cancelled as u8, Ordering::Relaxed);
                    log_event(
                        &mut session_log,
                        &format!(
                            "auto_trigger_help_screen harness={} reason=help_usage_screen_detected",
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[agent-doc] auto-trigger: help/usage screen detected, skipping trigger"
                    );
                    return;
                }
                match auto_trigger_no_prompt_action(&mut monitor, Instant::now()) {
                    AutoTriggerNoPromptAction::Continue => {}
                    AutoTriggerNoPromptAction::FailClosed => {
                        shared
                            .auto_trigger_outcome
                            .store(AutoTriggerOutcome::Timeout as u8, Ordering::Relaxed);
                        log_event(
                            &mut session_log,
                            &format!(
                                "auto_trigger_timeout harness={} reason=no_prompt_after_{}s",
                                harness.binary, AUTO_TRIGGER_TIMEOUT.as_secs()
                            ),
                        );
                        // Hard deadline: record startup-miss + fail closed instead of
                        // silently watching the child forever (`#startupdeadline`).
                        record_session_startup_miss(
                            &path,
                            &shared,
                            &harness,
                            &mut session_log,
                            "no_prompt",
                        );
                        return;
                    }
                }
            }
        })
        .expect("spawn auto-trigger thread")
}

fn clear_cooldown_blocks_auto_dispatch(
    path: &Path,
    harness: &crate::harness::HarnessConfig,
    source: &str,
    session_log: &mut Option<std::fs::File>,
    logged: &mut bool,
) -> bool {
    match crate::queue_continuation::clear_cooldown_active(path) {
        Ok(true) => {
            if !*logged {
                log_event(
                    session_log,
                    &format!(
                        "{source}_skipped harness={} reason=clear_cooldown file={}",
                        harness.binary,
                        path.display()
                    ),
                );
                eprintln!(
                    "[agent-doc] {source}: clear cooldown active for {}, skipping passive queue dispatch",
                    path.display()
                );
                *logged = true;
            }
            true
        }
        Ok(false) => {
            *logged = false;
            false
        }
        Err(err) => {
            if !*logged {
                log_event(
                    session_log,
                    &format!(
                        "{source}_skipped harness={} reason=clear_cooldown_error file={} error={:?}",
                        harness.binary,
                        path.display(),
                        err.to_string()
                    ),
                );
                eprintln!(
                    "[agent-doc] {source}: failed to inspect clear cooldown for {}, skipping passive queue dispatch: {err:#}",
                    path.display()
                );
                *logged = true;
            }
            true
        }
    }
}

/// Long-lived idle-queue watch thread for the supervisor
/// (`#jb-run-agent-doc-busy-queue-dispatch-deadlock`).
///
/// Unlike [`spawn_auto_trigger_thread`] (a one-shot restart continuation), this
/// watch runs for the whole child lifetime. On every busy→idle transition it
/// drains a live `agent:queue auto` head — including one a busy-pane
/// `Run Agent Doc` route appended via `enqueue_route_dispatch_prompt` — by
/// injecting a harness-specific drain payload. Claude/OpenCode keep their
/// normal harness trigger, while Codex receives an in-owner-pane continuation
/// prompt so it answers the head instead of recursively running `agent-doc
/// <FILE>` in the pane that already owns the document. It is the
/// supervisor-owned drain guarantee the route busy path lacked: route enqueues +
/// returns `Ok`, this thread fires the dispatch once the pane goes idle so the
/// queued prompt is never stranded.
///
/// Invariants:
/// - Never injects while the pane is mid-turn (no-inject-into-active-turn).
/// - Dedups on the head text so a stuck/undrained head cannot hot-loop.
/// - Respects the managed capability-proof gate, same as the auto-trigger.
///
/// `#ctlrecycle` R3 — replace this stale supervisor's process image with the
/// freshly-installed binary IN PLACE (`execve`), preserving the live harness child
/// and the tmux pane (`#ctlrecycle` R3, opt-in via `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`).
///
/// The child (a separate PID) survives the image swap untouched; we dup the live pty
/// master fd, clear its CLOEXEC so it survives `exec`, and hand the new image the
/// child PID + inherited fd through the environment. The new image re-enters
/// `run_with_reap_policy`, re-runs all supervisor setup (IPC, sessions, controller,
/// watchers) for free, and calls [`PtySession::adopt`] instead of spawning.
///
/// Returns `Err` if it could not even begin the exec (no live child/master fd, or
/// `exec` itself failed). On success it never returns. The caller falls back to a
/// clean `process::exit(0)` so a recycle still happens (the child restarts) rather
/// than wedging on the stale binary.
/// Ordered, de-duplicated candidate binary paths the supervisor self-`execve` may
/// target, each tagged with a short diagnostic note. `exec` tries them in order and
/// the first one it accepts wins; the rest exist so a single unresolvable path (an
/// old pre-fix launch path, a `(deleted)` `current_exe`, a `PATH`-only install)
/// cannot doom the whole recycle. The notes are surfaced in the failure log so a
/// recurring ENOENT is diagnosable instead of a bare "os error 2".
#[cfg(unix)]
fn supervisor_reexec_candidates() -> Vec<(PathBuf, &'static str)> {
    // 2. `current_exe()` is only a usable candidate when it is still a launchable
    //    file. macOS keeps a launchable `current_exe()` path after the binary is
    //    replaced (no `(deleted)` suffix); Linux reports a deleted inode that
    //    `exec` rejects with ENOENT, so it is dropped here.
    let current_exe = std::env::current_exe().ok();
    let current_exe_launchable = current_exe
        .as_ref()
        .map(|path| {
            std::fs::metadata(path)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false)
        })
        .unwrap_or(false);
    // 1. The freshly-installed launchable binary (skips a `(deleted)` current_exe,
    //    follows argv0 + `PATH` to the on-disk build).
    let resolved_fresh = crate::project_controller::current_agent_doc_binary().ok();
    build_reexec_candidates(resolved_fresh, current_exe, current_exe_launchable)
}

/// Pure candidate-ladder builder (env gathered by [`supervisor_reexec_candidates`]).
/// Ordered, de-duplicated, always ending with the bare-name `PATH` fallback so the
/// list is never empty.
#[cfg(unix)]
fn build_reexec_candidates(
    resolved_fresh: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    current_exe_launchable: bool,
) -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut push = |path: PathBuf, note: &'static str| {
        if !out.iter().any(|(existing, _)| existing == &path) {
            out.push((path, note));
        }
    };
    if let Some(path) = resolved_fresh {
        push(path, "resolved_fresh_binary");
    }
    if current_exe_launchable && let Some(path) = current_exe {
        push(path, "current_exe");
    }
    // Bare `agent-doc` → OS `PATH` lookup at exec time. Last-resort fallback so a
    // `PATH`-only install still recycles even if argv0/`current_exe` are gone.
    push(PathBuf::from("agent-doc"), "path_lookup_agent_doc");
    out
}

#[cfg(unix)]
fn supervisor_perform_reexec(
    shared: &SupervisorShared,
) -> std::io::Result<std::convert::Infallible> {
    use std::os::unix::process::CommandExt;
    let child_pid = shared.child_pid.load(Ordering::Relaxed);
    let master_fd = shared.master_fd.load(Ordering::Relaxed);
    if child_pid == 0 || master_fd < 0 {
        return Err(std::io::Error::other(
            "reexec: no live child/master fd to preserve",
        ));
    }
    // Dup the master fd and clear CLOEXEC on the dup so it survives the execve and the
    // new image can adopt it. The original fd (CLOEXEC) closes on exec as usual.
    let inherited = unsafe { libc::dup(master_fd) };
    if inherited < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let flags = unsafe { libc::fcntl(inherited, libc::F_GETFD) };
    if flags < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(inherited) };
        return Err(err);
    }
    if unsafe { libc::fcntl(inherited, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(inherited) };
        return Err(err);
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let state = ReexecState {
        child_pid,
        master_fd: inherited,
    };
    // Try every candidate in order. `exec()` replaces the current process image and
    // only returns on failure, so a returning call means this candidate could not
    // launch — record the path + errno and fall through to the next. Re-using the
    // same inherited (CLOEXEC-cleared) fd across attempts is safe: a failed `exec`
    // never closed it.
    let candidates = supervisor_reexec_candidates();
    let mut attempts: Vec<String> = Vec::with_capacity(candidates.len());
    for (exe, note) in &candidates {
        let exists_before = std::fs::metadata(exe)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false);
        let mut cmd = std::process::Command::new(exe);
        cmd.args(&argv);
        for (key, value) in state.to_env() {
            cmd.env(key, value);
        }
        let err = cmd.exec();
        attempts.push(format!(
            "{note} path={} exists_before={exists_before} errno={:?} ({err})",
            exe.display(),
            err.raw_os_error(),
        ));
    }
    // Every candidate failed; the inherited fd is still ours — close it so the
    // clean-exit fallback (or continued run on the current image) does not leak it.
    unsafe { libc::close(inherited) };
    Err(std::io::Error::other(format!(
        "reexec: all {} candidate(s) failed: [{}]",
        candidates.len(),
        attempts.join("; "),
    )))
}

struct ManagedCapabilityProofTask {
    proof_epoch: u64,
    harness_binary: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    frontmatter: frontmatter::Frontmatter,
    global_config: config::Config,
    session_log: Option<std::fs::File>,
}

fn spawn_managed_capability_proof_thread(
    shared: Arc<SupervisorShared>,
    task: ManagedCapabilityProofTask,
) -> std::thread::JoinHandle<()> {
    let ManagedCapabilityProofTask {
        proof_epoch,
        harness_binary,
        args,
        env,
        frontmatter,
        global_config,
        mut session_log,
    } = task;
    let thread_name = format!("{harness_binary}-capability-proof");
    let policy = crate::agent::resolve_managed_proof_policy(&frontmatter, &global_config);
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            // Bounded re-prove: a transient network blip no longer permanently
            // wedges the session. Retry the probe (with exponential back-off) up
            // to `policy.max_attempts` before committing the gate to `Failed`.
            // The gate stays `Pending` between attempts so dispatch remains gated
            // but recoverable rather than dead.
            let mut attempt: u32 = 0;
            loop {
                if !shared.capability_proof_epoch_current(proof_epoch) {
                    return;
                }
                attempt += 1;
                match crate::agent::codex::prove_managed_session_capabilities(
                    &harness_binary,
                    &args,
                    &env,
                    &frontmatter,
                    &global_config,
                    &harness_binary,
                    policy.probe_timeout,
                ) {
                    Ok(Some(event)) => {
                        if !shared.set_capability_proof_gate_for_epoch(
                            proof_epoch,
                            CapabilityProofGate::Proven,
                            None,
                        ) {
                            return;
                        }
                        if !shared.capability_proof_epoch_current(proof_epoch) {
                            return;
                        }
                        surface_managed_capability_proof_status(&shared, &harness_binary, &event);
                        log_event(&mut session_log, &event);
                        return;
                    }
                    Ok(None) => {
                        if !shared.set_capability_proof_gate_for_epoch(
                            proof_epoch,
                            CapabilityProofGate::NotRequired,
                            None,
                        ) {
                            return;
                        }
                        if !shared.capability_proof_epoch_current(proof_epoch) {
                            return;
                        }
                        log_event(
                            &mut session_log,
                            &format!("{}_capability_proof status=not_required", harness_binary),
                        );
                        return;
                    }
                    Err(err) => {
                        let detail = err.to_string();
                        match crate::agent::proof_retry_decision(
                            attempt,
                            policy.max_attempts,
                            policy.base_backoff,
                        ) {
                            crate::agent::ProofRetryDecision::Retry { backoff } => {
                                // Keep the gate `Pending` (gated but not failed)
                                // while we back off and re-prove.
                                if !shared.set_capability_proof_gate_for_epoch(
                                    proof_epoch,
                                    CapabilityProofGate::Pending,
                                    Some(detail.clone()),
                                ) {
                                    return;
                                }
                                if !shared.capability_proof_epoch_current(proof_epoch) {
                                    return;
                                }
                                let retry_event = format!(
                                    "{}_capability_proof status=retry attempt={attempt}/{} backoff_ms={} error={detail:?}",
                                    harness_binary,
                                    policy.max_attempts,
                                    backoff.as_millis()
                                );
                                log_event(&mut session_log, &retry_event);
                                surface_managed_capability_proof_status(
                                    &shared,
                                    &harness_binary,
                                    &retry_event,
                                );
                                if !sleep_with_stop(&shared.stop_requested, backoff) {
                                    // Supervisor is stopping; abandon the retry
                                    // loop without wedging the gate to `Failed`.
                                    return;
                                }
                                continue;
                            }
                            crate::agent::ProofRetryDecision::GiveUp => {
                                if !shared.set_capability_proof_gate_for_epoch(
                                    proof_epoch,
                                    CapabilityProofGate::Failed,
                                    Some(detail.clone()),
                                ) {
                                    return;
                                }
                                if !shared.capability_proof_epoch_current(proof_epoch) {
                                    return;
                                }
                                shared.transition_actor_state(
                                    crate::session_actor::ActorState::Blocked,
                                    "supervisor",
                                    &format!("{}_capability_proof_failed", harness_binary),
                                );
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "{}_capability_proof status=failed attempts={attempt} error={detail:?}",
                                        harness_binary
                                    ),
                                );
                                surface_managed_capability_proof_status(
                                    &shared,
                                    &harness_binary,
                                    &format!(
                                        "{}_capability_proof status=failed attempts={attempt} error={detail:?}",
                                        harness_binary
                                    ),
                                );
                                // `#tsiftmdcrash` — do NOT kill the live hosted child on
                                // capability-proof give-up. The `Failed` gate already blocks
                                // all prompt dispatch (`capability_dispatch_blocker`), so a
                                // SIGTERM here only destroys a healthy interactive harness the
                                // operator is actively using — a false-positive background
                                // network probe (e.g. a flaky `opencode run` proof child that
                                // timed out at 45s) would yank the live session out from under
                                // the operator and read as a "crash that killed the session
                                // while the tmux pane stayed alive". Leave the child running:
                                // dispatch stays gated + the actor is Blocked, so no unsafe
                                // work is auto-dispatched, and the operator can fix the
                                // environment / stop / restart to re-prove.
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "{}_capability_proof_live_child_preserved reason=dispatch_gated_not_killed",
                                        harness_binary
                                    ),
                                );
                                return;
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn capability proof thread")
}

fn managed_capability_proof_status_message(harness_binary: &str, event: &str) -> String {
    format!("[start] managed {harness_binary} capability proof: {event}")
}

fn display_managed_capability_proof_status(
    tmux: &sessions::Tmux,
    pane_id: &str,
    harness_binary: &str,
    event: &str,
) -> Result<()> {
    let message = managed_capability_proof_status_message(harness_binary, event);
    tmux.raw_cmd(&["display-message", "-t", pane_id, "-d", "5000", &message])?;
    Ok(())
}

fn surface_managed_capability_proof_status(
    shared: &SupervisorShared,
    harness_binary: &str,
    event: &str,
) {
    let message = managed_capability_proof_status_message(harness_binary, event);
    let Some(pane_id) = shared.inject_pane.as_deref() else {
        eprintln!("{message}");
        return;
    };
    let tmux = sessions::Tmux::default_server();
    if let Err(err) = display_managed_capability_proof_status(&tmux, pane_id, harness_binary, event)
    {
        eprintln!(
            "[start] warning: failed to surface managed {} capability proof in tmux status for pane {}: {}",
            harness_binary, pane_id, err
        );
    }
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
pub(crate) struct SupervisorShared {
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
    /// Harness binary for harness-specific tmux submit behavior.
    harness_binary: String,
    /// Writer handle for IPC `inject`. Replaced on each spawn, cleared between restarts.
    inject_writer: SharedWriter,
    /// Claimed tmux pane that should receive supervisor-owned injected input.
    inject_pane: Option<String>,
    /// Filtered output emitted by the current child process.
    recent_output: Mutex<Vec<u8>>,
    /// Alacritty-backed visible screen for the current child process.
    terminal_screen: Mutex<crate::supervisor::screen::OwnedPtyScreen>,
    /// Child PID for IPC `pid` queries and `kill` on restart/stop.
    child_pid: AtomicU32,
    /// `#ctlrecycle` R3 — a dup of the live pty master fd for the current child, so
    /// the idle-watch thread can hand it (CLOEXEC cleared) to a self-`execve` that
    /// preserves the child. `-1` when no child is running. Owned: replaced (old fd
    /// closed) on each spawn/adopt.
    master_fd: AtomicI32,
    /// Flag: IPC requested a restart.
    restart_requested: AtomicBool,
    /// `#supkill-bg` — flag: the pending restart should be served by an in-place
    /// `execve` re-exec at the next turn boundary (drain-and-supersede onto the fresh
    /// binary), NOT by the immediate kill-child → relaunch path. Stamped by the IPC
    /// `Restart` handler from `binary_stale` so the idle-watch reexec branch owns the
    /// upgrade and the in-process host loop defers its restart-kill.
    restart_reexec: AtomicBool,
    /// `#supkill-bg` — the idle-watch's latest staleness probe for this supervisor's
    /// launch binary (`process_binary_is_stale`), refreshed each idle tick so the IPC
    /// `Restart` handler can decide reexec-vs-relaunch without recomputing it.
    binary_stale: AtomicBool,
    /// Flag: IPC requested a stop.
    stop_requested: AtomicBool,
    /// Flag: IPC requested a "Stop Agent" — kill the harness child but keep the
    /// supervisor alive at the restart-or-quit keepalive prompt (never exit, never
    /// auto-restart). Distinct from `stop_requested` (which exits the supervisor).
    stop_agent_requested: AtomicBool,
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
    capability_proof_epoch: AtomicU64,
    capability_proof_error: Mutex<Option<String>>,
}

impl SupervisorShared {
    #[cfg(test)]
    fn new(cwd_source: &'static str, supervisor_instance_id: String) -> Self {
        Self::with_actor_runtime(
            cwd_source,
            supervisor_instance_id,
            "claude",
            None,
            None,
            None,
        )
    }

    fn with_actor_runtime(
        cwd_source: &'static str,
        supervisor_instance_id: String,
        harness_binary: &str,
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
            harness_binary: harness_binary.to_string(),
            inject_writer: Mutex::new(None),
            inject_pane,
            recent_output: Mutex::new(Vec::new()),
            terminal_screen: Mutex::new(crate::supervisor::screen::OwnedPtyScreen::default()),
            child_pid: AtomicU32::new(0),
            master_fd: AtomicI32::new(-1),
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            binary_stale: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            stop_agent_requested: AtomicBool::new(false),
            restart_mode: Mutex::new("continue".to_string()),
            ctrl_d_forwarded: AtomicBool::new(false),
            ctrl_c_forwarded: AtomicBool::new(false),
            auto_trigger_outcome: AtomicU8::new(AutoTriggerOutcome::NotNeeded as u8),
            prompt_visible_once: AtomicBool::new(false),
            suppress_stale_ctrl_d_until_prompt: AtomicBool::new(false),
            capability_proof_gate: AtomicU8::new(CapabilityProofGate::NotRequired as u8),
            capability_proof_epoch: AtomicU64::new(0),
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

    fn next_capability_proof_epoch(&self) -> u64 {
        self.capability_proof_epoch
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    fn capability_proof_epoch_current(&self, epoch: u64) -> bool {
        self.capability_proof_epoch.load(Ordering::Relaxed) == epoch
    }

    fn set_capability_proof_gate_for_epoch(
        &self,
        epoch: u64,
        gate: CapabilityProofGate,
        error: Option<String>,
    ) -> bool {
        if !self.capability_proof_epoch_current(epoch) {
            return false;
        }
        self.set_capability_proof_gate(gate, error);
        true
    }

    fn capability_dispatch_blocker(&self) -> Option<String> {
        match self.capability_proof_gate() {
            // `#capproofbg`: a *pending* managed-capability proof no longer blocks
            // dispatch. Dispatch proceeds immediately while the proof runs in the
            // background; a later proof FAILURE flips the gate to `Failed` and is
            // surfaced asynchronously (session log + tmux `display-message`) instead
            // of stalling every dispatch until the probe completes. Only a proven
            // failure gates subsequent dispatch.
            CapabilityProofGate::NotRequired
            | CapabilityProofGate::Proven
            | CapabilityProofGate::Pending => None,
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

/// Whether an IPC method is a prompt dispatch that must pass the managed
/// capability proof gate before delivery. Only [`IpcMethod::Inject`] is a real
/// dispatch; operator control methods ([`IpcMethod::Clear`], `Stop`, `Restart`)
/// and read-only methods (`State`, `Pid`) are gate-exempt so a session whose
/// proof failed can still be inspected, cleared, and stopped without `kill -9`.
/// Pure and deterministic for unit testing the gate-exemption classification.
mod supervisor_io;
pub(crate) use supervisor_io::*;

mod run;
pub use run::*;

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
pub fn relocate_if_wrong_session(
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
    match project_config_io::update_project_tmux_session(&actual_session) {
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
mod th {
    use super::*;
    use crate::config::Config;
    use crate::sessions::IsolatedTmux;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    pub(crate) struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }
    impl ScopedCurrentDir {
        pub(crate) fn set(path: &std::path::Path) -> Self {
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
    pub(crate) struct ScopedEnvVar {
        key: &'static str,
        previous: Option<String>,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }
    impl ScopedEnvVar {
        pub(crate) fn set(key: &'static str, value: String) -> Self {
            let env_guard = crate::test_support::env_lock();
            let previous = std::env::var(key).ok();
            unsafe { std::env::set_var(key, &value) };
            Self {
                key,
                previous,
                _env_guard: env_guard,
            }
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
    pub(crate) fn tmux_env_for_server(iso: &IsolatedTmux) -> String {
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
    // --- model injection from frontmatter tests ---
    /// Helper: simulates the base_args construction logic from run() for testing
    /// model injection without spawning a real process.
    pub(crate) fn build_base_args_for_test(
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
            let harness_key = agent_doc_model_tier::harness_key_for_agent_name(&harness.binary);
            if let Some(model) = fm.resolve_harness_model(&harness_key) {
                base_args.push("--model".into());
                base_args.push(agent_doc_model_tier::canonical_model_name(
                    model,
                    &harness_key,
                    &cfg.model,
                ));
            }
        }
        base_args
    }
    // --- relocate_if_wrong_session tests ---
    pub(crate) fn test_cycle(
        id: &str,
        phase: agent_doc_turn::CyclePhase,
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
            baseline_file: None,
            prompt_targets: Vec::new(),
            queue_task_id: None,
            turn_id: None,
            recycle_resume_consumed: false,
            pending_done_ids: Vec::new(),
            pending_kept_open_ids: Vec::new(),
            reaped_pending_ids: Vec::new(),
            expect_done_or_gate_ids: Vec::new(),
            pending_gated_ids: Vec::new(),
            pending_added_this_cycle: false,
            pending_added_ids: Vec::new(),
            ipc_snapshot_adoption_blocked: false,
            dropped_exchange_prompts: Vec::new(),
            dropped_queue_prompts: Vec::new(),
            active_queue_heads: Vec::new(),
            active_free_text_queue_heads: Vec::new(),
            pending_semantic_merge_acks: Vec::new(),
            blocked_closeout: None,
        }
    }
    pub(crate) fn committed_state_for_doc(
        path: &Path,
        content: &str,
    ) -> crate::cycle_state::CycleState {
        let mut state = test_cycle("cycle-2", agent_doc_turn::CyclePhase::Committed, 20);
        state.file = path.display().to_string();
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state
    }
    // #jb-run-agent-doc-busy-queue-dispatch-deadlock: the supervisor idle-queue
    // watch must drain a live active-queue head on the busy→idle transition,
    // never inject mid-turn, and never hot-loop on a stuck head.
    #[derive(Clone)]
    pub(crate) struct RecordingWriter(pub(crate) Arc<Mutex<Vec<u8>>>);

    impl Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    pub(crate) struct FailingWriter;

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
    // --- StopSignal + writer thread tests ---
}
#[cfg(test)]
pub(crate) use th::{
    FailingWriter, RecordingWriter, ScopedCurrentDir, ScopedEnvVar, build_base_args_for_test,
    committed_state_for_doc, test_cycle, tmux_env_for_server,
};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::config::Config;
    use crate::hooks::fire_doc_hooks;
    use crate::project_config_io as project_config;
    use crate::sessions::IsolatedTmux;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use std::collections::HashMap;
    use tempfile::TempDir;
    #[cfg(unix)]
    #[test]
    fn reexec_candidates_prefer_fresh_then_current_exe_then_path() {
        let fresh = PathBuf::from("/fresh/agent-doc");
        let current = PathBuf::from("/proc/self/exe-current");
        let candidates = build_reexec_candidates(Some(fresh.clone()), Some(current.clone()), true);
        let paths: Vec<_> = candidates.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(
            paths,
            vec![fresh, current, PathBuf::from("agent-doc")],
            "ordered: resolved fresh, launchable current_exe, then PATH fallback"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reexec_candidates_drop_deleted_current_exe_but_keep_path_fallback() {
        // The Linux post-`cargo install` shape: `current_exe()` is a `(deleted)` inode
        // (not launchable) and the fresh resolver succeeded. The deleted path must not
        // appear; the PATH fallback always does so the ladder is never empty.
        let fresh = PathBuf::from("/home/u/.cargo/bin/agent-doc");
        let deleted = PathBuf::from("/home/u/.cargo/bin/agent-doc (deleted)");
        let candidates = build_reexec_candidates(Some(fresh.clone()), Some(deleted.clone()), false);
        let notes: Vec<_> = candidates.iter().map(|(_, n)| *n).collect();
        assert_eq!(
            notes,
            vec!["resolved_fresh_binary", "path_lookup_agent_doc"]
        );
        assert!(
            !candidates.iter().any(|(p, _)| p == &deleted),
            "a non-launchable (deleted) current_exe must be excluded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reexec_candidates_dedup_and_never_empty() {
        // When the resolver and current_exe both point at the same path, it appears
        // once; with neither resolvable the PATH fallback alone keeps the list usable.
        let same = PathBuf::from("/usr/local/bin/agent-doc");
        let deduped = build_reexec_candidates(Some(same.clone()), Some(same.clone()), true);
        assert_eq!(
            deduped,
            vec![
                (same, "resolved_fresh_binary"),
                (PathBuf::from("agent-doc"), "path_lookup_agent_doc"),
            ]
        );

        let empty = build_reexec_candidates(None, None, false);
        assert_eq!(
            empty,
            vec![(PathBuf::from("agent-doc"), "path_lookup_agent_doc")],
            "ladder always ends with a PATH fallback so reexec can still try"
        );
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
    #[test]
    fn model_injected_from_claude_model_frontmatter() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            claude_model: Some("opus".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let args = build_base_args_for_test(&fm, &harness);
        assert!(args.contains(&"--model".to_string()));
        // The `opus` alias is deferred — agent-doc passes it through so Claude
        // Code resolves its current latest opus (no pinned version).
        assert!(args.contains(&"opus".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("claude-opus")));
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
        assert!(!args.iter().any(|a| a == "opus"));
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
    fn route_owned_start_prompts_instead_of_auto_restarting_codex() {
        assert_eq!(
            clean_exit_resolution_for_start(&crate::harness::HarnessConfig::codex(), true),
            CleanExitResolution::PromptUser,
            "route-owned tmux autostart panes must not immediately restart a cleanly exited child"
        );
    }
    #[test]
    fn route_owned_start_prompts_instead_of_auto_restarting_opencode() {
        assert_eq!(
            clean_exit_resolution_for_start(&crate::harness::HarnessConfig::opencode(), true),
            CleanExitResolution::PromptUser,
            "route-owned tmux autostart panes must not immediately restart a cleanly exited child"
        );
    }
    #[test]
    fn non_route_owned_start_preserves_codex_auto_resume_policy() {
        assert_eq!(
            clean_exit_resolution_for_start(&crate::harness::HarnessConfig::codex(), false),
            CleanExitResolution::RestartContinue
        );
    }
    #[test]
    fn route_owned_cycle_completion_ignores_unchanged_committed_baseline() {
        let baseline = test_cycle("cycle-1", agent_doc_turn::CyclePhase::Committed, 10);
        let current = baseline.clone();

        assert!(
            !route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "a stale committed cycle from before route-owned start must not reap the pane"
        );
    }
    #[test]
    fn route_owned_cycle_completion_detects_new_committed_cycle() {
        let baseline = test_cycle("cycle-1", agent_doc_turn::CyclePhase::Committed, 10);
        let current = test_cycle("cycle-2", agent_doc_turn::CyclePhase::Committed, 20);

        assert!(
            route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "a newer committed cycle should stop and reap a route-owned pane"
        );
    }
    #[test]
    fn route_owned_cycle_completion_waits_while_new_cycle_open() {
        let baseline = test_cycle("cycle-1", agent_doc_turn::CyclePhase::Committed, 10);
        let current = test_cycle("cycle-2", agent_doc_turn::CyclePhase::WriteApplied, 20);

        assert!(
            !route_owned_cycle_completed_after_start(&current, Some(&baseline)),
            "route-owned panes should stay alive for debugging until the new cycle commits"
        );
    }
    #[test]
    fn route_owned_reap_policy_auto_keeps_live_backlog_alive() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let content = "\
<!-- agent:exchange -->
### Re: prior — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#next] Continue the session
<!-- /agent:backlog -->
";
        std::fs::write(&file, content).unwrap();
        let state = committed_state_for_doc(&file, content);

        let decision = route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::Auto);

        assert!(!decision.reap);
        assert_eq!(decision.reason, "backlog_non_empty");
    }
    #[test]
    fn route_owned_reap_policy_auto_keeps_queue_alive() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let content = "\
<!-- agent:queue -->
- do #next
<!-- /agent:queue -->
";
        std::fs::write(&file, content).unwrap();
        let state = committed_state_for_doc(&file, content);

        let decision = route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::Auto);

        assert!(!decision.reap);
        assert_eq!(decision.reason, "queue_non_empty");
    }
    #[test]
    fn route_owned_reap_policy_auto_names_post_commit_user_follow_up() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\nnew prompt?\n");
        std::fs::write(&file, edited).unwrap();
        let state = committed_state_for_doc(&file, committed);

        let decision = route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::Auto);

        assert!(!decision.reap);
        assert_eq!(decision.reason, "post_commit_user_follow_up");
    }
    #[test]
    fn route_owned_reap_policy_auto_keeps_non_prompt_dirty_doc_alive() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\n<!-- local note -->\n");
        std::fs::write(&file, edited).unwrap();
        let state = committed_state_for_doc(&file, committed);

        let decision = route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::Auto);

        assert!(!decision.reap);
        assert_eq!(decision.reason, "document_dirty_after_commit");
    }
    #[test]
    fn route_owned_reap_policy_auto_reaps_without_liveness_signals() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let content = "\
<!-- agent:exchange -->
### Re: done — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        std::fs::write(&file, content).unwrap();
        let state = committed_state_for_doc(&file, content);

        let decision = route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::Auto);

        assert!(decision.reap);
        assert_eq!(decision.reason, "no_liveness_signals");
    }
    #[test]
    fn route_owned_explicit_reap_overrides_live_backlog() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        let content = "<!-- agent:backlog -->\n- [ ] [#next] Continue\n<!-- /agent:backlog -->\n";
        std::fs::write(&file, content).unwrap();
        let state = committed_state_for_doc(&file, content);

        let decision =
            route_owned_reap_decision(&file, &state, RouteOwnedReapPolicy::ReapAfterCommit);

        assert!(decision.reap);
        assert_eq!(decision.reason, "explicit_reap_after_commit");
    }
    #[test]
    fn route_owned_exchange_tail_prompt_is_live() {
        let body = "\
### Re: done — gpt-5
Done.

do #next
";

        assert!(route_owned_exchange_tail_has_unresolved_prompt(body));
    }
    #[test]
    fn route_owned_exchange_tail_ignores_prompt_text_before_latest_response() {
        let body = "\
### Re: earlier — gpt-5
Do #old after this.

### Re: latest — gpt-5
Done.
";

        assert!(!route_owned_exchange_tail_has_unresolved_prompt(body));
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
    fn idle_queue_turn_active_gate_is_scoped_to_owned_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "doc").unwrap();
        crate::turn_status::write_turn_active_marker(dir.path(), "%other").unwrap();

        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "claude",
            None,
            Some(crate::session_actor::ActorState::Ready),
            Some("%owner".to_string()),
        );
        assert!(!turn_active_for_owned_pane(&doc, &shared));

        crate::turn_status::write_turn_active_marker(dir.path(), "%owner").unwrap();
        assert!(turn_active_for_owned_pane(&doc, &shared));
    }
    #[test]
    fn idle_queue_drain_defers_to_real_lease_file_then_resumes_on_expiry() {
        // End-to-end over the actual lease sidecar the supervisor reads: a fresh
        // `/loop` lease makes the supervisor defer; an expired heartbeat hands the
        // drain back so the supervisor resumes (#kp5z / #qflood).
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        let file = doc.to_string_lossy().to_string();

        crate::drain_owner::refresh_drain_owner_lease(
            &file,
            crate::drain_owner::DRAIN_OWNER_CLAUDE_LOOP,
        )
        .unwrap();

        // Fresh lease: the supervisor (idle, fresh head) must defer.
        let now = current_epoch_secs();
        let fresh = crate::drain_owner::fresh_loop_drain_owner_lease(&file, now);
        assert!(fresh.is_some(), "just-claimed lease must read fresh");
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                fresh.is_some(),
                false,
                Some("do [#a]"),
                None
            ),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );

        // Expired heartbeat (far past the TTL): ownership returns to the supervisor.
        let expired = crate::drain_owner::fresh_loop_drain_owner_lease(&file, now + 100_000);
        assert!(
            expired.is_none(),
            "an expired heartbeat must not read fresh"
        );
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                expired.is_some(),
                false,
                Some("do [#a]"),
                None
            ),
            IdleQueueDrainDecision::Dispatch
        );
    }
    #[test]
    fn idle_queue_submit_mode_uses_enter_for_codex_owner_pane() {
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "codex",
            None,
            Some(crate::session_actor::ActorState::Ready),
            Some("%owner".to_string()),
        );

        assert_eq!(
            idle_queue_submit_mode(&shared, &crate::harness::HarnessConfig::codex()),
            "tmux_text_enter"
        );
    }
    #[test]
    fn idle_queue_submit_mode_uses_pty_cr_without_owner_pane() {
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "codex",
            None,
            Some(crate::session_actor::ActorState::Ready),
            None,
        );

        assert_eq!(
            idle_queue_submit_mode(&shared, &crate::harness::HarnessConfig::codex()),
            "pty_cr"
        );
    }
    #[test]
    fn idle_queue_drain_payload_keeps_trigger_for_non_codex_harnesses() {
        assert_eq!(
            idle_queue_drain_payload(
                "tasks/sampleorders.md",
                &crate::harness::HarnessConfig::claude(),
                "ignored",
            ),
            "/agent-doc tasks/sampleorders.md"
        );
        assert_eq!(
            idle_queue_drain_payload(
                "tasks/sampleorders.md",
                &crate::harness::HarnessConfig::opencode(),
                "ignored",
            ),
            "/agent-doc tasks/sampleorders.md"
        );
        assert_eq!(
            idle_queue_drain_payload_kind(&crate::harness::HarnessConfig::claude(), "ignored"),
            "trigger"
        );
    }
    #[test]
    fn idle_queue_restart_drain_does_not_clear_ordinary_sampleorders_head() {
        let harness = crate::harness::HarnessConfig::codex();
        let head = "JB Run Agent Doc on sampleorders.md stalled after a restart with /clear.";

        assert!(!clean_session_head_forces_context_reset(false, false,));
        assert_eq!(
            idle_queue_context_reset_decision(true, false, false, Some(head), None, false),
            IdleQueueContextResetDecision::SkipNoResetNeeded
        );
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, Some(head), None),
            IdleQueueDrainDecision::Dispatch
        );
        assert_eq!(
            idle_queue_drain_payload("tasks/sampleorders.md", &harness, head),
            "agent-doc tasks/sampleorders.md"
        );
        assert_eq!(idle_queue_drain_payload_kind(&harness, head), "trigger");
    }
    #[test]
    fn idle_queue_drain_payload_submits_literal_clear_command() {
        for harness in [
            crate::harness::HarnessConfig::claude(),
            crate::harness::HarnessConfig::codex(),
            crate::harness::HarnessConfig::opencode(),
        ] {
            assert_eq!(
                idle_queue_drain_payload("tasks/sampleorders.md", &harness, "  /clear  "),
                "/clear"
            );
            assert_eq!(
                idle_queue_drain_payload_kind(&harness, "/clear"),
                "slash_command"
            );
        }
    }
    #[test]
    fn idle_queue_drain_payload_submits_any_literal_slash_command() {
        let harness = crate::harness::HarnessConfig::codex();
        assert_eq!(
            idle_queue_drain_payload("tasks/sampleorders.md", &harness, "/model sonnet"),
            "/model sonnet"
        );
        assert_eq!(
            idle_queue_drain_payload_kind(&harness, "/model sonnet"),
            "slash_command"
        );
    }
    #[test]
    fn complete_idle_queue_slash_command_head_consumes_and_commits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("task.md");
        let content = concat!(
            "---\n",
            "session: sid\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue auto -->\n",
            "- /clear\n",
            "- do #next\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test User"],
            vec!["add", "task.md"],
            vec!["commit", "-m", "initial", "--no-verify"],
        ] {
            let status = std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }

        let mut session_log = None;
        assert!(complete_idle_queue_slash_command_head(
            &doc,
            "/clear",
            "/clear",
            &mut session_log
        ));

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("- ~~/clear~~"), "{updated}");
        assert!(updated.contains("- do #next"), "{updated}");
        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["log", "-1", "--pretty=%s"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let subject = String::from_utf8_lossy(&output.stdout);
        assert!(subject.contains("agent-doc"), "{subject}");
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
        assert!(!resume_handoff_failed(
            true,
            false,
            AutoTriggerOutcome::SkippedClearCooldown
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
    fn auto_trigger_clear_cooldown_waits_until_timeout_instead_of_terminal_skip() {
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));

        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(4)),
            AutoTriggerCooldownAction::Wait
        );
        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(5)),
            AutoTriggerCooldownAction::Timeout
        );
        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(10)),
            AutoTriggerCooldownAction::Wait,
            "timeout is reported once; the caller exits after recording it"
        );
    }
    #[test]
    fn auto_trigger_no_prompt_continues_before_deadline_then_fails_closed() {
        // Before the deadline the no-prompt branch keeps polling; once the hard
        // deadline expires it fails closed exactly once so the caller records a
        // startup-miss and returns instead of watching the child forever
        // (`#startupdeadline`).
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));

        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(4)),
            AutoTriggerNoPromptAction::Continue
        );
        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(5)),
            AutoTriggerNoPromptAction::FailClosed
        );
        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(10)),
            AutoTriggerNoPromptAction::Continue,
            "fail-closed fires once; the caller returns after recording the startup-miss"
        );
        assert_eq!(monitor.stop_outcome(), AutoTriggerOutcome::Timeout);
    }
    #[test]
    fn auto_trigger_timeout_exceeds_global_hang_ceiling_for_continue_restart() {
        // `#contrestartdispatch`: the auto-trigger no-prompt deadline must be
        // longer than the 10s `GLOBAL_HANG_CEILING`. A continue-mode
        // `restart-supervisor` relaunches `claude --continue`, which resumes a
        // potentially large prior session (plus SessionStart hooks) before showing
        // its first prompt — routinely > 10s. When this budget was clamped to the
        // ceiling, the auto-trigger always timed out before the prompt appeared,
        // so the `agent-doc <FILE>` re-dispatch never fired and the relaunched
        // operator came up unclaimed (controller stuck at `operator_ready`). Guard
        // that it is generous enough for harness startup yet still bounded (fails
        // closed, never an unbounded hang).
        assert!(
            AUTO_TRIGGER_TIMEOUT > agent_doc_turn::wait_machine::GLOBAL_HANG_CEILING,
            "auto-trigger startup budget {:?} must exceed the 10s responsiveness ceiling so a \
             continue-restart harness resume can reach its prompt before the re-dispatch is abandoned",
            AUTO_TRIGGER_TIMEOUT
        );
        // Still bounded: a freshly relaunched harness that never shows a prompt
        // must fail closed at this budget rather than hang forever.
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, AUTO_TRIGGER_TIMEOUT);
        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + AUTO_TRIGGER_TIMEOUT),
            AutoTriggerNoPromptAction::FailClosed
        );
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
    fn failed_capability_proof_gate_blocks_dispatch_so_live_child_need_not_be_killed() {
        // `#tsiftmdcrash` regression guard: the capability-proof give-up path no
        // longer SIGTERMs the live hosted child. That is only safe because the
        // `Failed` gate is itself a complete dispatch block — no prompt can reach
        // the agent while proof failed, so a healthy interactive harness the
        // operator is using can stay alive without auto-dispatching unsafe work.
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        assert!(
            shared.capability_dispatch_blocker().is_none(),
            "NotRequired gate must not block dispatch"
        );
        shared.set_capability_proof_gate(CapabilityProofGate::Proven, None);
        assert!(
            shared.capability_dispatch_blocker().is_none(),
            "Proven gate must not block dispatch"
        );
        // `#capproofbg`: a *pending* proof is non-blocking — dispatch proceeds
        // immediately while the proof runs in the background.
        shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
        assert!(
            shared.capability_dispatch_blocker().is_none(),
            "Pending gate must NOT block dispatch (#capproofbg non-blocking proof)"
        );
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("opencode child network probe timed out after 45s".to_string()),
        );
        let blocker = shared
            .capability_dispatch_blocker()
            .expect("Failed gate must block dispatch");
        assert!(
            blocker.contains("prompt dispatch is disabled"),
            "blocker must state dispatch is disabled: {blocker}"
        );
        assert!(
            blocker.contains("opencode child network probe timed out after 45s"),
            "blocker must carry the proof-failure detail: {blocker}"
        );
    }
    #[test]
    fn capability_proof_epoch_ignores_stale_thread_result() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let stale_epoch = shared.next_capability_proof_epoch();
        assert!(shared.set_capability_proof_gate_for_epoch(
            stale_epoch,
            CapabilityProofGate::Pending,
            None,
        ));

        let current_epoch = shared.next_capability_proof_epoch();
        assert!(shared.set_capability_proof_gate_for_epoch(
            current_epoch,
            CapabilityProofGate::Pending,
            Some("new proof running".to_string()),
        ));
        assert!(!shared.set_capability_proof_gate_for_epoch(
            stale_epoch,
            CapabilityProofGate::Proven,
            None,
        ));
        assert_eq!(shared.capability_proof_gate(), CapabilityProofGate::Pending);
        assert!(shared.set_capability_proof_gate_for_epoch(
            current_epoch,
            CapabilityProofGate::Proven,
            None,
        ));
        assert_eq!(shared.capability_proof_gate(), CapabilityProofGate::Proven);
    }
    #[test]
    fn auto_trigger_clear_command_bypasses_dispatch_gate_and_submits_enter() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let stop = AtomicBool::new(false);

        assert_eq!(
            auto_trigger_submit_queue_command(&shared, &stop, "/clear"),
            AutoTriggerOutcome::Sent
        );
        assert_eq!(written.lock().unwrap().as_slice(), b"/clear\r");
    }
    #[test]
    fn managed_capability_proof_status_message_names_harness() {
        let message = managed_capability_proof_status_message(
            "opencode",
            "opencode_capability_proof status=proven network=proven",
        );

        assert_eq!(
            message,
            "[start] managed opencode capability proof: opencode_capability_proof status=proven network=proven"
        );
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
