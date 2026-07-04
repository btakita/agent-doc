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
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent_doc_frontmatter::frontmatter;
use agent_doc_queue::idle_drain::{
    idle_queue_drain_payload, idle_queue_drain_payload_kind, idle_queue_head_slash_command,
};
#[cfg(test)]
use agent_doc_queue::queue::{
    IdleQueueContextResetDecision, IdleQueueDrainDecision, clean_session_head_forces_context_reset,
    idle_queue_context_reset_decision, idle_queue_drain_decision,
};
use agent_doc_queue_io::queue_consume;
use agent_doc_supervisor::auto_trigger::{
    AutoTriggerCooldownAction, AutoTriggerMonitor, AutoTriggerNoPromptAction, AutoTriggerOutcome,
    AutoTriggerStopOutcome, CapabilityProofGate, auto_trigger_clear_cooldown_action,
    auto_trigger_no_prompt_action,
};
use agent_doc_supervisor::config::AgentLaunchArgsSources;
use agent_doc_supervisor::crash_policy::{
    CrashPolicy, FAILED_RESUME_WINDOW, FailedResumeTracker, RestartAction,
    SupervisorCleanExitResolution, SupervisorPromptDecision, SupervisorRestartContinueExitStrategy,
    SupervisorState, classify_supervisor_prompt_input, format_exit_provenance_fields,
    forwarded_ctrl_c_interrupt_exit, restart_continue_exit_strategy,
    supervisor_clean_exit_before_prompt_seen, supervisor_clean_exit_resolution,
    supervisor_policy_exit_code, supervisor_resume_handoff_failed,
};
use agent_doc_supervisor::input::prompt_input_summary;
use agent_doc_supervisor::ipc_protocol::submit_bytes;
use agent_doc_supervisor::route_owned::RouteOwnedReapPolicy;
use agent_doc_supervisor::session_owner::{
    ExistingPaneConflictFacts, ExistingSessionPaneAction,
    format_existing_pane_conflict_error as format_existing_pane_conflict_error_from_facts,
};
use agent_doc_supervisor_io::cwd;
use agent_doc_supervisor_io::detection::*;
use agent_doc_supervisor_io::ipc::SupervisorIpc;
#[cfg(unix)]
use agent_doc_supervisor_process::ReexecState;
use agent_doc_supervisor_process::{
    in_process::{InProcessSupervisor, PtySupervisedChild, TickOutcome},
    output_state::SupervisorOutputState,
    pty::PtySpawnConfig,
    route_owned_completion::{RouteOwnedCompletionConfig, spawn_route_owned_completion_thread},
    shared_writer::{SharedPtyWriter, StopSignal, lock_writer_interruptibly},
};
use agent_doc_turn_executor::binary::current_agent_doc_binary;
use agent_doc_turn_executor::capability_proof::managed_capability_proof_status_message;

use agent_doc_project_config_io as project_config_io;

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
    format_exit_provenance_fields(&rendered, status.success())
}

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
/// Fail-closed handler for an expired session-startup deadline: record a
/// `startup_miss` marker against the owned pane and surface an actionable
/// "session did not become dispatch-ready in Ns" diagnostic on stderr, so a hung
/// harness child becomes a recoverable, dogfoodable error instead of an
/// indefinite hang (`#startupdeadline`). `reason` is the timeout provenance
/// (`no_prompt`, `capability_proof`, `clear_cooldown`).
fn record_session_startup_miss(
    path: &Path,
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    session_log: &mut Option<std::fs::File>,
    reason: &str,
) {
    let pane = shared.inject_pane.as_deref().unwrap_or("child_pty");
    let session_id = agent_doc_frontmatter_io::session::read_session_id(path).unwrap_or_default();
    let deadline_secs = AUTO_TRIGGER_TIMEOUT.as_secs();
    match agent_doc_supervisor_io::startup_miss::record_startup_miss(
        path,
        pane,
        &session_id,
        &harness.binary,
        agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
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

fn turn_active_for_owned_pane(file: &Path, shared: &SupervisorShared) -> bool {
    let Some(marker) = agent_doc_turn_status_io::read_turn_active_marker_for_file(file) else {
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
    match queue_consume::consume_queue_prompt_force_disk(
        file,
        &crate::write::QUEUE_CONSUME_WRITEBACK_EFFECTS,
    ) {
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

fn log_idle_queue_drain_submit(
    file: &Path,
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    payload_kind: &str,
    active_head: &str,
    drain_payload: &str,
) {
    let target = shared.inject_pane.as_deref().unwrap_or("child_pty");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "idle_queue_watch_drain file={} harness={} payload_kind={} submit_mode={} target={} head_bytes={} head_sha256={} payload_bytes={} proof=go_drain_dispatch",
            file.display(),
            harness.binary,
            payload_kind,
            agent_doc_supervisor::idle_watch::idle_queue_submit_mode(
                shared.inject_pane.is_some(),
                &harness.binary,
            ),
            target,
            active_head.len(),
            agent_doc_hash::content_hash(active_head),
            drain_payload.len(),
        ),
    );
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
        match classify_supervisor_prompt_input(bytes_read, &input) {
            SupervisorPromptDecision::Quit => {
                log_event(session_log, quit_event);
                return PromptOutcome::Quit;
            }
            SupervisorPromptDecision::QuitEof => match eof_policy {
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
            SupervisorPromptDecision::RestartFresh => {
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
            SupervisorPromptDecision::Invalid => {
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

fn route_owned_live_pane_busy_reason(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
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
        .is_some_and(|state| state == agent_doc_sqlite::state_store::ActorState::Ready)
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

impl agent_doc_supervisor_process::route_owned_completion::RouteOwnedCompletionState
    for SupervisorShared
{
    fn actor_ready(&self) -> bool {
        actor_state_is_ready(self)
    }

    fn ready_busy_blocker_reason(
        &self,
        harness: &agent_doc_harness::HarnessConfig,
    ) -> Option<String> {
        ready_busy_blocker_reason(self, harness)
    }

    fn live_pane_busy_reason(&self, harness: &agent_doc_harness::HarnessConfig) -> Option<String> {
        route_owned_live_pane_busy_reason(self, harness)
    }

    fn owned_pane_label(&self) -> String {
        owned_pane_label(self).to_string()
    }

    fn request_child_stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        self.kill_child();
    }
}

fn is_forwarded_ctrl_c_interrupt_exit(
    status: &portable_pty::ExitStatus,
    ctrl_c_forwarded: bool,
) -> bool {
    let rendered = status.to_string();
    forwarded_ctrl_c_interrupt_exit(&rendered, status.exit_code(), ctrl_c_forwarded)
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
        agent_doc_sqlite::state_store::ActorState::Busy,
        "dispatch",
        "auto_trigger_inject",
    );
    let submitted_text =
        agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(trigger_cmd)
            .to_string();
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        let profile =
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&shared.harness_binary);
        agent_doc_tmux_io::input_diag::log_text_submit(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
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

    let payload = submit_bytes(&submitted_text).into_bytes();
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
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
        agent_doc_sqlite::state_store::ActorState::Busy,
        "operator",
        "auto_trigger_clear",
    );
    let submitted_text =
        agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(clear_cmd)
            .to_string();
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        let profile =
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&shared.harness_binary);
        agent_doc_tmux_io::input_diag::log_text_submit(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
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

    let payload = submit_bytes(&submitted_text).into_bytes();
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
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

fn dispatch_submit_text_to_tmux(
    tmux: &tmux_router::Tmux,
    pane: &str,
    text: &str,
    harness: &str,
) -> Result<()> {
    agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        tmux,
        pane,
        text,
        harness,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    )
    .with_context(|| format!("failed to inject submitted input into pane {}", pane))
}

fn dispatch_submit_text_to_pane(pane: &str, text: &str, harness: &str) -> Result<()> {
    let tmux = tmux_router::Tmux::default_server();
    dispatch_submit_text_to_tmux(&tmux, pane, text, harness)
}

fn spawn_auto_trigger_thread(
    shared: Arc<SupervisorShared>,
    stop: Arc<AtomicBool>,
    file: String,
    harness: agent_doc_harness::HarnessConfig,
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
                    let outcome = match monitor.stop_outcome() {
                        AutoTriggerStopOutcome::Cancelled => AutoTriggerOutcome::Cancelled,
                        AutoTriggerStopOutcome::Timeout => AutoTriggerOutcome::Timeout,
                    };
                    shared
                        .auto_trigger_outcome
                        .store(outcome as u8, Ordering::Relaxed);
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
                            if agent_doc_tmux_commands::input_diag::verbose_enabled() {
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
                            if agent_doc_tmux_commands::input_diag::verbose_enabled() {
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
    harness: &agent_doc_harness::HarnessConfig,
    source: &str,
    session_log: &mut Option<std::fs::File>,
    logged: &mut bool,
) -> bool {
    match agent_doc_queue_io::continuation_marker::clear_cooldown_active(path) {
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

#[cfg(unix)]
fn supervisor_reexec_candidates() -> Vec<(PathBuf, &'static str)> {
    // Gather the effectful environment facts here, then hand candidate ordering
    // to `agent-doc-supervisor`.
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
    let resolved_fresh = current_agent_doc_binary().ok();
    agent_doc_supervisor::reexec::build_reexec_candidates(
        resolved_fresh,
        current_exe,
        current_exe_launchable,
    )
}

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
    global_config: agent_doc_config::Config,
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
    let policy = agent_doc_turn_executor::capability_proof::resolve_managed_proof_policy(
        agent_doc_turn_executor::capability_proof::ManagedProofPolicyInputs {
            frontmatter_max_attempts: frontmatter.managed_proof_max_attempts,
            config_max_attempts: global_config.managed_proof_max_attempts,
            frontmatter_retry_backoff_secs: frontmatter.managed_proof_retry_backoff_secs,
            config_retry_backoff_secs: global_config.managed_proof_retry_backoff_secs,
            frontmatter_probe_timeout_secs: frontmatter.managed_proof_probe_timeout_secs,
            config_probe_timeout_secs: global_config.managed_proof_probe_timeout_secs,
        },
    );
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
                match agent_doc_agent_io::agent::codex::prove_managed_session_capabilities(
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
                        match agent_doc_turn_executor::capability_proof::proof_retry_decision(
                            attempt,
                            policy.max_attempts,
                            policy.base_backoff,
                        ) {
                            agent_doc_turn_executor::capability_proof::ProofRetryDecision::Retry {
                                backoff,
                            } => {
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
                            agent_doc_turn_executor::capability_proof::ProofRetryDecision::GiveUp => {
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
                                    agent_doc_sqlite::state_store::ActorState::Blocked,
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

fn display_managed_capability_proof_status(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    harness_binary: &str,
    event: &str,
) -> Result<()> {
    let message = managed_capability_proof_status_message(harness_binary, event);
    agent_doc_tmux_io::show_message(tmux, pane_id, "5000", &message)?;
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
    let tmux = tmux_router::Tmux::default_server();
    if let Err(err) = display_managed_capability_proof_status(&tmux, pane_id, harness_binary, event)
    {
        eprintln!(
            "[start] warning: failed to surface managed {} capability proof in tmux status for pane {}: {}",
            harness_binary, pane_id, err
        );
    }
}

fn existing_session_pane_action(
    tmux: &tmux_router::Tmux,
    session_id: &str,
    file: &Path,
    current_pane: &str,
) -> Result<Option<ExistingSessionPaneAction>> {
    let entry = agent_doc_session_registry_io::lookup_entry(session_id)?;
    let live_owner = agent_doc_sync_io::sync::find_normal_path_owner_pane_excluding_quiet(
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
    tmux: &tmux_router::Tmux,
    current_pane: &str,
    entry: Option<&tmux_router::RegistryEntry>,
    live_owner: Option<&str>,
) -> Option<ExistingSessionPaneAction> {
    let registry_pane = entry.map(|entry| entry.pane.as_str());
    let registry_pane_alive = registry_pane
        .map(|pane| tmux.pane_alive(pane))
        .unwrap_or(false);
    agent_doc_supervisor::session_owner::existing_session_pane_action(
        current_pane,
        registry_pane,
        registry_pane_alive,
        live_owner,
    )
}

fn format_existing_pane_conflict_error(
    tmux: &tmux_router::Tmux,
    file: &Path,
    current_pane: &str,
    conflicting_pane: &str,
) -> String {
    let conflict_session = tmux.pane_session(conflicting_pane).unwrap_or_default();
    let conflict_window =
        agent_doc_tmux_io::target_window_id(tmux, conflicting_pane).unwrap_or_default();
    let current_session = tmux.pane_session(current_pane).unwrap_or_default();
    let current_window =
        agent_doc_tmux_io::target_window_id(tmux, current_pane).unwrap_or_default();
    let document = file.display().to_string();
    format_existing_pane_conflict_error_from_facts(&ExistingPaneConflictFacts {
        document: &document,
        current_pane,
        conflicting_pane,
        conflict_session: &conflict_session,
        conflict_window: &conflict_window,
        current_session: &current_session,
        current_window: &current_window,
    })
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
        state: agent_doc_sqlite::state_store::ActorState,
        caller: &str,
        reason: &str,
    ) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
        agent_doc_controller_io::project_controller::mark_lifecycle(
            &self.project_root,
            agent_doc_controller_io::project_controller::LifecycleRequest {
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
    actor_state: Mutex<Option<agent_doc_sqlite::state_store::ActorState>>,
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
    /// Filtered output and visible terminal projection for the current child process.
    output: SupervisorOutputState,
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
        actor_state: Option<agent_doc_sqlite::state_store::ActorState>,
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
            output: SupervisorOutputState::default(),
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
        state: agent_doc_sqlite::state_store::ActorState,
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

impl agent_doc_supervisor_io::detection::SupervisorDetectionState for SupervisorShared {
    fn record_recent_output(&self, bytes: &[u8]) {
        self.output.record_recent_output(bytes);
    }

    fn record_terminal_screen(&self, bytes: &[u8]) {
        self.output.record_terminal_screen(bytes);
    }

    fn reset_terminal_screen(&self, size: PtySize) {
        self.output.reset_terminal_screen(size);
    }

    fn child_output_for_detection(&self) -> String {
        self.output.child_output_for_detection()
    }

    fn mark_prompt_visible_once(&self) -> bool {
        !self.prompt_visible_once.swap(true, Ordering::Relaxed)
    }

    fn actor_known_non_ready(&self) -> bool {
        self.actor_state
            .lock()
            .unwrap()
            .is_some_and(|state| state != agent_doc_sqlite::state_store::ActorState::Ready)
    }

    fn actor_ready(&self) -> bool {
        self.actor_state
            .lock()
            .unwrap()
            .is_some_and(|state| state == agent_doc_sqlite::state_store::ActorState::Ready)
    }

    fn actor_busy_or_starting(&self) -> bool {
        self.actor_state.lock().unwrap().is_some_and(|state| {
            matches!(
                state,
                agent_doc_sqlite::state_store::ActorState::Busy
                    | agent_doc_sqlite::state_store::ActorState::Starting
            )
        })
    }

    fn owned_pane_id(&self) -> Option<String> {
        self.inject_pane.clone().or_else(|| {
            self.actor_runtime
                .as_ref()
                .map(|runtime| runtime.pane_id.clone())
        })
    }

    fn with_recent_output<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        self.output.with_recent_output(f)
    }
}

/// Whether an IPC method is a prompt dispatch that must pass the managed
/// capability proof gate before delivery. Only
/// [`agent_doc_supervisor::ipc_protocol::IpcMethod::Inject`] is a real
/// dispatch; operator control methods
/// ([`agent_doc_supervisor::ipc_protocol::IpcMethod::Clear`], `Stop`, `Restart`)
/// and read-only methods (`State`, `Pid`) are gate-exempt so a session whose
/// proof failed can still be inspected, cleared, and stopped without `kill -9`.
/// Pure and deterministic for unit testing the gate-exemption classification.
mod supervisor_io;

mod run;
pub use run::*;

fn agent_launch_args_sources(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
) -> AgentLaunchArgsSources {
    AgentLaunchArgsSources {
        frontmatter_agent_args: fm.agent_args.clone(),
        frontmatter_claude_args: fm.claude_args.clone(),
        frontmatter_codex_args: fm.codex_args.clone(),
        frontmatter_opencode_args: fm.opencode_args.clone(),
        config_agent_args: global_config.agent_args.clone(),
        config_claude_args: global_config.claude_args.clone(),
        config_codex_args: global_config.codex_args.clone(),
        config_opencode_args: global_config.opencode_args.clone(),
        env_claude_args: std::env::var("AGENT_DOC_CLAUDE_ARGS").ok(),
    }
}

/// Auto-relocate `pane_id` to `expected_session` if it is currently in a different session.
/// Returns `true` if relocation succeeded or was unnecessary; `false` if relocation failed.
/// Falls back to warn-only on failure so the start isn't aborted.
pub fn relocate_if_wrong_session(
    tmux: &tmux_router::Tmux,
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
        match tmux_router::PaneMoveOp::new(tmux, pane_id, &anchor)
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
    tmux: &tmux_router::Tmux,
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
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use tmux_router::IsolatedTmux;
    pub(crate) struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: agent_doc_test_support::ProcessGlobalLockGuard,
    }
    impl ScopedCurrentDir {
        pub(crate) fn set(path: &std::path::Path) -> Self {
            let env_guard = agent_doc_test_support::env_lock();
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
        _env_guard: agent_doc_test_support::ProcessGlobalLockGuard,
    }
    impl ScopedEnvVar {
        pub(crate) fn set(key: &'static str, value: String) -> Self {
            let env_guard = agent_doc_test_support::env_lock();
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
        let socket_path =
            agent_doc_tmux_io::socket_path(iso).expect("tmux should report its socket path");
        format!("{socket_path},0,0")
    }
    // --- model injection from frontmatter tests ---
    /// Helper: simulates the base_args construction logic from run() for testing
    /// model injection without spawning a real process.
    pub(crate) fn build_base_args_for_test(
        fm: &Frontmatter,
        harness: &agent_doc_harness::HarnessConfig,
    ) -> Vec<String> {
        let cfg = Config::default();
        let resolved_agent_args = agent_doc_supervisor::config::resolve_agent_launch_args(
            &harness.binary,
            agent_launch_args_sources(fm, &cfg),
        );
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
    ) -> agent_doc_cycle_state_io::CycleState {
        agent_doc_cycle_state_io::CycleState {
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
    test_cycle, tmux_env_for_server,
};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use agent_doc_hooks_io::fire_doc_hooks;
    use agent_doc_project_config_io as project_config_io;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tmux_router::IsolatedTmux;

    #[test]
    fn stale_busy_reconcile_preserves_already_dispatched_head_dedup() {
        let mut idle_busy_ticks = STALE_BUSY_RECONCILE_TICKS;
        let last_dispatched =
            agent_doc_supervisor::idle_reconcile::reconcile_stale_busy_idle_queue_state(
                Some("do [#learn-ohio-duplicate-gate]".to_string()),
                &mut idle_busy_ticks,
            );

        assert_eq!(idle_busy_ticks, 0);
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some("do [#learn-ohio-duplicate-gate]"),
                last_dispatched.as_deref(),
            ),
            IdleQueueDrainDecision::SkipAlreadyDispatched
        );
    }

    #[test]
    fn idle_queue_prompt_visible_trusts_ready_actor_over_stale_renderer_tail() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::claude();
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, b"turn committed, renderer tail has no composer\n");

        assert!(
            !current_child_prompt_visible(&shared, &harness),
            "stale output alone should not prove an idle composer"
        );
        assert!(
            idle_queue_prompt_visible(&shared, &harness),
            "the supervisor's ready actor state should let the idle queue drain"
        );
    }

    #[test]
    fn idle_queue_prompt_visible_keeps_blocker_over_ready_actor() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::claude();
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(
            &shared,
            concat!(
                "✶ Generating… (3s · esc to interrupt)\n",
                "❯\n",
                "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
                "  ⏵⏵ bypass permissions on · 1 shell\n",
            )
            .as_bytes(),
        );

        assert!(
            !idle_queue_prompt_visible(&shared, &harness),
            "active-turn blockers must win over a stale ready actor state"
        );
    }

    #[test]
    fn route_owned_live_pane_busy_requires_idle_prompt_before_reap() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        record_recent_output(&shared, b"exploring repository\n");

        let reason = route_owned_live_pane_busy_reason(&shared, &harness)
            .expect("running child without prompt should block route-owned reap");

        assert!(reason.contains("live_pane_busy_no_idle_prompt"));
        assert!(reason.contains("exploring repository"));
    }

    #[test]
    fn route_owned_live_pane_busy_trusts_ready_actor_over_stale_renderer_tail() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"Working...\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );

        assert_eq!(route_owned_live_pane_busy_reason(&shared, &harness), None);
    }

    #[test]
    fn route_owned_live_pane_busy_keeps_ready_actor_for_blocking_prompt_state() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"tab to queue message\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );

        let reason = route_owned_live_pane_busy_reason(&shared, &harness)
            .expect("queued composer state must still block route-owned reap");

        assert!(reason.contains("live_pane_busy_blocked_prompt"));
        assert!(reason.contains("queued draft in composer"));
    }

    #[test]
    fn ready_busy_blocker_reason_filters_to_recoverable_queue_draft() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"tab to queue message\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 41% used\n".as_bytes(),
        );

        assert_eq!(
            ready_busy_blocker_reason(&shared, &harness).as_deref(),
            Some("queued draft in composer")
        );

        let active_shared = SupervisorShared::new("test", "test-instance".to_string());
        record_recent_output(
            &active_shared,
            "• Working (1m 34s • esc to interrupt)\n\n› Write tests\n".as_bytes(),
        );
        record_recent_output(
            &active_shared,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n".as_bytes(),
        );
        assert_eq!(ready_busy_blocker_reason(&active_shared, &harness), None);
    }

    #[test]
    fn route_owned_live_pane_busy_allows_idle_prompt_reap() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        record_recent_output(&shared, b"done\n");
        record_recent_output(&shared, "›\n".as_bytes());

        assert_eq!(route_owned_live_pane_busy_reason(&shared, &harness), None);
    }

    #[test]
    fn is_help_screen_visible_detects_opencode_help() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::opencode();
        record_recent_output(
            &shared,
            b"opencode [project]           start opencode tui\n",
        );
        record_recent_output(
            &shared,
            b"opencode run [message..]     run opencode with a message\n",
        );
        record_recent_output(
            &shared,
            b"opencode debug               debugging and troubleshooting tools\n",
        );
        assert!(is_help_screen_visible(&shared, &harness));
    }

    #[test]
    fn is_help_screen_visible_rejects_normal_opencode_output() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::opencode();
        record_recent_output(&shared, b"some normal output\n");
        record_recent_output(&shared, b">\n");
        assert!(!is_help_screen_visible(&shared, &harness));
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
        *shared.actor_state.lock().unwrap() = Some(agent_doc_sqlite::state_store::ActorState::Busy);
        assert!(
            prompt_visible_requires_ready_transition(&shared),
            "a busy actor that surfaces the prompt again must return to ready"
        );
    }

    #[test]
    fn model_injected_from_claude_model_frontmatter() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            claude_model: Some("opus".into()),
            ..Default::default()
        };
        let harness = agent_doc_harness::HarnessConfig::claude();
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
        let harness = agent_doc_harness::HarnessConfig::claude();
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
        let harness = agent_doc_harness::HarnessConfig::codex();
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
        let harness = agent_doc_harness::HarnessConfig::opencode();
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
        let harness = agent_doc_harness::HarnessConfig::claude();
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
        let harness = agent_doc_harness::HarnessConfig::claude();
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
    fn route_owned_liveness_file_adapter_maps_read_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = test_cycle("cycle-1", agent_doc_turn::CyclePhase::Committed, 10);
        let facts =
            agent_doc_supervisor_process::route_owned_completion::route_owned_facts_from_cycle_state(
                &state,
            );
        let missing = tmp.path().join("missing.md");

        let reason =
            agent_doc_supervisor_process::route_owned_completion::route_owned_liveness_reason_for_file(
                &missing, &facts,
            )
            .expect("missing file should be an adapter-failure liveness signal");
        assert!(
            reason.as_str().starts_with("read_failed:"),
            "read failure should remain an explicit process adapter concern: {reason}"
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
    fn idle_queue_turn_active_gate_is_scoped_to_owned_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "doc").unwrap();
        agent_doc_turn_status_io::write_turn_active_marker(dir.path(), "%other").unwrap();

        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "claude",
            None,
            Some(agent_doc_sqlite::state_store::ActorState::Ready),
            Some("%owner".to_string()),
        );
        assert!(!turn_active_for_owned_pane(&doc, &shared));

        agent_doc_turn_status_io::write_turn_active_marker(dir.path(), "%owner").unwrap();
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

        agent_doc_queue::drain_owner::refresh_drain_owner_lease(
            &file,
            agent_doc_queue::drain_owner::DRAIN_OWNER_CLAUDE_LOOP,
        )
        .unwrap();

        // Fresh lease: the supervisor (idle, fresh head) must defer.
        let now = current_epoch_secs();
        let fresh = agent_doc_queue::drain_owner::fresh_loop_drain_owner_lease(&file, now);
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
        let expired =
            agent_doc_queue::drain_owner::fresh_loop_drain_owner_lease(&file, now + 100_000);
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
    fn idle_queue_drain_payload_keeps_trigger_for_non_codex_harnesses() {
        let claude = agent_doc_harness::HarnessConfig::claude();
        let opencode = agent_doc_harness::HarnessConfig::opencode();
        assert_eq!(
            idle_queue_drain_payload("ignored", claude.trigger_command("tasks/sampleorders.md"),),
            "/agent-doc tasks/sampleorders.md"
        );
        assert_eq!(
            idle_queue_drain_payload("ignored", opencode.trigger_command("tasks/sampleorders.md"),),
            "/agent-doc tasks/sampleorders.md"
        );
        assert_eq!(idle_queue_drain_payload_kind("ignored"), "trigger");
    }
    #[test]
    fn idle_queue_restart_drain_does_not_clear_ordinary_sampleorders_head() {
        let harness = agent_doc_harness::HarnessConfig::codex();
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
            idle_queue_drain_payload(head, harness.trigger_command("tasks/sampleorders.md")),
            "agent-doc tasks/sampleorders.md"
        );
        assert_eq!(idle_queue_drain_payload_kind(head), "trigger");
    }
    #[test]
    fn idle_queue_drain_payload_submits_literal_clear_command() {
        for harness in [
            agent_doc_harness::HarnessConfig::claude(),
            agent_doc_harness::HarnessConfig::codex(),
            agent_doc_harness::HarnessConfig::opencode(),
        ] {
            assert_eq!(
                idle_queue_drain_payload(
                    "  /clear  ",
                    harness.trigger_command("tasks/sampleorders.md")
                ),
                "/clear"
            );
            assert_eq!(idle_queue_drain_payload_kind("/clear"), "slash_command");
        }
    }
    #[test]
    fn idle_queue_drain_payload_submits_any_literal_slash_command() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        assert_eq!(
            idle_queue_drain_payload(
                "/model sonnet",
                harness.trigger_command("tasks/sampleorders.md")
            ),
            "/model sonnet"
        );
        assert_eq!(
            idle_queue_drain_payload_kind("/model sonnet"),
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
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
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
            agent_doc_harness::HarnessConfig::codex(),
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
}
