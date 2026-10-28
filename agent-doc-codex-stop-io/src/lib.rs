//! # Module: codex_hook
//!
//! ## Spec
//! - Implements the repo-local Codex hook bridge used by `agent-doc` installs.
//! - Codex `UserPromptSubmit` is handled by `agent-doc-codex-hook-io`; this
//!   module consumes the resulting tracked session state during `Stop`.
//! - `handle_stop()` reads the Codex `Stop` JSON payload from stdin, checks the
//!   tracked document with `session_check::inspect()`, and only intervenes when
//!   the cycle is still open.
//! - On the first intercepted stop, the hook first tries to finish the response
//!   cycle deterministically: validate `last_assistant_message`, replay only a
//!   single-response closeout through `repair`, and run the normal
//!   `git::commit()` boundary.
//! - The same closeout path also self-heals a missed startup when the document
//!   still has unresolved prompt-bearing user edits but no new cycle ever
//!   started, instead of letting Codex exit with an external-only answer.
//! - If another durable keyed owner already holds the exact captured closeout,
//!   the hook follows the controller's reactive cycle projection before it
//!   decides whether recovery is still blocked. A lease is not itself a user-
//!   visible blocker when that owner reaches the terminal state.
//! - If the cycle still cannot be closed automatically, the hook falls back to
//!   a same-turn continuation with instructions to finish recovery/persistence.
//! - If Codex reaches a second `Stop` for the same still-open cycle
//!   (`stop_hook_active = true`), fail closed with `continue=false` instead of
//!   looping forever.
//!
//! ## Agentic Contracts
//! - Hook handling is deterministic and binary-owned; generated Codex hook files
//!   should only shell out to these commands.
//! - Missing project roots, unmatched prompts, or stale session state are all
//!   treated as no-ops.
//! - Hook state is scoped by Codex `session_id`, not globally across documents.
//!
//! ## Evals
//! - `stop_auto_closes_open_cycle_from_last_assistant_message`
//! - `stop_blocks_transcript_shaped_last_assistant_message`
//! - `stop_passes_through_committed_cycle`
//! - `stop_blocks_open_cycle_without_recoverable_response`
//! - `stop_fails_closed_after_one_auto_continue`

use agent_doc_codex_hook_io::{
    SessionState, clear_state_across_roots, load_state_any_with_timeout, parked_session_state,
    project_roots_for, prompt_writeback_debt, save_state_across_roots, tracking_roots,
};
#[cfg(test)]
use agent_doc_codex_hook_io::{
    UserPromptSubmitInput, apply_user_prompt_submit, load_state, save_state,
};
use agent_doc_queue_io::queue_consume;
use agent_doc_turn::codex_stop_continuation::{
    render_prompt_continuation_instruction, render_slash_command_continuation_instruction,
};
use agent_doc_turn::response_text::{
    first_nonempty_prompt_line, is_committed_prompt_diff_interruption,
    prompt_target_from_interruption_reason,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(test)]
use agent_doc_codex_hook_io::project_root_for;

#[derive(Debug, Clone, Deserialize)]
struct StopInput {
    session_id: String,
    turn_id: String,
    cwd: String,
    /// `#stopnullmessage`: Codex sends `null` here when the turn ended on a
    /// tool call with no final assistant text (observed 2026-09-30 after a
    /// `sleep 60`). `#[serde(default)]` covers only an ABSENT field, so the
    /// null failed the whole payload as `parse stop JSON` — a fail-closed stop
    /// with no document, no session and no recovery guidance — and the
    /// `missing_last_assistant_message` path written for exactly this
    /// tool-only stop was unreachable.
    #[serde(default, deserialize_with = "null_as_default")]
    last_assistant_message: String,
    #[serde(default, deserialize_with = "null_as_default")]
    stop_hook_active: bool,
}

fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Claude Code's Stop payload. Claude does not expose a turn id, so its guard
/// is deliberately narrower than the Codex recovery hook: it only enforces a
/// clean closeout's already-durable queue-continuation decision.
#[derive(Debug, Clone, Deserialize)]
struct ClaudeStopInput {
    session_id: String,
    cwd: String,
    #[serde(default)]
    stop_hook_active: bool,
    /// Claude Code's session transcript (JSONL). `#stoploopalreadyarmed`: read
    /// to tell whether this turn already scheduled the `/loop` re-entry.
    #[serde(default)]
    transcript_path: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ClaudeStopBlock {
    decision: &'static str,
    reason: String,
}

/// `#stopfeedbacknoterror`: a queue continuation keeps the Claude turn going
/// through `hookSpecificOutput.additionalContext`, not `decision: "block"`.
///
/// Claude Code labels every Stop-hook `decision: "block"` (and every exit-2
/// stderr) as a hook ERROR, so each drained queue item showed the operator
/// "Stop hook error" beside text that said "not an error". Since Claude Code
/// 2.1.163 a Stop hook may return `additionalContext` instead: "Non-error
/// feedback for Claude. The conversation continues so Claude can act on it,
/// but unlike `decision: "block"` it is shown in the transcript as hook
/// feedback rather than a hook error", under the same loop protections
/// (`stop_hook_active` and the consecutive-continuation cap). A continuation is
/// the hook working as designed, so it uses the feedback form. A genuine hook
/// failure still uses [`ClaudeStopBlock`]: that one IS an error.
///
/// Claude-only. The Codex Stop contract (`StopResponse`) is unchanged.
#[derive(Debug, PartialEq, Eq)]
struct ClaudeStopContinuation {
    reason: String,
}

impl ClaudeStopContinuation {
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "Stop",
                "additionalContext": self.reason,
            }
        })
    }
}

/// Semantic outcome of a Codex Stop decision.
///
/// The wire protocol unfortunately spells [`Self::ContinueTurn`] as
/// `decision: "block"`. For a Stop hook that means "create another prompt in
/// this turn", not "reject the turn". Keeping the host spelling out of the
/// domain type prevents healthy continuation from being confused with
/// [`Self::RefuseExit`] in code and makes every model-visible continuation
/// identify itself as non-error feedback.
#[derive(Debug, PartialEq, Eq)]
enum StopResponse {
    /// `P || !D`: persistence is proved, or this thread owns no write debt.
    AllowExit,
    /// `D && !P && C`: write debt exists, is unproved, and a same-turn recovery remains possible.
    ContinueTurn {
        class: StopContinuationClass,
        reason: String,
    },
    /// `D && !P && !C`: unproved write debt exists and this Stop invocation cannot recover it.
    RefuseExit { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopContinuationClass {
    Queue,
    SessionWriteback,
    StatusSync,
    Recovery,
}

impl StopContinuationClass {
    fn prefix(self) -> &'static str {
        match self {
            Self::Queue => "Queue continuation, not an error: ",
            Self::SessionWriteback => "Session persistence continuation, not a refusal or error: ",
            Self::StatusSync => "Final status synchronization, not an error: ",
            Self::Recovery => "Recovery continuation, not a refusal or error: ",
        }
    }
}

impl Serialize for StopResponse {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::AllowExit => serde_json::json!({ "continue": true }).serialize(serializer),
            Self::ContinueTurn { class, reason } => serde_json::json!({
                "decision": "block",
                "reason": format!("{}{reason}", class.prefix()),
            })
            .serialize(serializer),
            Self::RefuseExit { reason } => serde_json::json!({
                "continue": false,
                "stopReason": reason,
            })
            .serialize(serializer),
        }
    }
}

enum StopCloseAttempt {
    Closed,
    /// The response closeout is already committed, and a later operator edit
    /// to a typed component is intentionally being left for the next cycle.
    /// The Stop hook must end this turn instead of wrapping that refusal as a
    /// recovery prompt that tells the agent to keep working (`#stopnextedit`).
    UnansweredEditPending,
    StillOpen {
        note: String,
    },
    /// Repair retained the exact write under the binary's keyed retry. This is
    /// still an open closeout, but it is not an instruction for the agent to
    /// repair, recapture, or reopen the cycle.
    RepairDeferredToDurableOwner {
        note: String,
    },
    /// The commit boundary refused only because the captured response was not
    /// yet projected, while a durable owner (retained capture / keyed worker)
    /// holds the intent and commits it on its own delivery edge. Not an agent
    /// action item: the Stop hook continues instead of blocking.
    DeferredToDurableOwner,
    /// `#fixlazilycodex`: the cycle is already committed and the Stop payload
    /// is already materialized in HEAD. The terminal closeout already owns
    /// this exact text, so there is nothing to capture or reopen: an
    /// idempotent no-op, never a fail-closed error.
    AlreadyCommitted,
    NotPossible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DurableOwnerProgressDecision {
    Reinspect,
    ResumeThenReinspect,
    StillPending,
}

/// Policy for an error returned by the final closeout/commit boundary.
///
/// A durable owner can make two different promises. A missing response body is
/// already owned by the retained delivery worker and can be left to that
/// worker directly. An active closeout lease is only a transient snapshot, so
/// Stop must follow the exact cycle's controller projection before deciding
/// whether to resume or surface a blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseoutFailureDisposition {
    FailClosed,
    DeferToDurableOwner,
    AwaitDurableOwner,
}

/// Result of the Stop hook's single opportunistic captured-finalize attempt.
///
/// `NeedsOperator` must remain distinct from an ordinary pending retry. Once
/// the exact keyed owner reaches that verdict, telling the agent to keep
/// waiting on the owner is stale and hides the only transition that can
/// unblock the closeout.
#[derive(Debug, PartialEq, Eq)]
enum CapturedFinalizeHookResume {
    Closed,
    Pending,
    NeedsOperator { reason: String },
}

fn durable_owner_progress_decision(
    outcome: agent_doc_closeout_runtime_io::CloseoutCycleWaitOutcome,
) -> DurableOwnerProgressDecision {
    use agent_doc_closeout_runtime_io::CloseoutCycleWaitOutcome;

    match outcome {
        CloseoutCycleWaitOutcome::Terminal | CloseoutCycleWaitOutcome::Superseded => {
            DurableOwnerProgressDecision::Reinspect
        }
        CloseoutCycleWaitOutcome::OwnerReleased => {
            DurableOwnerProgressDecision::ResumeThenReinspect
        }
        CloseoutCycleWaitOutcome::TimedOut => DurableOwnerProgressDecision::StillPending,
    }
}

fn await_durable_owner_progress_with(
    file: &Path,
    cycle_id: &str,
    await_progress: impl FnOnce(
        &Path,
        &str,
    ) -> Result<agent_doc_closeout_runtime_io::CloseoutCycleWaitOutcome>,
) -> Result<DurableOwnerProgressDecision> {
    await_progress(file, cycle_id).map(durable_owner_progress_decision)
}

/// What [`reopen_terminal_cycle_before_stop_capture`] did before capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopCaptureReopen {
    /// No terminal predecessor or no replayable payload: capture proceeds on
    /// the existing cycle state unchanged.
    NotNeeded,
    /// A fresh cycle was minted after the committed predecessor.
    Reopened,
    /// The committed predecessor's HEAD already holds the payload verbatim.
    PayloadAlreadyCommitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopPaneIdentity {
    Ambient,
    AuthoritativeActor(String),
}

fn stop_pane_identity(
    identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin,
    actor: Option<&agent_doc_controller::actor::ActorRecord>,
) -> StopPaneIdentity {
    if identity_origin != agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook {
        return StopPaneIdentity::Ambient;
    }
    match actor {
        Some(actor) if actor.state != agent_doc_controller::actor::ActorState::Closed => {
            StopPaneIdentity::AuthoritativeActor(actor.pane_id.clone())
        }
        _ => StopPaneIdentity::Ambient,
    }
}

/// Inner wall-clock budget for one Codex Stop hook invocation.
///
/// The route-owned supervisor owns long-lived closeout retries. A hook is an
/// interactive status gate and must return a valid fail-closed response before
/// the harness's outer timeout can discard its output.
pub const STOP_HOOK_BUDGET_SECS: u64 = 45;
/// Leave most of the Stop hook's budget for authoritative document inspection
/// and a fail-closed response when SQLite is contended by a concurrent closeout.
const STOP_HOOK_STATE_DB_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const STOP_HOOK_BUDGET_ENV: &str = "AGENT_DOC_CODEX_STOP_HOOK_BUDGET_SECS";
/// Deadline source for one Stop hook invocation (`#stophookdeadlineflake`).
///
/// Unset (production): the [`STOP_HOOK_BUDGET_SECS`] wall-clock budget, so the
/// hook answers before the harness's outer timeout. `worker_completion`: no
/// wall-clock deadline; the hook waits for the worker's actual completion
/// event. Integration tests that assert what the hook DOES (replay, resume,
/// commit) select it, because racing real closeout work against a fixed timer
/// made their outcome depend on machine load: at load average ~100 the same
/// fixture took 37-42s in one phase and failed closed on the 45s budget. The
/// budget's own behaviour is covered deterministically by the unit tests below
/// with an injected task.
const STOP_HOOK_DEADLINE_ENV: &str = "AGENT_DOC_CODEX_STOP_HOOK_DEADLINE";
const STOP_HOOK_DEADLINE_WORKER_COMPLETION: &str = "worker_completion";
#[cfg(test)]
const STOP_HOOK_TEST_DELAY_MS_ENV: &str = "AGENT_DOC_CODEX_STOP_HOOK_TEST_DELAY_MS";

struct StopHookRun {
    response: StopResponse,
    timed_out: bool,
}

/// `#codexstopbudgetblind`: where one Stop hook invocation spent its budget.
///
/// The hook already had a phase timer, but it was called from three places
/// inside `attempt_stop_closeout` and nowhere else — so every document
/// resolution before those points, and the whole of `apply_stop` around them,
/// was attributed to nothing. A hook that blew its 45s budget therefore failed
/// closed while emitting no timing at all: a search of every log on the dogfood
/// machine returned zero `codex_stop.` perf lines despite live overruns.
///
/// The ledger is shared with the worker thread rather than owned by it, because
/// the timing is only interesting in exactly the case where the worker has NOT
/// returned. Rust threads cannot be cancelled, so on timeout the main thread
/// reads what the still-running worker has recorded so far and names the phase
/// it is stuck in.
#[derive(Default)]
struct StopPhaseLedgerInner {
    completed: Vec<(String, u128)>,
    current: Option<(String, std::time::Instant)>,
}

#[derive(Clone, Default)]
struct StopPhaseLedger(std::sync::Arc<std::sync::Mutex<StopPhaseLedgerInner>>);

impl StopPhaseLedger {
    fn enter(&self, phase: &str) {
        let mut inner = self.0.lock().unwrap_or_else(|err| err.into_inner());
        if let Some((name, started)) = inner.current.take() {
            let elapsed = started.elapsed().as_millis();
            inner.completed.push((name, elapsed));
        }
        inner.current = Some((phase.to_string(), std::time::Instant::now()));
    }

    /// Render `phase=ms` for every completed phase, then the phase still
    /// running and how long it has been running. Never panics on a poisoned
    /// lock: a diagnostic that disappears when something else went wrong is
    /// worthless precisely when it is needed.
    fn render(&self) -> String {
        let inner = self.0.lock().unwrap_or_else(|err| err.into_inner());
        let mut parts: Vec<String> = inner
            .completed
            .iter()
            .map(|(phase, ms)| format!("{phase}={ms}ms"))
            .collect();
        if let Some((phase, started)) = inner.current.as_ref() {
            parts.push(format!(
                "{phase}=RUNNING_{}ms",
                started.elapsed().as_millis()
            ));
        }
        if parts.is_empty() {
            "none recorded".to_string()
        } else {
            parts.join(" ")
        }
    }
}

thread_local! {
    static STOP_PHASE_LEDGER: std::cell::RefCell<Option<StopPhaseLedger>> =
        const { std::cell::RefCell::new(None) };
}

/// Record that the Stop hook has entered `phase`. A no-op outside the worker
/// thread, so call sites need no plumbing and tests need no setup.
fn stop_phase(phase: &str) {
    STOP_PHASE_LEDGER.with(|ledger| {
        if let Some(ledger) = ledger.borrow().as_ref() {
            ledger.enter(phase);
        }
    });
}

fn stop_hook_deadline() -> Option<std::time::Duration> {
    resolve_stop_hook_deadline(
        std::env::var(STOP_HOOK_DEADLINE_ENV).ok().as_deref(),
        std::env::var(STOP_HOOK_BUDGET_ENV).ok().as_deref(),
    )
}

/// `None` means "wait for the worker's completion event" (no wall-clock
/// deadline); otherwise the wall-clock budget.
fn resolve_stop_hook_deadline(
    mode: Option<&str>,
    budget_raw: Option<&str>,
) -> Option<std::time::Duration> {
    if mode.map(str::trim) == Some(STOP_HOOK_DEADLINE_WORKER_COMPLETION) {
        None
    } else {
        Some(resolve_stop_hook_budget(budget_raw))
    }
}

fn resolve_stop_hook_budget(raw: Option<&str>) -> std::time::Duration {
    let secs = raw
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(STOP_HOOK_BUDGET_SECS);
    std::time::Duration::from_secs(secs)
}

pub fn handle_stop() -> Result<()> {
    let run = match read_stdin_payload()
        .and_then(|payload| serde_json::from_str::<StopInput>(&payload).context("parse stop JSON"))
        .and_then(|input| apply_stop_within_budget(input, stop_hook_deadline()))
    {
        Ok(run) => run,
        Err(err) => StopHookRun {
            response: StopResponse::RefuseExit {
                reason: format!("agent-doc Stop hook failed closed: {err}"),
            },
            timed_out: false,
        },
    };
    println!("{}", serde_json::to_string(&run.response)?);
    if run.timed_out {
        // Rust threads cannot be cancelled. A timed-out recovery worker may
        // still own long-running IO, so returning from this function alone can
        // leave the hook process alive until the harness kills it and discards
        // the response. Flush the valid fail-closed response, then terminate
        // the hook process without waiting for that detached worker.
        use std::io::Write as _;
        std::io::stdout()
            .flush()
            .context("flush timed-out Codex Stop response")?;
        std::process::exit(0);
    }
    Ok(())
}

/// Claude Code Stop boundary for a completed queue item.
///
/// The ordinary Claude Stop hook was previously cosmetic (`turn-status idle`),
/// leaving the model free to emit a final answer after `session-check` had
/// durably proven another drainable queue head. Bindings are exact-session
/// scoped so an unrelated Claude conversation can never inherit this work.
pub fn handle_claude_stop() -> Result<()> {
    // Read stdin exactly once; `claude_stop_response` owns every decision made
    // from it, so the tests exercise the same function the hook does.
    let response = claude_stop_response(read_stdin_payload().as_deref().ok())?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// The JSON the Claude Stop hook prints for `payload`.
///
/// Every path that can refuse a final answer has to be bounded, and this
/// function is where that is decided:
///
/// * the continuation guard is bounded by the run-keyed request ledger
///   (`apply_claude_stop`);
/// * a hook error is bounded by `stop_hook_active` — errors here are
///   overwhelmingly persistent (an unreadable document, a refused authority
///   resolve, a state ledger that will not open), so re-blocking re-runs the
///   same failing check against the same inputs forever. This was the one path
///   through the hook that never consulted it;
/// * a payload that is not JSON at all cannot carry `stop_hook_active`, so a
///   refusal there could never be bounded — and there is nothing to bound it
///   FOR: with no parseable envelope there is no session, no document, and no
///   continuation to protect. A refusal issued on no evidence about a document
///   is just an unbounded loop.
///
/// Modelled in `formal/tla/StopHookContinuation.tla`.
fn claude_stop_response(payload: Option<&str>) -> Result<serde_json::Value> {
    let envelope =
        payload.and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok());
    let Some(envelope) = envelope else {
        eprintln!(
            "[agent-doc] Claude Stop hook received a payload that is not JSON; it names no \
             session or document, so there is no queue continuation to check. Allowing the \
             final answer."
        );
        return Ok(serde_json::json!({}));
    };
    let stop_hook_active = envelope
        .get("stop_hook_active")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    match serde_json::from_value::<ClaudeStopInput>(envelope)
        .context("parse Claude Stop JSON")
        .and_then(|input| apply_claude_stop(&input))
    {
        Ok(response) => Ok(response
            .map(|response| response.to_hook_output())
            .unwrap_or_else(|| serde_json::json!({}))),
        // Failing closed is right the FIRST time: the operator needs to hear
        // that the continuation check could not run. Repeating it is not.
        Err(err) if stop_hook_active => {
            eprintln!(
                "[agent-doc] Claude Stop hook failed again while checking queue continuation: \
                 {err:#}. Already blocked once for this stop; allowing the final answer so the \
                 failure is reported instead of looped."
            );
            Ok(serde_json::json!({}))
        }
        Err(err) => Ok(serde_json::to_value(ClaudeStopBlock {
            decision: "block",
            reason: format!(
                "agent-doc Claude Stop hook failed closed while checking queue continuation: {err:#}. Do not send the final answer; report this hook failure to the operator."
            ),
        })?),
    }
}

fn apply_claude_stop(input: &ClaudeStopInput) -> Result<Option<ClaudeStopContinuation>> {
    apply_claude_stop_with_drain_readiness(
        input,
        agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc,
    )
}

/// Whether a previous queue handoff to the supervisor is still undrained.
fn supervisor_handoff_overdue(file: &Path) -> bool {
    match current_document_content(file, "claude_stop_supervisor_handoff_check") {
        Ok(content) => {
            agent_doc_queue_io::queue_auto_clear::undrained_supervisor_drain_handoff_age(
                file, &content,
            )
            .is_some()
        }
        Err(error) => {
            eprintln!(
                "[agent-doc] Claude Stop hook could not read {} to check the supervisor handoff: {error}",
                file.display()
            );
            true
        }
    }
}

const FINAL_STATUS_SYNC_MARKER: &str = "[agent-doc] final status-sync checkpoint";

/// Re-surface a persistent status synchronization contract at the owner turn's
/// final lifecycle boundary.
///
/// Preflight is the first checkpoint and an optional `interval` provides
/// mid-turn checkpoints. Neither proves that a fact which changed during the
/// final tool call is current at handoff. The Stop hook is the only boundary
/// that can make that guarantee without polling. It requests one bounded
/// re-evaluation and relies on the harness' `stop_hook_active` recursion bit to
/// pass the following Stop through.
fn final_status_sync_continuation(file: &Path, stop_hook_active: bool) -> Result<Option<String>> {
    if stop_hook_active {
        return Ok(None);
    }

    // Read the editor-authoritative document cut, not a possibly stale disk
    // mirror: the operator can add, edit, or remove `sync` during the turn.
    let content = current_document_content(file, "stop_final_status_sync")?;
    let Some(directive) = agent_doc_document::status_projection::status_sync_directive(&content)?
    else {
        return Ok(None);
    };
    let mode = match directive.mode {
        agent_doc_document::status_projection::StatusSyncMode::Additive => "additive",
        agent_doc_document::status_projection::StatusSyncMode::Replace => "replace",
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "status_sync_final_checkpoint_surfaced file={} mode={} source=stop_hook",
            file.display(),
            mode,
        ),
    );
    Ok(Some(format!(
        "{FINAL_STATUS_SYNC_MARKER}\n\
         Before ending this owner turn, re-evaluate the persistent `agent:status` instruction \
         against its current authoritative source: {}\n\
         Mode: `{mode}`. If its answer changed, run `agent-doc status set '{}' --text \
         \"<current result>\"`, then `agent-doc commit '{}'` so the focused status update is \
         durable. Pass only the result: agent-doc owns the `<!-- status:sync -->` markers. \
         If the answer did not change, make no write. This is a bounded final checkpoint; do \
         not repeat completed work or start unrelated work, then send the final answer.{}",
        directive.instruction,
        file.display(),
        file.display(),
        // `#statuspatchdrop` (c): quote the current payload so "did the answer
        // change" is decided against the document, not the agent's memory of a
        // status write that may not have landed.
        agent_doc_document::status_projection::render_status_sync_checkpoint_evidence(&content)
            .map(|evidence| format!("\n{evidence}"))
            .unwrap_or_default(),
    )))
}

/// [`apply_claude_stop`] with the supervisor drain-readiness probe injected.
fn apply_claude_stop_with_drain_readiness(
    input: &ClaudeStopInput,
    drain_readiness: impl Fn(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Result<Option<ClaudeStopContinuation>> {
    let cwd = PathBuf::from(&input.cwd);
    let Some((_loaded_root, state)) = load_bound_session_for_stop(&cwd, &input.session_id)? else {
        return Ok(None);
    };
    let file = PathBuf::from(&state.doc_path);
    if !file.exists() {
        return Ok(None);
    }

    // `#stopneedsclosedcycle`: the marker and the stall projection are both
    // document-level and OUTLIVE the cycle that wrote them, so neither one
    // proves that *this* turn closed. A marker left by the last clean closeout
    // satisfied both reads while the current cycle sat at `response_captured`
    // with nothing committed, and the hook then told the agent "the completed
    // cycle durably proved another drainable head" and forbade its final
    // answer. The head it named was the one the failed turn had already
    // answered and reaped in authority — the disk mirror the queue head is read
    // from is exactly what a retained write has not updated yet. Observed
    // 2026-09-27 on tasks/agent-doc/agent-doc-bugs.md (`#focusstashedactor`,
    // stranded on `editor_attached_model_missing` after a mid-session install)
    // and the same day on tasks/software/lazily.md (`#lzgooptionalpgx`).
    //
    // A failed closeout is already first in SKILL.md's exhaustive skip list, so
    // an open cycle must fail this gate before either document-level read is
    // consulted: looping a retained write re-answers an answered head.
    let cycle = agent_doc_cycle_state_io::load(&file)?;
    if cycle.as_ref().is_some_and(|cycle| cycle.is_open()) {
        agent_doc_ops_log_io::log_op(
            &file,
            "claude_stop_queue_continuation_skipped reason=cycle_open action=allow_final_answer",
        );
        return Ok(None);
    }
    // `#stopnoresponserun`: a closed cycle that never captured a response
    // answered nothing, so it cannot have drained a head. The stale-lock repair
    // closes an abandoned `PreflightStarted` cycle exactly this way, and the next
    // trigger is then refused admission. Observed 2026-10-01 on
    // tasks/agent-doc/agent-doc-bugs.md: the repair committed
    // `cycle-1790898286195` with no response, the operator's trigger was refused,
    // and this hook still forbade the final answer and ordered a `/loop` that
    // could only hit the same refusal. The operator must hear the refusal.
    if cycle
        .as_ref()
        .is_some_and(|cycle| cycle.response_sha256.is_none())
    {
        agent_doc_ops_log_io::log_op(
            &file,
            "claude_stop_queue_continuation_skipped reason=run_captured_no_response action=allow_final_answer",
        );
        return Ok(None);
    }
    // The run this stop is deciding about: the cycle that has just closed. A
    // continuation request is a request that THIS run be followed by another,
    // so it is the run, not the document text, that the repeat guard below
    // reconciles against.
    let run_id = cycle.as_ref().map(|cycle| cycle.cycle_id.clone());
    if let Some(continuation) =
        claude_queue_continuation(input, &file, run_id.as_deref(), &drain_readiness)?
    {
        return Ok(Some(continuation));
    }
    if let Some(continuation) =
        claude_steering_continuation(input, &file, run_id.as_deref(), &drain_readiness)?
    {
        return Ok(Some(continuation));
    }
    Ok(
        final_status_sync_continuation(&file, input.stop_hook_active)?
            .map(|reason| ClaudeStopContinuation { reason }),
    )
}

/// The queue half of [`apply_claude_stop_with_drain_readiness`]: a durably
/// proven next queue head that nobody else will drain.
fn claude_queue_continuation(
    input: &ClaudeStopInput,
    file: &Path,
    run_id: Option<&str>,
    drain_readiness: &impl Fn(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Result<Option<ClaudeStopContinuation>> {
    let file = file.to_path_buf();
    // A continuation marker or the session-check stall projection proves that
    // the prior item reached a clean closeout. This avoids redirecting an
    // unfinished response back through `/loop` merely because its current head
    // remains visible in the document.
    let marker = agent_doc_queue_io::continuation_marker::load_continuation_marker(&file)?;
    let continuation_proven = marker.is_some()
        || agent_doc_controller_io::project_controller::
            queue_drain_stall_continuation_pending_for_file(&file)?
            .is_some();
    if !continuation_proven {
        return Ok(None);
    }
    let Some(prompt) = active_auto_queue_prompt(&file)? else {
        return claimed_heads_stop_continuation(&file);
    };

    // `#stopneedsclosedcycle`, second half: `stop_hook_active` only breaks
    // recursion WITHIN one stop. Across separate turns the bound is this
    // request ledger.
    //
    // It used to live on `ContinuationMarker::last_requested_head`, and the
    // comparison was right while the storage was not. The marker belongs to
    // queue reconciliation: it can be absent on exactly the branch reached here
    // (`continuation_proven` is satisfied by the stall projection alone), where
    // the arming write documented itself as a no-op — so the bound was absent,
    // not weak — and a reconcile between two stops deletes it, disarming a
    // guard that had been armed. Measured 24 blocks against 2 skips on
    // tasks/agent-doc/agent-doc-bugs.md 2026-09-28, every repeat inside one run.
    //
    // Keying on the run is also strictly sharper than keying on the head. At
    // 00:30:47 the head moved 62 -> 26 bytes while `turn` stayed on
    // `cycle-1790552355729`: a head can move because the operator edited the
    // document or a reconcile rewrote the queue, and neither is drain progress.
    // Only a completed run is. Falling through hands the turn back to the
    // agent, which is what lets the operator hear about it.
    let previous_request =
        agent_doc_queue_io::continuation_request::load_continuation_request(&file)?;
    if let Some(reason) = agent_doc_queue_io::continuation_request::non_advancing_continuation(
        previous_request.as_ref(),
        run_id,
        &prompt,
    ) {
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "claude_stop_queue_continuation_skipped reason={} \
                 head_bytes={} action=allow_final_answer",
                reason.token(),
                prompt.len(),
            ),
        );
        return Ok(None);
    }

    // `#stoploopalreadyarmed`: the agent has already scheduled the exact
    // `/loop agent-doc <FILE>` re-entry this block would ask for. Blocking again
    // cannot change what happens next; it only surfaces a "Stop hook blocking
    // error" on every drained item and re-asks for a wake-up that is pending.
    if let Some(transcript) = input.transcript_path.as_deref()
        && claude_transcript_arms_loop_reentry(Path::new(transcript), &file)
    {
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "claude_stop_queue_continuation_already_armed head_bytes={} source=transcript_schedule_wakeup action=allow_final_answer",
                prompt.len(),
            ),
        );
        return Ok(None);
    }

    // `#stopblocksupervisorowned`: a continuation request is only needed when no
    // one else will continue the queue. (It used to be a `decision: "block"`,
    // which Claude Code renders as "Stop hook error"; it is now non-error
    // `additionalContext` feedback, `#stopfeedbacknoterror`, but it still costs
    // the agent a turn, so it is still withheld from a ready supervisor.) A live, fresh supervisor's idle-queue watch
    // submits the next `agent-doc <FILE>` trigger into this pane once the turn
    // ends — a real submitted prompt that seals its own cycle contract — so the
    // block adds nothing but the error line. Block only when that supervisor is
    // missing, stale, or running a stale binary — or when an earlier handoff to
    // it is still undrained (`#supdrainyieldfalsifiable`): a supervisor that has
    // not kept its last promise does not get this one.
    let readiness = drain_readiness(&file);
    if readiness.is_ready() && !supervisor_handoff_overdue(&file) {
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "claude_stop_queue_continuation_supervisor_owned head_bytes={} \
                 action=allow_final_answer reason=supervisor_idle_watch_drains",
                prompt.len(),
            ),
        );
        return Ok(None);
    }

    if input.stop_hook_active {
        // Claude explicitly requires Stop hooks to break their own recursion.
        // The route-owned supervisor and stall projection remain the bounded
        // fallback if the model ignored the first block instead of invoking the
        // loop skill as directed.
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "claude_stop_queue_continuation_repeat head_bytes={} action=allow_bounded_fallback",
                prompt.len(),
            ),
        );
        return Ok(None);
    }

    // Arm the bound BEFORE blocking, and propagate a failure instead of
    // discarding it: an unarmed guard is an unbounded loop, so a ledger write
    // that cannot land must fail the hook closed rather than quietly produce
    // the block it can no longer bound.
    agent_doc_queue_io::continuation_request::record_continuation_request(&file, run_id, &prompt)
        .with_context(|| {
        format!(
            "record the Stop-hook continuation request for {}",
            file.display()
        )
    })?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "claude_stop_queue_continuation head_bytes={} run={} source=exact_session_binding action=block_and_loop",
            prompt.len(),
            run_id.unwrap_or("none"),
        ),
    );
    Ok(Some(ClaudeStopContinuation {
        reason: claude_stop_continuation_reason(&file.display().to_string(), &prompt),
    }))
}

/// `#steerinterruptexit`: the steering half of the Claude Stop gate.
///
/// `session-check` now reports operator steering after a committed cycle as
/// `steering pending` with exit `0`, so the old implicit stop (a closeout that
/// "failed" and skipped the auto-loop) no longer happens. An exchange prompt the
/// operator added while the turn ran is the next cycle's input and only an
/// agent cycle can answer it, so the final answer is held once per run for it,
/// with the same bounds as the queue continuation: never inside a recursive
/// stop, never twice for one run (the shared request ledger), not when the
/// `/loop` re-entry is already armed, and not when a ready supervisor's
/// steering wake (`#steeringwake`) will submit the trigger itself. Queue-sourced
/// steering is queue work: the queue continuation above already decided it.
fn claude_steering_continuation(
    input: &ClaudeStopInput,
    file: &Path,
    run_id: Option<&str>,
    drain_readiness: &impl Fn(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Result<Option<ClaudeStopContinuation>> {
    // Observation failures allow the final answer: this gate adds a hold the
    // hook never had, so it must not add a new way for the hook to fail closed.
    let observed = (|| -> Result<Option<(String, String, usize)>> {
        let content = current_document_content(file, "claude_stop_pending_steering")?;
        let items =
            agent_doc_session_check_io::midturn_steering::pending_steering_items(file, &content);
        let Some(first) = items
            .iter()
            .find(|item| agent_doc_session_check_io::midturn_steering::is_exchange_item(item))
        else {
            return Ok(None);
        };
        // The same verdict `session-check` and preflight admission derive from.
        let Some(marker) = agent_doc_session_check_io::turn_admission(file, false)?.steering else {
            return Ok(None);
        };
        Ok(Some((marker, first.verbatim.clone(), items.len())))
    })();
    let (marker, verbatim, item_count) = match observed {
        Ok(Some(observed)) => observed,
        Ok(None) => return Ok(None),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "claude_stop_steering_observation_failed error={err:#} action=allow_final_answer (#steerinterruptexit)"
                ),
            );
            return Ok(None);
        }
    };
    if input.stop_hook_active {
        agent_doc_ops_log_io::log_op(
            file,
            "claude_stop_steering_continuation_repeat action=allow_bounded_fallback (#steerinterruptexit)",
        );
        return Ok(None);
    }
    let previous_request =
        agent_doc_queue_io::continuation_request::load_continuation_request(file)?;
    if let Some(reason) = agent_doc_queue_io::continuation_request::non_advancing_continuation(
        previous_request.as_ref(),
        run_id,
        &marker,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "claude_stop_steering_continuation_skipped reason={} action=allow_final_answer (#steerinterruptexit)",
                reason.token(),
            ),
        );
        return Ok(None);
    }
    if let Some(transcript) = input.transcript_path.as_deref()
        && claude_transcript_arms_loop_reentry(Path::new(transcript), file)
    {
        agent_doc_ops_log_io::log_op(
            file,
            "claude_stop_steering_continuation_already_armed action=allow_final_answer (#steerinterruptexit)",
        );
        return Ok(None);
    }
    let supervisor_wakes = drain_readiness(file).is_ready()
        && !supervisor_handoff_overdue(file)
        // Settled items wake now; items still settling wake once they settle.
        && agent_doc_session_check_io::midturn_steering::observe_for_wake(file)
            .is_ok_and(|wake| !wake.items.is_empty() || wake.pending > 0);
    if supervisor_wakes {
        agent_doc_ops_log_io::log_op(
            file,
            "claude_stop_steering_continuation_supervisor_owned action=allow_final_answer reason=steering_wake (#steerinterruptexit)",
        );
        return Ok(None);
    }
    agent_doc_queue_io::continuation_request::record_continuation_request(file, run_id, &marker)
        .with_context(|| {
            format!(
                "record the Stop-hook steering continuation request for {}",
                file.display()
            )
        })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "claude_stop_steering_continuation run={} items={} action=block_and_loop (#steerinterruptexit)",
            run_id.unwrap_or("none"),
            item_count,
        ),
    );
    Ok(Some(ClaudeStopContinuation {
        reason: claude_stop_steering_reason(&file.display().to_string(), &verbatim),
    }))
}

/// The Stop-hook instruction for pending exchange steering. Like
/// [`claude_stop_continuation_reason`], it names the one re-entry that admits
/// a cycle; the steering is the next cycle's prompt, not a failed closeout.
pub fn claude_stop_steering_reason(file_display: &str, verbatim: &str) -> String {
    format!(
        "agent-doc operator steering is pending for {file_display}: the operator added {head:?} \
         after your response was committed. This is not a failed closeout and nothing needs \
         repair; it is the next cycle's prompt. Re-enter with `ScheduleWakeup` using exactly the \
         prompt `/loop agent-doc {file_display}` (a `loop` Skill call admits no cycle). Do not \
         re-run finalize, do not `--force-disk`. (`#steerinterruptexit`)",
        head = continuation_head_preview(verbatim),
    )
}

/// `#queueclaim`: when no drainable head remains, distinguish worker-bound
/// claims from an overdue `Claimed` attempt. The latter is a coordinator
/// obligation and must block Stop with the exact start/requeue commands instead
/// of being mislabeled as in-flight work (GH #301).
fn claimed_heads_stop_continuation(file: &Path) -> Result<Option<ClaudeStopContinuation>> {
    claimed_heads_stop_continuation_at(
        file,
        agent_doc_queue_io::queue_claim::claim_clock_now_secs(),
        agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
    )
}

fn claimed_heads_stop_continuation_at(
    file: &Path,
    now_secs: u64,
    grace_secs: u64,
) -> Result<Option<ClaudeStopContinuation>> {
    let content = match current_document_content(file, "claude_stop_claimed_heads_check") {
        Ok(content) => content,
        Err(err) => {
            eprintln!(
                "[agent-doc] Claude Stop hook could not read {} to report claimed queue heads: {err:#}",
                file.display()
            );
            return Ok(None);
        }
    };
    let obligations =
        agent_doc_queue_io::queue_claim::dispatch_acknowledgement_obligations_for_content_at(
            file, &content, now_secs, grace_secs,
        )?;
    if !obligations.is_empty() {
        let remedies = obligations
            .iter()
            .map(|obligation| obligation.message())
            .collect::<Vec<_>>()
            .join(" ");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "claude_stop_queue_dispatch_acknowledgement_missing obligations={} action=block_and_repair",
                obligations.len(),
            ),
        );
        return Ok(Some(ClaudeStopContinuation {
            reason: format!(
                "agent-doc cannot end this turn because a claimed queue dispatch has no worker \
                 acknowledgement. Resolve the coordinator obligation, then finish closeout. \
                 {remedies}"
            ),
        }));
    }
    let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(file, &content);
    let claimed_heads = agent_doc_queue::queue_continuation::claimed_head_count(&content, &claimed);
    if claimed_heads > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "claude_stop_queue_continuation_skipped reason=heads_claimed_in_flight \
                 claimed_heads={claimed_heads} action=allow_final_answer state=waiting_on_claims"
            ),
        );
    }
    Ok(None)
}

/// `#loopreentrynoop`: the Claude Code Stop-hook continuation instruction.
///
/// The original text named exactly one mechanism -- "Invoke the `loop` skill" --
/// and that mechanism is a NO-OP for every iteration after the first. Once
/// `/loop` is loaded in a session, the Skill tool answers "already loaded ...
/// instructions unchanged" and seals no cycle contract, because a Skill call is
/// not a submitted prompt and so never fires the `UserPromptSubmit` hook that
/// runs binary preflight. Observed six consecutive times on
/// tasks/agent-doc/agent-doc-bugs.md.
///
/// That left the agent with no legal move: this same message forbids shelling
/// `agent-doc <FILE>`, and `#preflightinbinary` forbids shelling
/// `agent-doc preflight`. The drain then looked stalled while the agent was
/// doing the only thing available to it, and `#qstallguard` scored it as
/// `no_valid_stop_with_continuation_required`.
///
/// So the instruction now names the fallback that actually re-enters with a
/// sealed contract: schedule a wake-up that SUBMITS the trigger as a real
/// prompt. Keep the no-op case and the fallback together -- naming the fallback
/// without naming the symptom leaves the agent unable to tell that it needs one.
/// Bytes of transcript tail inspected; one turn's records fit comfortably.
const CLAUDE_TRANSCRIPT_TAIL_BYTES: u64 = 4 * 1024 * 1024;

fn claude_transcript_tail(transcript: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut handle = std::fs::File::open(transcript).ok()?;
    let len = handle.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = len.saturating_sub(CLAUDE_TRANSCRIPT_TAIL_BYTES);
    if let Err(err) = handle.seek(SeekFrom::Start(start)) {
        eprintln!("[agent-doc] Claude Stop hook could not read the transcript tail: {err}");
        return None;
    }
    let mut tail = Vec::new();
    if let Err(err) = handle.read_to_end(&mut tail) {
        eprintln!("[agent-doc] Claude Stop hook could not read the transcript tail: {err}");
        return None;
    }
    Some(String::from_utf8_lossy(&tail).into_owned())
}

fn claude_transcript_arms_loop_reentry(transcript: &Path, file: &Path) -> bool {
    claude_transcript_tail(transcript)
        .is_some_and(|tail| transcript_tail_arms_loop_reentry(&tail, file))
}

/// Harness-authored records inside a turn: Stop-hook feedback, and
/// `<task-notification>` wake-ups from background tasks (`#stoplooparmednotify`).
/// Neither is an operator prompt, so neither ends the search for an armed
/// `/loop` re-entry. Treating a task notification as a new prompt hid a wake-up
/// scheduled one turn earlier and re-blocked every notification turn with a
/// "Stop hook error".
fn is_stop_hook_feedback(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("Stop hook feedback:")
        || text.starts_with("<task-notification>")
        // `#stopfeedbacknoterror`: the continuation now arrives as Stop-hook
        // `additionalContext`, which Claude Code injects as a system reminder.
        || text.starts_with("<system-reminder>")
        || text.contains("(`#loopreentrynoop`)")
}

/// `#stoploopalreadyarmed`: true when, after the operator prompt that started
/// the current turn, the assistant called `ScheduleWakeup` with a
/// `/loop ... agent-doc <FILE>` prompt. Stop-hook feedback and tool results are
/// harness records inside the turn, not a new operator prompt, so they do not
/// end the search.
pub fn transcript_tail_arms_loop_reentry(tail: &str, file: &Path) -> bool {
    let file_display = file.display().to_string();
    for line in tail.lines().rev() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let content = record.pointer("/message/content");
        match record.get("type").and_then(serde_json::Value::as_str) {
            Some("assistant") => {
                let armed = content
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|block| {
                        block.get("type").and_then(serde_json::Value::as_str) == Some("tool_use")
                            && block.get("name").and_then(serde_json::Value::as_str)
                                == Some("ScheduleWakeup")
                            && block
                                .pointer("/input/prompt")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|prompt| {
                                    let prompt = prompt.trim();
                                    prompt.starts_with("/loop")
                                        && prompt.contains("agent-doc")
                                        && prompt.contains(file_display.as_str())
                                })
                    });
                if armed {
                    return true;
                }
            }
            Some("user") => {
                let operator_prompt = match content {
                    Some(serde_json::Value::String(text)) => !is_stop_hook_feedback(text),
                    Some(serde_json::Value::Array(blocks)) => blocks.iter().any(|block| {
                        block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                            && !block
                                .get("text")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(is_stop_hook_feedback)
                    }),
                    _ => false,
                };
                if operator_prompt {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

/// GH #85 / `#loopskillnoadmit` / `#loopreentrynoop`: name the one re-entry
/// that admits a cycle, and nothing else.
///
/// A `loop` Skill call never admits a cycle: it is not a submitted prompt, so
/// `UserPromptSubmit` never runs binary preflight or claims the drain-owner
/// lease (verified a no-op 2026-10-01 on tasks/agent-doc/agent-doc-bugs.md).
/// Shelling `agent-doc <FILE>` or `agent-doc preflight` is forbidden too. The
/// working route is a `ScheduleWakeup` whose prompt is the trigger.
///
/// Kept short on purpose (`#stopfeedbacknoterror`): the earlier text repeated
/// itself across ~800 bytes. The head is previewed, not quoted in full, because
/// a multi-line free-text head would otherwise dominate the message.
pub fn claude_stop_continuation_reason(file_display: &str, prompt: &str) -> String {
    format!(
        "agent-doc queue continues for {file_display}; next head: {head:?}. Re-enter with \
         `ScheduleWakeup` using exactly the prompt `/loop agent-doc {file_display}`, not the \
         `loop` skill (a Skill call submits no prompt, so it admits no cycle). Do not shell \
         `agent-doc {file_display}` or `agent-doc preflight`. (`#loopreentrynoop`)",
        head = continuation_head_preview(prompt),
    )
}

/// First line of `prompt`, capped at [`CONTINUATION_HEAD_PREVIEW_CHARS`]
/// characters (char-boundary safe), with an ellipsis when anything was cut.
fn continuation_head_preview(prompt: &str) -> String {
    let trimmed = prompt.trim();
    let first_line = trimmed.lines().next().unwrap_or("");
    let mut preview: String = first_line
        .chars()
        .take(CONTINUATION_HEAD_PREVIEW_CHARS)
        .collect();
    if preview.len() < trimmed.len() {
        preview.push('…');
    }
    preview
}

const CONTINUATION_HEAD_PREVIEW_CHARS: usize = 120;

fn apply_stop_within_budget(
    input: StopInput,
    budget: Option<std::time::Duration>,
) -> Result<StopHookRun> {
    #[cfg(test)]
    if let Some(delay_ms) = std::env::var(STOP_HOOK_TEST_DELAY_MS_ENV)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
    {
        return run_stop_hook_task_within_budget(budget, move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            Ok(StopResponse::AllowExit)
        });
    }
    run_stop_hook_task_within_budget(budget, move || apply_stop(&input))
}

fn run_stop_hook_task_within_budget<F>(
    budget: Option<std::time::Duration>,
    task: F,
) -> Result<StopHookRun>
where
    F: FnOnce() -> Result<StopResponse> + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let ledger = StopPhaseLedger::default();
    let worker_ledger = ledger.clone();
    std::thread::Builder::new()
        .name("agent-doc-codex-stop".to_string())
        .spawn(move || {
            STOP_PHASE_LEDGER.with(|slot| {
                *slot.borrow_mut() = Some(worker_ledger);
            });
            if sender.send(task()).is_err() {
                eprintln!(
                    "[agent-doc] Codex Stop hook worker finished after its response receiver closed"
                );
            }
        })
        .context("spawn Codex Stop hook worker")?;
    let Some(budget) = budget else {
        // `#stophookdeadlineflake`: wait for the worker's completion event.
        return match receiver.recv() {
            Ok(response) => response.map(|response| StopHookRun {
                response,
                timed_out: false,
            }),
            Err(_) => anyhow::bail!("Codex Stop hook worker exited without a response"),
        };
    };
    match receiver.recv_timeout(budget) {
        Ok(response) => response.map(|response| StopHookRun {
            response,
            timed_out: false,
        }),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(StopHookRun {
            response: StopResponse::RefuseExit {
                reason: format!(
                    "agent-doc Stop hook exceeded its {}s internal budget and failed closed before the harness timeout. The route-owned supervisor retains any captured closeout and continues recovery; do not rerun finalize or recapture the response. Phases: {}",
                    budget.as_secs(),
                    ledger.render(),
                ),
            },
            timed_out: true,
        }),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("Codex Stop hook worker exited without a response")
        }
    }
}

/// Resolve only the document binding owned by this exact Codex thread.
///
/// This is intentionally narrower than document- or project-scoped recovery:
/// ambient hooks must not infer ownership from another thread's hook state or
/// from a durable queue marker.
pub fn load_bound_session_for_stop(
    cwd: &Path,
    session_id: &str,
) -> Result<Option<(PathBuf, SessionState)>> {
    let roots = project_roots_for(cwd);
    if roots.is_empty() {
        return Ok(None);
    }
    load_state_any_with_timeout(&roots, session_id, STOP_HOOK_STATE_DB_BUSY_TIMEOUT)
}

fn apply_stop(input: &StopInput) -> Result<StopResponse> {
    stop_phase("load_bound_session");
    let cwd = PathBuf::from(&input.cwd);
    let Some((loaded_root, state)) = load_bound_session_for_stop(&cwd, &input.session_id)? else {
        // Ambient Codex hooks are exact-thread scoped. A durable document queue
        // marker proves that some agent-doc actor owes work, but it does not
        // prove that this Codex thread owns that work. Falling back from a
        // missing exact session binding to a project-scoped marker lets an
        // unrelated pure Codex session inherit agent-doc work.
        return Ok(StopResponse::AllowExit);
    };

    let file = PathBuf::from(&state.doc_path);
    let cleanup_roots = tracking_roots(&cwd, Some(&file));
    if state.preflight_admitted == Some(false) {
        // Admission already told this turn to stop. Neither an older document
        // cycle nor its active queue can authorize capturing that explanation.
        // Retire the denied binding as well: it never acquired document
        // ownership, so a later ordinary prompt in this Codex thread must not
        // inherit the other pane's open-cycle debt.
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        agent_doc_ops_log_io::log_op(&file, "codex_stop_refused_admission_preserved");
        return Ok(StopResponse::AllowExit);
    }
    if !file.exists() {
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        return Ok(StopResponse::AllowExit);
    }

    // Codex runs hooks from its harness hook executor. That executor can carry the tmux pane
    // currently active in the shared window rather than the pane that owns this exact Codex
    // thread. The hook binding has already identified the document; delegate the remaining
    // status/closeout effect to its authoritative actor just like a controller-owned effect.
    // `pane_execution_authority` still re-proves pane liveness and exact process ownership at
    // every mutation boundary, so a stale or foreign actor remains fail-closed.
    let project_root = agent_doc_project_root_io::project_root_containing(&file);
    let actor = project_root
        .as_deref()
        .map(|root| {
            agent_doc_controller_io::project_controller::authoritative_actor_binding(root, &file)
        })
        .transpose()?
        .flatten();
    if let StopPaneIdentity::AuthoritativeActor(pane_id) =
        stop_pane_identity(state.identity_origin, actor.as_ref())
    {
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "codex_stop_actor_identity_bound pane={} source=exact_thread_document_actor",
                pane_id
            ),
        );
        return agent_doc_tmux_io::with_current_pane_id_override(&pane_id, || {
            retire_bound_turn_status_on_stop(project_root.as_deref(), &pane_id, input, &file);
            apply_bound_stop(input, &loaded_root, &state, &file, &cleanup_roots)
        });
    }

    apply_bound_stop(input, &loaded_root, &state, &file, &cleanup_roots)
}

/// `#codexstopexactpane`: the generic Codex `turn-status idle` Stop hook runs
/// in the hook executor's ambient tmux pane. In a shared window that can be a
/// different pane from the exact Codex thread that stopped, leaving this
/// document's turn-active lease and dispatch receipt live indefinitely. The
/// consequential Stop hook already resolved the exact document actor above;
/// project the same idle edge through that authoritative pane identity.
///
/// This is intentionally idempotent with the generic status hook and
/// best-effort, preserving Stop closeout recovery if status storage is
/// temporarily unavailable.
fn retire_bound_turn_status_on_stop(
    project_root: Option<&Path>,
    pane_id: &str,
    input: &StopInput,
    file: &Path,
) {
    let Some(project_root) = project_root else {
        return;
    };
    let payload = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": input.session_id,
        "turn_id": input.turn_id,
        "cwd": input.cwd,
    })
    .to_string();
    if let Err(err) = agent_doc_turn_status_io::apply_turn_status_hook_at(
        project_root,
        pane_id,
        false,
        Some(&payload),
        now_secs(),
    ) {
        eprintln!(
            "[agent-doc] warning: exact-pane Codex Stop idle projection failed for {} in {}: {err:#}",
            file.display(),
            pane_id,
        );
        return;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_turn_status_idle pane={} source=exact_thread_document_actor (#codexstopexactpane)",
            pane_id
        ),
    );
}

fn apply_bound_stop(
    input: &StopInput,
    loaded_root: &Path,
    state: &SessionState,
    file: &Path,
    cleanup_roots: &[PathBuf],
) -> Result<StopResponse> {
    stop_phase("session_check_inspect");
    match agent_doc_session_check_io::inspect(
        file,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )? {
        agent_doc_session_check_io::SessionCheckStatus::Ok(_) => {
            // `#codexsubagentstop`: Codex collaboration workers can share the
            // parent session id while owning a distinct turn id. A clean
            // document here means the remaining work is continuation debt, not
            // closeout recovery; only the coordinator turn recorded by the
            // session ledger may inherit that queue handoff. Preserve legacy
            // parked rows with no turn identity.
            if !state.last_turn_id.trim().is_empty()
                && !input.turn_id.trim().is_empty()
                && state.last_turn_id != input.turn_id
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_foreign_turn_passed_through source=clean_session_turn_fence",
                );
                return Ok(StopResponse::AllowExit);
            }
            stop_phase("auto_queue_continuation");
            if let Some(response) =
                auto_queue_continuation_response(file, cleanup_roots, loaded_root, state, input)?
            {
                return Ok(response);
            }
            stop_phase("active_session_prompt_writeback");
            if let Some(response) = active_session_prompt_requires_writeback(
                file,
                cleanup_roots,
                loaded_root,
                state,
                input,
            )? {
                return Ok(response);
            }
            stop_phase("final_status_sync");
            if let Some(reason) = final_status_sync_continuation(file, input.stop_hook_active)? {
                return Ok(StopResponse::ContinueTurn {
                    class: StopContinuationClass::StatusSync,
                    reason,
                });
            }
            stop_phase("settle_session_binding");
            settle_session_binding(file, cleanup_roots, loaded_root, state)?;
            Ok(StopResponse::AllowExit)
        }
        // `#steerinterruptexit`: steering pending after a committed cycle is a
        // success status for the CLI, but the Stop gate must still hand it back
        // before a final answer. Its message keeps the committed-prompt-diff
        // shape, so it takes the same in-pane steering path an `INTERRUPTED`
        // report from an older binary took; nothing below treats it as a failure.
        agent_doc_session_check_io::SessionCheckStatus::SteeringPending(reason)
        | agent_doc_session_check_io::SessionCheckStatus::Interrupted(reason) => {
            // `#stopsteeringafterabandon`: the first Stop can fail before it
            // captures anything (for example, an operator edit temporarily
            // fragments an exchange marker). Once the harness turn ends, the
            // supervisor correctly abandons that empty preflight. A recursive
            // Stop still owns the same in-pane continuation, however, and the
            // unresolved steering must get a fresh cycle now rather than fall
            // through to "already continued once" and require an operator to
            // type `agent-doc <FILE>` manually.
            if let Some((cycle_id, prompt)) = reopen_abandoned_cycle_for_pending_steering(file)? {
                return Ok(StopResponse::ContinueTurn {
                    class: StopContinuationClass::Recovery,
                    reason: format!(
                        "agent-doc Stop hook opened successor cycle {cycle_id} for operator steering that outlived an empty abandoned predecessor in {disp}. Continue THIS turn and handle the steering by its previously supplied dispatch; wait for any dispatched worker result, then persist the completed answer through the normal agent-doc closeout. Pending steering begins: {prompt:?}. Do not send the final answer yet.",
                        disp = file.display(),
                    ),
                });
            }
            // `#stopterminalreapfirst`: a completed-item guard after a committed
            // cycle is residue from that terminal cycle, not proof that Codex's
            // latest chat text belongs to a new response cycle. Reopening here
            // used a predecessor's closing status as the response to an adjacent
            // auto-queue head when that status merely mentioned the head. Finish
            // the idempotent terminal maintenance first and never capture the
            // predecessor payload on this branch.
            if agent_doc_session_check_io::is_completed_pending_reap_interruption(&reason)
                && agent_doc_flow_io::closeout::cycle_already_committed(file).is_some()
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_terminal_reap_maintenance_started capture_skipped=true",
                );
                let closeout_result =
                    agent_doc_closeout_runtime_io::complete_required_closeout(file, false);
                invalidate_stop_document_cache();
                let status = agent_doc_session_check_io::inspect(
                    file,
                    &agent_doc_closeout_runtime_io::session_check_effects(),
                )?;
                let completed_reap_still_pending = matches!(
                    &status,
                    agent_doc_session_check_io::SessionCheckStatus::Interrupted(message)
                        | agent_doc_session_check_io::SessionCheckStatus::SteeringPending(message)
                        if agent_doc_session_check_io::is_completed_pending_reap_interruption(message)
                );
                if !completed_reap_still_pending {
                    let closeout_outcome = if closeout_result.is_ok() {
                        "closed"
                    } else {
                        "advanced_to_followup_guard"
                    };
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "codex_stop_terminal_reap_maintenance_settled capture_skipped=true outcome={closeout_outcome}"
                        ),
                    );

                    // This is a fresh selection boundary even on recursive Stop:
                    // the predecessor text was deliberately excluded above, so
                    // repeated-head recovery must not reinterpret it as an answer.
                    let mut fresh_queue_input = input.clone();
                    fresh_queue_input.stop_hook_active = false;
                    if let Some(response) = auto_queue_continuation_response(
                        file,
                        cleanup_roots,
                        loaded_root,
                        state,
                        &fresh_queue_input,
                    )? {
                        return Ok(response);
                    }

                    return match status {
                        agent_doc_session_check_io::SessionCheckStatus::Ok(_) => apply_stop(input),
                        agent_doc_session_check_io::SessionCheckStatus::Interrupted(message)
                        | agent_doc_session_check_io::SessionCheckStatus::SteeringPending(
                            message,
                        ) => Ok(StopResponse::ContinueTurn {
                            class: StopContinuationClass::Recovery,
                            reason: format!(
                                "agent-doc Stop hook finished terminal completed-item maintenance for {}, but newer work still needs its owning turn. {} The predecessor assistant text was not replayed. Continue in-pane and do not send the final answer yet.",
                                file.display(),
                                message,
                            ),
                        }),
                    };
                }

                let closeout_note = closeout_result
                    .err()
                    .map(|err| format!(" Closeout maintenance reported: {err}."))
                    .unwrap_or_default();
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_terminal_reap_maintenance_still_pending capture_skipped=true",
                );
                let message = format!(
                    "agent-doc Stop hook could not persist terminal completed-item maintenance for {}. {}{} The predecessor assistant text was not captured or replayed. Run `agent-doc repair {}` and then `agent-doc session-check {}`. Do not send the final answer yet.",
                    file.display(),
                    reason,
                    closeout_note,
                    file.display(),
                    file.display(),
                );
                if input.stop_hook_active {
                    return Ok(StopResponse::RefuseExit { reason: message });
                }
                return Ok(StopResponse::ContinueTurn {
                    class: StopContinuationClass::Recovery,
                    reason: message,
                });
            }
            // A durable retained intent whose endpoint explicitly refused is
            // no longer an automatic-retry state. On the first Stop, hand the
            // exact operator remedy back to the agent so it can report the
            // blocker. On the recursive Stop after that report, allow the
            // answer through; repeating the same stop veto cannot create the
            // editor endpoint transition the remedy requires.
            if retained_closeout_needs_operator(&reason) {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_retained_closeout_needs_operator action=report_operator_blocker",
                );
                if input.stop_hook_active {
                    return Ok(StopResponse::AllowExit);
                }
                return Ok(StopResponse::ContinueTurn {
                    class: StopContinuationClass::Recovery,
                    reason: format!(
                        "agent-doc Stop hook found a retained closeout that requires operator action for {}. {} Report this exact blocker and its recovery to the operator; do not resend or recapture the response, force disk, or keep retrying session-check. After reporting it, the recursive Stop may finish this turn while the durable capture remains retained.",
                        file.display(),
                        reason,
                    ),
                });
            }
            // `#binaryownedfinalize`: once the response is durably captured, the
            // Stop hook is a status gate, not a request for another agent-authored
            // finalize attempt. Give the binary's keyed repair/commit operation a
            // bounded opportunity to finish through editor/CRDT authority. The
            // route-owned supervisor continues the same operation after this hook
            // returns if convergence takes longer.
            let binary_owned_closeout_pending = is_binary_owned_closeout_interruption(&reason);
            // The hook executes the freshly-installed binary even when the
            // route-owned supervisor still has an older inode. Resume the
            // existing keyed capture here as the version-independent liveness
            // boundary; strict repair preserves editor authority and never
            // recaptures or elects force-disk.
            match try_resume_captured_finalize_in_hook(file) {
                CapturedFinalizeHookResume::Closed => return apply_stop(input),
                CapturedFinalizeHookResume::NeedsOperator { reason } => {
                    return Ok(captured_finalize_needs_operator_response(
                        file,
                        &reason,
                        input.stop_hook_active,
                    ));
                }
                CapturedFinalizeHookResume::Pending => {}
            }
            if binary_owned_closeout_pending {
                agent_doc_ops_log_io::log_op(file, "codex_stop_binary_owned_closeout_pending");
                let display = file.display();
                let message = format!(
                    "agent-doc Stop hook found a binary-owned closeout still converging for {display}. {reason} The captured response is retained and the agent-doc binary/supervisor owns the keyed editor/CRDT retry and terminal commit. Do not recapture the response, rerun finalize, kill the controller, or use `--force-disk`; only re-check session status after the binary reports recovery, unless it explicitly reports `needs_operator`. Do not send the final answer yet."
                );
                if input.stop_hook_active {
                    return Ok(StopResponse::RefuseExit { reason: message });
                }
                return Ok(StopResponse::ContinueTurn {
                    class: StopContinuationClass::Recovery,
                    reason: message,
                });
            }
            // A recursive Stop normally acts only as a status gate. The one
            // exception is fresh prompt work sitting behind a committed
            // predecessor: there is no open cycle to duplicate, and telling
            // the agent to run `finalize` is impossible because finalize must
            // reject that terminal predecessor. Reuse the same binary-owned
            // reopen/capture/closeout path as the first Stop invocation.
            // `#fpestopreplay`: a committed cycle that already settled THIS turn's
            // prompt owns the turn's answer. The unresolved prompt diff session-check
            // reports is newer work (typically a recurring queue head the committed
            // response already answered once), and Codex's closing chat message is a
            // restatement of the committed answer, not an answer to that diff.
            // Reopening a cycle to capture it replayed console status into the
            // document as operator prompts (src/haiven-dev/tasks/fpe.md,
            // 2026-09-28). Neither reopen nor capture; direct the agent to answer the
            // fresh diff in-pane instead.
            if is_committed_prompt_diff_interruption(&reason)
                && agent_doc_flow_io::closeout::cycle_already_committed(file).is_some()
                && committed_cycle_settled_prompt_debt(file, state)?
                && let Some(response) =
                    committed_prompt_diff_stop_response(file, &reason, input.stop_hook_active)?
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_post_commit_replay_skipped file={} reason=committed_cycle_settled_turn_prompt stop_hook_active={}",
                        file.display(),
                        input.stop_hook_active,
                    ),
                );
                return Ok(response);
            }
            let recursive_post_commit_prompt = input.stop_hook_active
                && is_committed_prompt_diff_interruption(&reason)
                && agent_doc_flow_io::closeout::cycle_already_committed(file).is_some()
                && matches!(
                    agent_doc_template::replay_guard::classify_replay_payload(
                        &input.last_assistant_message
                    ),
                    agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
                );
            let recursive_steering_successor =
                input.stop_hook_active && stop_steering_continuation_cycle_is_open(file)?;
            let mut payload_already_committed = false;
            if !input.stop_hook_active
                || recursive_post_commit_prompt
                || recursive_steering_successor
            {
                let stop_closeout = match attempt_stop_closeout(file, state, input) {
                    Ok(stop_closeout) => stop_closeout,
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!("codex_stop_auto_close_failed err={err}"),
                        );
                        return Ok(StopResponse::ContinueTurn {
                            class: StopContinuationClass::Recovery,
                            reason: format!(
                                "agent-doc Stop hook intercepted an unfinished document cycle for {}. The hook wrote or recovered the response but could not finish the required commit boundary: {err}. Do not send the final answer yet. Finish the commit boundary for this turn with `agent-doc commit {}` and end with `agent-doc session-check {}`.",
                                file.display(),
                                file.display(),
                                file.display()
                            ),
                        });
                    }
                };
                match stop_closeout {
                    StopCloseAttempt::Closed => {
                        if recursive_steering_successor {
                            agent_doc_ops_log_io::log_op(
                                file,
                                "codex_stop_steering_continuation_auto_closed source=recursive_stop",
                            );
                        }
                        if recursive_post_commit_prompt {
                            agent_doc_ops_log_io::log_op(
                                file,
                                "codex_stop_post_commit_prompt_auto_closed source=recursive_stop",
                            );
                        }
                        if let Some(response) = auto_queue_continuation_response(
                            file,
                            cleanup_roots,
                            loaded_root,
                            state,
                            input,
                        )? {
                            return Ok(response);
                        }
                        settle_session_binding(file, cleanup_roots, loaded_root, state)?;
                        return Ok(StopResponse::AllowExit);
                    }
                    StopCloseAttempt::AlreadyCommitted => {
                        // `#fixlazilycodex`: the closing text is the committed
                        // closeout's own; it answers nothing new, so it is
                        // never recaptured. Session-check still reports work
                        // behind the commit: fresh prompt work is handed back
                        // in-pane (the `#fpestopreplay` response); any other
                        // interruption falls through to the plain hand-back
                        // below with capture skipped.
                        if is_committed_prompt_diff_interruption(&reason)
                            && let Some(response) = committed_prompt_diff_stop_response(
                                file,
                                &reason,
                                input.stop_hook_active,
                            )?
                        {
                            return Ok(response);
                        }
                        payload_already_committed = true;
                    }
                    StopCloseAttempt::UnansweredEditPending => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            "codex_stop_unanswered_edit_pending action=allow_exit_next_cycle (#stopnextedit)",
                        );
                        return Ok(StopResponse::AllowExit);
                    }
                    StopCloseAttempt::DeferredToDurableOwner => {
                        return Ok(StopResponse::AllowExit);
                    }
                    StopCloseAttempt::RepairDeferredToDurableOwner { note } => {
                        return await_durable_owner_repair_deferral(file, &note, input);
                    }
                    StopCloseAttempt::StillOpen { note } => {
                        return Ok(StopResponse::ContinueTurn {
                            class: StopContinuationClass::Recovery,
                            reason: format!(
                                "agent-doc Stop hook intercepted an unfinished document cycle for {}. {}{} Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair {}` first. Then finish the commit boundary for this turn and end with `agent-doc session-check {}`.",
                                file.display(),
                                reason,
                                note,
                                file.display(),
                                file.display()
                            ),
                        });
                    }
                    StopCloseAttempt::NotPossible => {}
                }
            }

            let capture_note = if input.stop_hook_active || payload_already_committed {
                String::new()
            } else {
                capture_assistant_text(file, state, input)
            };
            let display = file.display();
            if input.stop_hook_active {
                if let Some(response) =
                    committed_prompt_diff_stop_response(file, &reason, input.stop_hook_active)?
                {
                    return Ok(response);
                }
                return Ok(StopResponse::RefuseExit {
                    reason: format!(
                        "agent-doc Stop hook already continued once for {display}, but the cycle is still open. {reason}{capture_note}"
                    ),
                });
            }
            Ok(StopResponse::ContinueTurn {
                class: StopContinuationClass::Recovery,
                reason: format!(
                    "agent-doc Stop hook intercepted an unfinished document cycle for {display}. {reason}{capture_note} Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair {display}` first. Then finish the commit boundary for this turn and end with `agent-doc session-check {display}`."
                ),
            })
        }
    }
}

fn try_resume_captured_finalize_in_hook(file: &Path) -> CapturedFinalizeHookResume {
    invalidate_stop_document_cache();
    let Some(key) = agent_doc_repair_command_io::captured_finalize_resume_key(file)
        .ok()
        .flatten()
    else {
        return CapturedFinalizeHookResume::Pending;
    };
    // Stop is a status gate, not the retry owner. The supervisor reacts to the
    // retained state edge after this single opportunistic attempt.
    const MAX_ATTEMPTS: u32 = 1;
    for attempt in 1..=MAX_ATTEMPTS {
        match agent_doc_repair_command_io::resume_captured_finalize(file, &key) {
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Committed { .. } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_committed cycle_id={} capture_id={} response_sha256={} attempt={} authority=editor_crdt",
                        key.cycle_id, key.capture_id, key.response_sha256, attempt,
                    ),
                );
                return CapturedFinalizeHookResume::Closed;
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::ClosedIgnoredPath => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_closed_ignored_path cycle_id={} capture_id={} response_sha256={} attempt={} outcome=terminal_without_commit",
                        key.cycle_id, key.capture_id, key.response_sha256, attempt,
                    ),
                );
                return CapturedFinalizeHookResume::Closed;
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded => {
                return if agent_doc_session_check_io::inspect(
                    file,
                    &agent_doc_closeout_runtime_io::session_check_effects(),
                )
                .is_ok_and(|status| !status.is_failure())
                {
                    CapturedFinalizeHookResume::Closed
                } else {
                    CapturedFinalizeHookResume::Pending
                };
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::WaitingForSignal {
                reason,
            } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_waiting_for_state cycle_id={} capture_id={} response_sha256={} attempt={} reason_bytes={} action=await_controller_state_edge",
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        attempt,
                        reason.len(),
                    ),
                );
                return CapturedFinalizeHookResume::Pending;
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::RetryAt {
                reason,
                retry_at_secs,
            } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_lease_retry cycle_id={} capture_id={} response_sha256={} attempt={} retry_at_secs={} reason_bytes={} action=await_supervisor_timer_edge",
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        attempt,
                        retry_at_secs,
                        reason.len(),
                    ),
                );
                return CapturedFinalizeHookResume::Pending;
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::RetryableEffect {
                reason,
            } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_retry cycle_id={} capture_id={} response_sha256={} attempt={} reason_bytes={} action=retry_without_disk_write",
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        attempt,
                        reason.len(),
                    ),
                );
                if attempt < MAX_ATTEMPTS {
                    std::thread::sleep(
                        agent_doc_supervisor::idle_watch::captured_finalize_resume_retry_delay(
                            attempt,
                        ),
                    );
                }
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Unlandable {
                reason,
                recovery,
            } => {
                // GH 90: deterministic refusal; retrying inside the Stop hook
                // cannot change it. Surface the reason and the recovery once.
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_unlandable cycle_id={} capture_id={} response_sha256={} reason_bytes={} action=stop_retrying recovery=\"{recovery}\"",
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        reason.len(),
                    ),
                );
                eprintln!("[agent-doc] {reason}");
                return CapturedFinalizeHookResume::NeedsOperator {
                    reason: format!("{reason} Recovery: {recovery}."),
                };
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::NeedsOperator {
                reason,
            } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_captured_finalize_resume_needs_operator cycle_id={} capture_id={} response_sha256={} reason_bytes={} action=retain_without_mutation",
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        reason.len(),
                    ),
                );
                return CapturedFinalizeHookResume::NeedsOperator { reason };
            }
        }
    }
    CapturedFinalizeHookResume::Pending
}

fn is_binary_owned_closeout_interruption(reason: &str) -> bool {
    !retained_closeout_needs_operator(reason)
        && (reason.contains("closeout blocked by `editor_convergence_required`")
            || (reason.contains("editor_convergence_required")
                && reason.contains("operator_text_authority_v1"))
            || (reason.contains("binary-owned response delivery `")
                && reason.contains(" is retained for `")
                && reason.contains("Same-capture recovery remains pending"))
            || (reason.contains("cycle `")
                && reason.contains(" is still `write_applied`")
                && reason.contains("response write landed but no terminal commit followed")))
}

fn retained_closeout_needs_operator(reason: &str) -> bool {
    reason.contains(agent_doc_session_check_io::RETAINED_PENDING_NEEDS_OPERATOR_TOKEN)
}

/// Open the successor owned by a recursive Stop when its predecessor was an
/// empty preflight abandoned after the first Stop returned.
///
/// This is deliberately derived from typed cycle and steering observations,
/// not from the human-readable `session-check` string. Only the exact
/// `cancel_preflight_cycle_abandoned` shape qualifies; an abandoned cycle with
/// a capture, or one without unresolved steering, keeps the ordinary bounded
/// Stop behavior.
fn reopen_abandoned_cycle_for_pending_steering(file: &Path) -> Result<Option<(String, String)>> {
    let Some(cycle) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(None);
    };
    if cycle.phase != agent_doc_turn::CyclePhase::Abandoned
        || cycle.last_event != "cancel_preflight_cycle_abandoned"
        || cycle.capture_id.is_some()
        || cycle.response_sha256.is_some()
    {
        return Ok(None);
    }

    let current = current_document_content(file, "codex_stop_abandoned_steering_successor")?;
    let items =
        agent_doc_session_check_io::midturn_steering::pending_steering_items(file, &current);
    let Some(first) = items.first() else {
        return Ok(None);
    };
    let head = agent_doc_git_io::revision::show_head(file)?.ok_or_else(|| {
        anyhow::anyhow!(
            "cannot open a Stop steering successor for {} without a committed HEAD baseline",
            file.display()
        )
    })?;
    let opened = agent_doc_cycle_state_io::start_stop_steering_continuation_preflight(
        file,
        Some(&head),
        Some(&current),
    )?;
    anyhow::ensure!(
        opened.last_event == agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT,
        "Stop steering successor {} lost its durable origin event",
        opened.cycle_id
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_abandoned_steering_successor_opened previous_cycle_id={} cycle_id={} items={} source=typed_pending_steering",
            cycle.cycle_id,
            opened.cycle_id,
            items.len(),
        ),
    );
    Ok(Some((
        opened.cycle_id,
        first_nonempty_prompt_line(&first.verbatim),
    )))
}

fn stop_steering_continuation_cycle_is_open(file: &Path) -> Result<bool> {
    Ok(agent_doc_cycle_state_io::load(file)?.is_some_and(|cycle| {
        cycle.phase == agent_doc_turn::CyclePhase::PreflightStarted
            && cycle.last_event == agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT
            && cycle.capture_id.is_none()
            && cycle.response_sha256.is_none()
    }))
}

fn committed_prompt_diff_stop_response(
    file: &Path,
    reason: &str,
    stop_hook_active: bool,
) -> Result<Option<StopResponse>> {
    committed_prompt_diff_stop_response_with(
        file,
        reason,
        stop_hook_active,
        agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc,
    )
}

fn committed_prompt_diff_stop_response_with(
    file: &Path,
    reason: &str,
    stop_hook_active: bool,
    drain_readiness: impl FnOnce(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Result<Option<StopResponse>> {
    if !is_committed_prompt_diff_interruption(reason) {
        return Ok(None);
    }
    // `#claimedstopforward`: the shared steering classifier has already
    // decided who owns each prompt-bearing edit. A queue edit routed to a live
    // claim owner is not fresh work for the coordinator merely because the
    // predecessor cycle committed before Stop observed the edit. Recompute the
    // typed items from current document authority so this path agrees with the
    // mid-turn hook; if every item is forwarded, let the coordinator stop.
    // Mixed edits still re-enter for the first item that has no external owner.
    let current = current_document_content(file, "codex_stop_committed_prompt_diff")?;
    let pending =
        agent_doc_session_check_io::midturn_steering::pending_steering_items(file, &current);
    let coordinator_item = pending
        .iter()
        .find(|item| item.dispatch.as_str() != "forward_to_owner");
    let coordinator_prompt = coordinator_item.map(|item| item.verbatim.clone());
    let disp = file.display().to_string();
    // `#reportedrelease`: a queue line whose worker already reported is not
    // fresh work to answer; its result exists only in this turn's context.
    // Name the exact persistence (the closeout that strikes the line) so the
    // agent neither re-dispatches it nor persists a heading-only answer that
    // leaves the line queued to be re-served.
    if let Some(item) = coordinator_item
        && item.dispatch.as_str() == "persist_reported"
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_committed_prompt_diff_reported_worker_unpersisted owner={} item_bytes={} (#reportedrelease)",
                item.owner.as_deref().unwrap_or("-"),
                item.verbatim.trim().len(),
            ),
        );
        return Ok(Some(StopResponse::ContinueTurn {
            class: StopContinuationClass::SessionWriteback,
            reason: format!(
                "agent-doc Stop hook found a reported worker result for {disp} that was never persisted. {} Do NOT send the final answer yet.",
                agent_doc_queue::claim_release_receipt::persist_reported_instruction(
                    &disp,
                    &item.verbatim,
                    item.owner.as_deref().unwrap_or("its worker"),
                ),
            ),
        }));
    }
    let queue_close = coordinator_item
        .filter(|item| !agent_doc_session_check_io::midturn_steering::is_exchange_item(item))
        .map(|item| {
            format!(
                " It is a queue line: {}; that closeout strikes it.",
                agent_doc_queue::claim_release_receipt::queue_line_closeout(&disp, &item.verbatim)
            )
        })
        .unwrap_or_default();
    if !pending.is_empty() && coordinator_prompt.is_none() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_committed_prompt_diff_forwarded_to_claim_owners items={} action=continue (#claimedstopforward)",
                pending.len(),
            ),
        );
        return Ok(Some(StopResponse::AllowExit));
    }
    // `#queuetypingsteer`: a `content_edit` is the operator's own edit inside an
    // existing queue item or prompt; name that edit, not some other prompt the
    // exchange happens to hold.
    let steering_is_content_edit = reason.find("content_edit:").is_some_and(|at| {
        reason
            .find("prompt_target:")
            .is_none_or(|target| at < target)
    });
    let prompt = if coordinator_prompt.is_some() {
        coordinator_prompt
    } else if steering_is_content_edit {
        prompt_target_from_interruption_reason(reason)
    } else {
        agent_doc_session_check_io::unresolved_exchange_prompt(file)?
            .or_else(|| prompt_target_from_interruption_reason(reason))
    }
    .unwrap_or_else(|| "the unresolved exchange prompt".to_string());
    // `#steeringwake`: session-check can surface the same fresh post-commit
    // steering as `SteeringPending` instead of the clean-session path handled
    // by `active_session_prompt_requires_writeback_with`. Preserve the same
    // receiver-first contract here: on the first Stop, a ready supervisor owns
    // the reactive idle wake, so do not emit Codex's `Blocked by hook` wire
    // response. A recursive Stop has already handed this work in-pane and must
    // retain that answer path rather than yielding it away.
    if !stop_hook_active
        && let Some(previous_cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file)
    {
        let readiness = drain_readiness(file);
        if let agent_doc_controller::status::SupervisorDrainReadiness::Ready { supervisor_pid } =
            readiness
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_post_commit_prompt_supervisor_yield previous_cycle_id={} supervisor_pid={} prompt_bytes={} prompt_sha256={} successor_opened=false capture_skipped=true action=allow_exit source=committed_prompt_diff (#steeringwake)",
                    previous_cycle_id,
                    supervisor_pid,
                    prompt.len(),
                    agent_doc_hash::content_hash(&prompt),
                ),
            );
            return Ok(Some(StopResponse::AllowExit));
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_post_commit_prompt_supervisor_yield_unavailable previous_cycle_id={} supervisor_drain_readiness={} supervisor_pid={} action=continue_in_pane source=committed_prompt_diff (#steeringwake)",
                previous_cycle_id,
                readiness.reason(),
                readiness
                    .supervisor_pid()
                    .map_or_else(|| "-".to_string(), |pid| pid.to_string()),
            ),
        );
    }
    Ok(Some(StopResponse::ContinueTurn {
        class: StopContinuationClass::SessionWriteback,
        reason: format!(
            "agent-doc Stop hook found fresh unresolved exchange work for {disp} after the previous cycle was already committed. Continue THIS turn in-pane: answer {prompt:?} in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`).{queue_close} Do NOT send the final answer yet.",
            prompt = first_nonempty_prompt_line(&prompt),
        ),
    }))
}

fn active_session_prompt_requires_writeback(
    file: &Path,
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<Option<StopResponse>> {
    active_session_prompt_requires_writeback_with(
        file,
        roots,
        loaded_root,
        state,
        input,
        agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc,
    )
}

/// [`active_session_prompt_requires_writeback`] with the supervisor readiness
/// probe injected.
fn active_session_prompt_requires_writeback_with(
    file: &Path,
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
    input: &StopInput,
    drain_readiness: impl FnOnce(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Result<Option<StopResponse>> {
    let prompt = match active_session_prompt_or_queue_head(file)? {
        Some(prompt) => prompt,
        None => {
            let Some(prompt) = prompt_writeback_debt(state, &input.turn_id) else {
                return Ok(None);
            };
            if committed_cycle_settled_prompt_debt(file, state)? {
                return Ok(None);
            }
            let content = current_document_content(file, "codex_stop_same_turn_prompt_debt")?;
            if agent_doc_turn::closeout_signal::exchange_contains_prompt_line(&content, prompt) {
                return Ok(None);
            }
            prompt.to_string()
        }
    };

    // `#postcommitpromptownership`: the first Stop after a committed cycle can
    // observe a queue item that appeared after that cycle while Codex still
    // supplies the predecessor turn's closing chat summary. Textual novelty is
    // not proof that the summary answers the new item: the committed cycle's
    // prompt binding is the durable ownership fact. A ready supervisor already
    // owns a reactive idle steering wake for this document, so let Codex finish
    // this turn without emitting its necessarily `Blocked by hook` Stop wire
    // response. Crucially, do this BEFORE opening the successor: its baseline
    // would otherwise absorb the new prompt and hide it from the steering wake.
    // Without a proven receiver, retain the bounded in-pane successor fallback
    // so the prompt cannot be stranded.
    if !input.stop_hook_active
        && let Some(previous_cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file)
        && committed_cycle_settled_prompt_debt(file, state)?
    {
        let readiness = drain_readiness(file);
        if let agent_doc_controller::status::SupervisorDrainReadiness::Ready { supervisor_pid } =
            readiness
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_post_commit_prompt_supervisor_yield previous_cycle_id={} supervisor_pid={} prompt_bytes={} prompt_sha256={} successor_opened=false capture_skipped=true action=allow_exit source=settled_prompt_debt (#steeringwake)",
                    previous_cycle_id,
                    supervisor_pid,
                    prompt.len(),
                    agent_doc_hash::content_hash(&prompt),
                ),
            );
            return Ok(Some(StopResponse::AllowExit));
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_post_commit_prompt_supervisor_yield_unavailable previous_cycle_id={} supervisor_drain_readiness={} supervisor_pid={} action=open_successor source=settled_prompt_debt (#steeringwake)",
                previous_cycle_id,
                readiness.reason(),
                readiness
                    .supervisor_pid()
                    .map_or_else(|| "-".to_string(), |pid| pid.to_string()),
            ),
        );
        let current = current_document_content(file, "codex_stop_post_commit_prompt_successor")?;
        let head = agent_doc_git_io::revision::show_head(file)?.ok_or_else(|| {
            anyhow::anyhow!(
                "cannot open a post-commit Stop successor for {} without a committed HEAD baseline",
                file.display()
            )
        })?;
        let opened = agent_doc_cycle_state_io::start_stop_steering_continuation_preflight(
            file,
            Some(&head),
            Some(&current),
        )?;
        anyhow::ensure!(
            opened.last_event == agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT,
            "post-commit Stop successor {} lost its durable origin event",
            opened.cycle_id
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_post_commit_prompt_successor_opened previous_cycle_id={} cycle_id={} capture_skipped=true source=settled_prompt_debt",
                previous_cycle_id, opened.cycle_id,
            ),
        );
        return Ok(Some(StopResponse::ContinueTurn {
            class: StopContinuationClass::SessionWriteback,
            reason: format!(
                "agent-doc Stop hook opened successor cycle {cycle_id} for fresh post-commit session work in {disp}. The previous closing assistant text belongs to committed predecessor {previous_cycle_id} and was deliberately not captured. Active prompt: {prompt:?}. Continue THIS turn in-pane, answer that prompt, and persist through the normal agent-doc closeout. Do not send the final answer yet.",
                cycle_id = opened.cycle_id,
                disp = file.display(),
                prompt = first_nonempty_prompt_line(&prompt),
            ),
        }));
    }

    // `#stopwritebackfirst`: prompt debt plus a replayable answer is enough to
    // attempt the binary-owned closeout. Requiring an already-committed
    // predecessor here left an admitted/open cycle on the slower fallback:
    // Codex first rendered the answer, then rendered the Stop continuation as
    // `Blocked by hook`, and only the continuation could call finalize. The
    // closeout routine already proves capture, editor delivery, and commit, so
    // attempt it before exposing any continuation wire response.
    let mut payload_already_committed = false;
    if matches!(
        agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message),
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
    ) {
        match attempt_stop_closeout(file, state, input)? {
            // `#fixlazilycodex`: the closing text is already committed, so it
            // is not this prompt's answer and must not be recaptured. The
            // prompt is still real session work; hand it back in-pane below.
            StopCloseAttempt::AlreadyCommitted => payload_already_committed = true,
            StopCloseAttempt::Closed => {
                settle_session_binding(file, roots, loaded_root, state)?;
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_prompt_writeback_auto_closed source=exact_thread_prompt_debt",
                );
                return Ok(Some(StopResponse::AllowExit));
            }
            StopCloseAttempt::UnansweredEditPending => {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_prompt_writeback_next_cycle action=allow_exit (#stopnextedit)",
                );
                return Ok(Some(StopResponse::AllowExit));
            }
            StopCloseAttempt::DeferredToDurableOwner => {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_prompt_writeback_closeout_deferred owner=durable_retained_capture",
                );
                return Ok(Some(StopResponse::AllowExit));
            }
            StopCloseAttempt::RepairDeferredToDurableOwner { note } => {
                return await_durable_owner_repair_deferral(file, &note, input).map(Some);
            }
            StopCloseAttempt::StillOpen { note } => {
                return Ok(Some(StopResponse::ContinueTurn {
                    class: StopContinuationClass::SessionWriteback,
                    reason: format!(
                        "agent-doc Stop hook attempted binary-owned closeout for session prompt work in {disp}, retained the response under that cycle, but could not finish closeout.{note} Do not recapture the response or rerun finalize/write. Run `agent-doc session-check {disp}` and let the binary-owned keyed retry finish.",
                        disp = file.display(),
                    ),
                }));
            }
            StopCloseAttempt::NotPossible => {}
        }
    }
    let capture_note = if input.stop_hook_active || payload_already_committed {
        String::new()
    } else {
        capture_assistant_text(file, state, input)
    };
    Ok(Some(StopResponse::ContinueTurn {
        class: StopContinuationClass::SessionWriteback,
        reason: format!(
            "agent-doc Stop hook found active session-document work for {disp} that has not crossed the binary-owned write boundary. Active prompt: {prompt:?}. Continue THIS turn in-pane and persist with `agent-doc finalize {disp}` or `agent-doc write --commit {disp}`, then run `agent-doc session-check {disp}`. Do not send the final answer yet.{capture_note}",
            disp = file.display(),
            prompt = first_nonempty_prompt_line(&prompt),
        ),
    }))
}

fn committed_cycle_settled_prompt_debt(file: &Path, state: &SessionState) -> Result<bool> {
    let Some(cycle) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(false);
    };
    if cycle.phase.as_str() != "committed" || cycle.response_sha256.is_none() {
        return Ok(false);
    }
    if let Some(observed) = state.last_prompt_cycle.as_ref() {
        if observed.cycle_id == cycle.cycle_id {
            return Ok(observed.was_open);
        }
        // The prompt may have been admitted into a later cycle after the hook
        // observed a terminal predecessor. A committed response from that later
        // cycle settles the debt when its entire lifecycle is newer than the
        // prompt binding. Do not pin the debt forever to the predecessor id.
        return Ok(state.updated_at <= cycle.started_at && state.updated_at < cycle.updated_at);
    }

    // Backward-compatible proof for bindings written before
    // `last_prompt_cycle` existed. Strict inequalities avoid treating a prompt
    // registered at or after the terminal event as settled.
    Ok(cycle.started_at <= state.updated_at && state.updated_at < cycle.updated_at)
}

fn park_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
) -> Result<()> {
    let parked = parked_session_state(state, now_secs());
    save_state_across_roots(roots, loaded_root, &parked)
}

fn settle_session_binding(
    file: &Path,
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
) -> Result<()> {
    let queue_requests_clear = document_queue_requests_clear(file)?;
    if agent_doc_codex_hook_io::prompt_requests_clear(&state.last_prompt) || queue_requests_clear {
        clear_state_across_roots(roots, loaded_root, &state.session_id)
    } else {
        park_state_across_roots(roots, loaded_root, state)
    }
}

fn document_queue_requests_clear(file: &Path) -> Result<bool> {
    stop_phase("document_queue_requests_clear");
    if active_auto_queue_prompt(file)?
        .as_deref()
        .is_some_and(agent_doc_codex_hook_io::prompt_requests_clear)
    {
        return Ok(true);
    }
    let content = current_document_content(file, "codex_stop_context_clear_queue")?;
    let Ok(components) = agent_doc_element::element::parse(&content) else {
        return Ok(false);
    };
    Ok(components
        .iter()
        .find(|component| component.name == "queue")
        .is_some_and(|queue| {
            agent_doc_codex_hook_io::prompt_requests_clear(queue.content(&content))
        }))
}

fn active_session_prompt_or_queue_head(file: &Path) -> Result<Option<String>> {
    stop_phase("active_session_prompt_or_queue_head");
    // One authoritative materialization answers both questions. Calling the
    // session-check file-level helper first used a separate realtime resolver,
    // then `current_document_content` repeated the same controller/CRDT round
    // trip for the queue. Under closeout contention those serial reads consumed
    // the Stop hook's entire 45-second budget.
    let content = current_document_content(file, "codex_stop_active_session_prompt_or_queue_head")?;
    if let Some(prompt) =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&content)
    {
        return Ok(Some(prompt));
    }
    // `#fixlazilycodex`: a head whose worker is still dispatching/running (or
    // the queue-level `subagents` plan holds back) is not this session's prompt
    // debt. A `Reported` claim is different: the worker has finished and its
    // result is now coordinator writeback debt. Keeping it visible here stops
    // a post-commit coordinator exit from stranding a ☑️ row in the outbox.
    let in_flight =
        agent_doc_queue_io::queue_claim::in_flight_claimed_items_for_content(file, &content);
    // GH #381: a head naming a `.agent-doc/project.md` preset is prompt debt
    // exactly like a document-local preset head.
    let presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(&content, file);
    Ok(first_active_queue_prompt_in_content(
        &content, &in_flight, &presets,
    ))
}

fn first_active_queue_prompt_in_content(
    content: &str,
    claimed: &agent_doc_queue::queue_claim::ClaimedQueueItems,
    presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Option<String> {
    let prompt =
        agent_doc_queue::queue_continuation::pending_head_prompt_text_excluding_claimed_with_presets(
            content,
            agent_doc_queue::queue_continuation::DrainScope::InSessionLoop,
            claimed,
            presets,
        )?;
    if agent_doc_codex_hook_io::is_context_clear_prompt(&prompt)
        || agent_doc_queue::queue_command::slash_command_text(&prompt).is_some()
    {
        return None;
    }
    Some(prompt)
}

/// `#bgclearpolicy`: hand the item boundary to the supervisor's queue
/// auto-clear instead of continuing in this Codex context.
///
/// Returns the allow-stop response when the queue auto-clears (no
/// `disable-auto-clear`) and the document's supervisor can take the hand-off;
/// `None` keeps the in-pane continuation, which is also the fallback whenever
/// the supervisor is missing, stale, or still owes an earlier hand-off — a
/// yield nobody receives would stall the queue. An unreadable document keeps
/// the in-pane continuation for the same reason.
fn codex_stop_auto_clear_yield(file: &Path, prompt: &str, source: &str) -> Option<StopResponse> {
    codex_stop_auto_clear_yield_with(
        file,
        prompt,
        source,
        agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc,
    )
}

/// [`codex_stop_auto_clear_yield`] with the supervisor readiness probe injected.
fn codex_stop_auto_clear_yield_with(
    file: &Path,
    prompt: &str,
    source: &str,
    drain_readiness: impl FnOnce(&Path) -> agent_doc_controller::status::SupervisorDrainReadiness,
) -> Option<StopResponse> {
    let content = resolve_document_content_uncached(file, "codex_stop_queue_auto_clear").ok()?;
    let decision = agent_doc_queue_io::queue_auto_clear::in_session_auto_clear_yield_with(
        &content,
        true,
        || drain_readiness(file),
        || {
            agent_doc_queue_io::queue_auto_clear::undrained_supervisor_drain_handoff_age(
                file, &content,
            )
            .is_some()
        },
    );
    if !decision.yields {
        if decision.enabled {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_queue_auto_clear_kept_in_pane file={} source={} supervisor_drain_readiness={} handoff_overdue={} (#bgclearpolicy)",
                    file.display(),
                    source,
                    decision.readiness_reason(),
                    decision.handoff_overdue,
                ),
            );
        }
        return None;
    }
    agent_doc_queue_io::queue_auto_clear::record_supervisor_drain_handoff(file, &content);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_queue_continuation_yield file={} source={} result=supervisor_auto_clear supervisor_drain_readiness={} prompt_bytes={} prompt_sha256={} (#bgclearpolicy)",
            file.display(),
            source,
            decision.readiness_reason(),
            prompt.len(),
            agent_doc_hash::content_hash(prompt),
        ),
    );
    Some(StopResponse::AllowExit)
}

/// With queue auto-clear off (`disable-auto-clear`) or no ready supervisor, a
/// reset reason (session accretion, recent compaction, or Codex ctx% over
/// threshold) never changes the in-pane continuation. It is recorded in ops.log
/// only: the agent cannot act on it and Codex shows the Stop payload to the
/// operator verbatim (`#codexstopterse`).
fn log_background_context_clear_suppressed(
    file: &Path,
    prompt: &str,
    source: &str,
    last_context_clear_at: Option<u64>,
) {
    if let Some(reason) =
        agent_doc_codex_hook_io::codex_continuation_clear_reason(file, last_context_clear_at)
    {
        agent_doc_codex_hook_io::log_codex_background_context_clear_suppressed(
            file, prompt, source, &reason,
        );
    }
}

enum RepeatedQueueHeadRecovery {
    Recovered { note: String },
    NotRecoverable { note: String },
}

fn response_has_patch_markers(response: &str) -> bool {
    response.contains("<!-- patch:") || response.contains("<!-- /patch:")
}

fn response_has_response_heading(response: &str) -> bool {
    response
        .lines()
        .any(|line| line.trim_start().starts_with("### Re:"))
}

fn wrap_repeated_queue_response_patch(prompt: &str, response: &str) -> String {
    let heading = prompt
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("active queue head");
    let mut patch = format!("<!-- patch:exchange -->\n### Re: {heading} — gpt-5\n\n");
    patch.push_str(
        &agent_doc_queue::queue_response::format_consumed_prompt_echo(&[prompt.to_string()], None),
    );
    patch.push('\n');
    patch.push_str(response.trim());
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str("<!-- /patch:exchange -->\n");
    patch
}

fn consume_recovered_queue_head(
    file: &Path,
    expected_head: &str,
    queue_completion_ids: &[String],
) -> Result<Option<queue_consume::QueueConsumptionOutcome>> {
    let force_disk_without_listener =
        !agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file);
    queue_consume::consume_queue_prompt_if_head_matches_with_outcome(
        file,
        expected_head,
        queue_completion_ids,
        force_disk_without_listener,
        &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
    )
}

fn repeated_queue_response_for_write(
    file: &Path,
    prompt: &str,
    response: &str,
) -> Result<std::result::Result<String, String>> {
    if response_explicitly_targets_current_queue_head(
        file,
        response,
        "codex_stop_repeated_queue_response",
    )? {
        return Ok(Ok(response.to_string()));
    }
    if response_has_patch_markers(response) || response_has_response_heading(response) {
        return Ok(Err(format!(
            "it already contained a patch block or `### Re:` heading, but that heading did not target the active queue head {prompt:?}"
        )));
    }
    Ok(Ok(wrap_repeated_queue_response_patch(prompt, response)))
}

fn try_recover_repeated_queue_head_response(
    file: &Path,
    prompt: &str,
    input: &StopInput,
    last_prompt: Option<&str>,
) -> Result<RepeatedQueueHeadRecovery> {
    let payload =
        agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let response = match payload {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
            return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                note: capture_missing_stop_response(file, last_prompt),
            });
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                note: capture_blocked_stop_payload(
                    file,
                    &input.last_assistant_message,
                    &reason,
                    last_prompt,
                ),
            });
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            response
        }
    };

    let response_to_write =
        match repeated_queue_response_for_write(file, prompt, response.as_ref())? {
            Ok(response) => response,
            Err(reason) => {
                return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                    note: capture_blocked_stop_payload(
                        file,
                        &input.last_assistant_message,
                        &reason,
                        last_prompt,
                    ),
                });
            }
        };
    let content_before_repair =
        current_document_content(file, "codex_stop_repeated_queue_before_repair")?;
    let queue_completion_ids =
        agent_doc_queue::queue_consume::queue_targeted_completion_id_for_current_head(
            file,
            None,
            &content_before_repair,
            &response_to_write,
            &[],
        )?
        .into_iter()
        .collect::<Vec<_>>();

    stop_phase("repeated_queue_save_pending");
    agent_doc_repair_io::pending::save_pending(file, &response_to_write)?;
    agent_doc_ops_log_io::log_op(file, "codex_stop_repeated_queue_response_saved");
    let mut note = format!(
        " The hook replayed the last assistant response into `agent:exchange` for repeated queue head {:?}.",
        prompt
    );

    stop_phase("repeated_queue_repair");
    let repair_outcome = agent_doc_repair_io::run_with_queue_completion_ids(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        &queue_completion_ids,
    )?;
    // Repair wrote the document; the pre-repair memo is now stale.
    invalidate_stop_document_cache();
    if repair_outcome.replayed_response() {
        note.push_str(" The response was written through the normal repair/write path.");
    } else if repair_outcome == agent_doc_turn::repair::RepairOutcome::AlreadyApplied {
        note.push_str(" The response was already present and was adopted by repair.");
    } else if repair_outcome == agent_doc_turn::repair::RepairOutcome::BlockedCaptureQuarantined
        && active_auto_queue_prompt(file)?.as_deref() != Some(prompt)
    {
        // `#qresiduewedge`: the head's answer was already committed under the
        // same `### Re:` heading, so repair parked this duplicate instead of
        // writing it and struck the answered head. Nothing is left to commit
        // for this head; the next admission's commit carries the strike.
        agent_doc_ops_log_io::log_op(
            file,
            "codex_stop_repeated_queue_committed_heading_replay_parked #qresiduewedge",
        );
        return Ok(RepeatedQueueHeadRecovery::Recovered {
            note: format!(
                " The answer to repeated queue head {prompt:?} was already committed under the same heading, so the hook did not write a duplicate; repair parked the summary and struck the answered head."
            ),
        });
    } else {
        return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
            note: format!(
                "{note} The repair path did not replay or adopt the response (outcome: {repair_outcome:?})."
            ),
        });
    }

    stop_phase("repeated_queue_consume_head");
    if active_auto_queue_prompt(file)?.as_deref() == Some(prompt) {
        match consume_recovered_queue_head(file, prompt, &queue_completion_ids) {
            Ok(Some(outcome)) => {
                note.push_str(&format!(
                    " The hook consumed the completed queue head {:?} before commit.",
                    outcome.consumed_text
                ));
            }
            Ok(None) => {}
            Err(err) => {
                return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                    note: format!(
                        "{note} The hook wrote the response but could not consume the completed queue head: {err}."
                    ),
                });
            }
        }
    }

    if !agent_doc_git_io::status::is_in_git_repo(file) {
        return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
            note: format!(
                "{note} The document is not in a git repository, so the hook could not finish the required commit boundary automatically."
            ),
        });
    }

    stop_phase("repeated_queue_replay_receipt");
    if repair_outcome.replayed_response()
        && agent_doc_flow_io::closeout::replay_closeout_still_proven(
            file,
            &fresh_document_content(file, "codex_stop_queue_replay_terminal_receipt")?,
        )?
    {
        agent_doc_ops_log_io::log_op(
            file,
            "codex_stop_repeated_queue_recovery_success source=strict_replay_receipt",
        );
        return Ok(RepeatedQueueHeadRecovery::Recovered { note });
    }
    stop_phase("repeated_queue_closeout");
    match agent_doc_closeout_runtime_io::complete_required_closeout(file, false) {
        Ok(true) => {
            note.push_str(" The hook finished the commit boundary automatically.");
        }
        Ok(false) => {}
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!("codex_stop_repeated_queue_closeout_failed err={err}"),
            );
            return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                note: format!(
                    "{note} The hook wrote the response but could not finish the required commit boundary: {err}."
                ),
            });
        }
    }

    agent_doc_ops_log_io::log_op(file, "codex_stop_repeated_queue_recovery_success");
    Ok(RepeatedQueueHeadRecovery::Recovered { note })
}

fn repeated_queue_recovery_unavailable_response(
    file: &Path,
    prompt: &str,
    note: &str,
) -> StopResponse {
    StopResponse::ContinueTurn {
        class: StopContinuationClass::Queue,
        reason: format!(
            "agent-doc Stop hook requested auto-queue continuation for {}, but the queue head did not advance after the previous continuation request: {:?}. The hook could not safely replay the last assistant response.{} {}",
            file.display(),
            prompt,
            note,
            render_prompt_continuation_instruction(
                &file.display().to_string(),
                agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
            )
        ),
    }
}

fn tracked_repeated_queue_recovery_response(
    file: &Path,
    cleanup_roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
    _input: &StopInput,
    prompt: &str,
    note: String,
) -> Result<StopResponse> {
    stop_phase("repeated_queue_next_head");
    let Some(next_prompt) = active_auto_queue_prompt(file)? else {
        park_state_across_roots(cleanup_roots, loaded_root, state)?;
        return Ok(StopResponse::AllowExit);
    };
    if next_prompt == prompt {
        return Ok(StopResponse::ContinueTurn {
            class: StopContinuationClass::Queue,
            reason: format!(
                "agent-doc Stop hook replayed a response for {}, but the queue head still did not advance: {:?}.{} {}",
                file.display(),
                prompt,
                note,
                render_prompt_continuation_instruction(
                    &file.display().to_string(),
                    agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
                )
            ),
        });
    }

    if let Some(response) =
        codex_stop_auto_clear_yield(file, &next_prompt, "tracked_state_after_recovery")
    {
        return Ok(response);
    }
    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(next_prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    log_background_context_clear_suppressed(
        file,
        &next_prompt,
        "tracked_state_after_recovery",
        state.last_context_clear_at,
    );
    Ok(StopResponse::ContinueTurn {
        class: StopContinuationClass::Queue,
        reason: format!(
            "agent-doc Stop hook recovered the previous queue response for {disp}.{note} The next queue prompt is {prompt:?}. {instruction}",
            disp = file.display(),
            note = note,
            prompt = next_prompt,
            instruction = {
                let display_path = file.display().to_string();
                if let Some(command) =
                    agent_doc_queue::queue_command::slash_command_text(&next_prompt)
                {
                    render_slash_command_continuation_instruction(&display_path, &command)
                } else {
                    render_prompt_continuation_instruction(
                        &display_path,
                        agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
                    )
                }
            },
        ),
    })
}

fn auto_queue_continuation_response(
    file: &Path,
    cleanup_roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<Option<StopResponse>> {
    // A response cycle may become `committed` before the editor-native save of
    // that exact retained projection is visible. Queue selection is a later
    // operation: advancing it here lets a recursive Stop capture the next head
    // while the prior keyed write still owns editor authority. Gate before even
    // reading the active head, and leave the session binding untouched so the
    // same owner pane continues only after delivery becomes terminal.
    //
    // `#sdkretained`: the holder is named by the SAME read that decides the
    // gate, and the wait is derived from the shared ownership predicate. A
    // gate that cannot name a holder does not pause the queue — waiting on an
    // unobserved holder is a wedge, since nothing can ever release it.
    if let Some(holder) = agent_doc_document_realtime_io::retained_session_closeout_holder(
        file,
        "codex_stop_auto_queue_continuation_gate",
    ) {
        let retry_key = holder.key().to_string();
        let gate = agent_doc_turn::write_ownership::retained_queue_gate(
            Some(holder.kind()),
            agent_doc_capture_io::retained_write_ownership(file),
        );
        if let agent_doc_turn::write_ownership::RetainedQueueGate::Recover(verdict) = gate {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_auto_queue_continuation_recovery intent_id={retry_key} verdict={} action=named_recovery queue_advanced=false capture_created=false (#sdkretained)",
                    verdict.as_str(),
                ),
            );
            return Ok(Some(retained_write_queue_recovery_response(
                file,
                &retry_key,
                verdict,
                input.stop_hook_active,
            )));
        }
        // Only a write-intent holder has a predecessor continuation to
        // quarantine against; a compact projection is not a response write.
        let pending = matches!(
            holder,
            agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::WriteIntent { .. }
        )
        .then(|| agent_doc_document_realtime_io::pending_document_write(file))
        .flatten();
        if let (Some(cycle), Some(pending)) = (
            agent_doc_cycle_state_io::load_with_closeout_projection(file)?,
            pending.as_ref(),
        ) && should_quarantine_retained_queue_child(
            cycle.phase,
            cycle.capture_id.is_some(),
            cycle.response_sha256.is_some(),
            &cycle.cycle_id,
            pending
                .continuation
                .as_ref()
                .map(|continuation| continuation.cycle_id.as_str()),
            state.last_auto_queue_head.as_deref(),
            cycle
                .prompt_targets
                .iter()
                .chain(&cycle.active_queue_heads)
                .chain(&cycle.active_free_text_queue_heads)
                .chain(&cycle.selected_free_text_queue_heads)
                .map(String::as_str),
        ) {
            agent_doc_cycle_state_io::mark_abandoned(
                file,
                "codex_stop_retained_predecessor_queue_child_quarantined",
                None,
                None,
            )?;
            let mut repaired_state = state.clone();
            repaired_state.last_auto_queue_head = None;
            repaired_state.updated_at = now_secs();
            save_state_across_roots(cleanup_roots, loaded_root, &repaired_state)?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_retained_predecessor_queue_child_quarantined cycle_id={} predecessor_cycle_id={} intent_id={} content_mutated=false capture_created=false session_queue_selection_rolled_back=true",
                    cycle.cycle_id,
                    pending
                        .continuation
                        .as_ref()
                        .map(|continuation| continuation.cycle_id.as_str())
                        .unwrap_or("none"),
                    retry_key,
                ),
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_auto_queue_continuation_deferred intent_id={} holder={} action=await_retained_write_terminal queue_advanced=false capture_created=false",
                retry_key,
                holder_token(&holder),
            ),
        );
        return Ok(Some(retained_write_queue_deferral_response(
            file,
            &holder,
            input.stop_hook_active,
        )));
    }
    let Some(prompt) = active_auto_queue_prompt(file)? else {
        return Ok(None);
    };
    if agent_doc_codex_hook_io::is_context_clear_prompt(&prompt) {
        return Ok(None);
    }
    if input.stop_hook_active && state.last_auto_queue_head.as_deref() == Some(&prompt) {
        return Ok(Some(
            match try_recover_repeated_queue_head_response(
                file,
                &prompt,
                input,
                Some(state.last_prompt.as_str()),
            )? {
                RepeatedQueueHeadRecovery::Recovered { note } => {
                    tracked_repeated_queue_recovery_response(
                        file,
                        cleanup_roots,
                        loaded_root,
                        state,
                        input,
                        &prompt,
                        note,
                    )?
                }
                RepeatedQueueHeadRecovery::NotRecoverable { note } => {
                    repeated_queue_recovery_unavailable_response(file, &prompt, &note)
                }
            },
        ));
    }
    // `#bgclearpolicy`: with queue auto-clear on (the default), a ready
    // supervisor owns this item boundary — it restarts Codex fresh (Codex's
    // `/clear` exits the process) and dispatches the next head to that fresh
    // context. Yield instead of continuing in this already-loaded context.
    if let Some(response) = codex_stop_auto_clear_yield(file, &prompt, "tracked_state") {
        return Ok(Some(response));
    }
    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    agent_doc_codex_hook_io::log_codex_stop_queue_continuation(file, &prompt, "tracked_state");
    log_background_context_clear_suppressed(
        file,
        &prompt,
        "tracked_state",
        state.last_context_clear_at,
    );
    // #codex-self-reinvoke-prevent (Option B): redirect the auto-queue
    // continuation to an IN-PANE answer + persist instead of instructing Codex to
    // run `agent-doc <FILE>` again. Re-running the entrypoint from the owner pane
    // re-enters the pane it runs in and trips the recursive-direct-invocation
    // deadlock guard; answering the next prompt in this same turn and persisting
    // with `agent-doc finalize <FILE>` (a non-dispatch command) continues the
    // queue without any nested self-invocation. This matches the run-path guard's
    // own OwnedPaneSelfInvocation guidance so both sources agree.
    Ok(Some(StopResponse::ContinueTurn {
        class: StopContinuationClass::Queue,
        reason: format!(
            "Codex labels the Stop continuation wire shape `Blocked by hook`. Next `agent:queue auto` prompt for {disp}: {prompt:?}. {instruction}",
            disp = file.display(),
            prompt = prompt,
            instruction = {
                let display_path = file.display().to_string();
                if let Some(command) = agent_doc_queue::queue_command::slash_command_text(&prompt) {
                    render_slash_command_continuation_instruction(&display_path, &command)
                } else {
                    render_prompt_continuation_instruction(
                        &display_path,
                        agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
                    )
                }
            },
        ),
    }))
}

fn should_quarantine_retained_queue_child<'a>(
    phase: agent_doc_turn::CyclePhase,
    has_capture: bool,
    has_response_hash: bool,
    current_cycle_id: &str,
    retained_predecessor_cycle_id: Option<&str>,
    last_auto_queue_head: Option<&str>,
    mut cycle_prompts: impl Iterator<Item = &'a str>,
) -> bool {
    let Some(predecessor) = retained_predecessor_cycle_id else {
        return false;
    };
    let Some(last_head) = last_auto_queue_head
        .map(str::trim)
        .filter(|head| !head.is_empty())
    else {
        return false;
    };
    phase == agent_doc_turn::CyclePhase::PreflightStarted
        && !has_capture
        && !has_response_hash
        && predecessor != current_cycle_id
        && cycle_prompts.any(|prompt| prompt.trim() == last_head)
}

fn holder_token(
    holder: &agent_doc_document_realtime_io::RetainedSessionCloseoutHolder,
) -> &'static str {
    match holder {
        agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::WriteIntent { .. } => {
            "write_intent"
        }
        agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::CompactProjection {
            ..
        } => "compact_projection",
    }
}

fn retained_write_queue_deferral_response(
    file: &Path,
    holder: &agent_doc_document_realtime_io::RetainedSessionCloseoutHolder,
    stop_hook_active: bool,
) -> StopResponse {
    let what = match holder {
        agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::WriteIntent {
            intent_id,
        } => format!(
            "the prior binary-owned editor write is still converging (intent_id={intent_id})"
        ),
        agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::CompactProjection {
            continuation_id,
        } => format!(
            "the post-commit compact projection is still converging (continuation_id={continuation_id}; terminal edge `compact_projection_completed`)"
        ),
    };
    let message = format!(
        "agent-doc Stop hook kept `agent:queue auto` paused for {} because {what}. The same owner pane continues only after that retained delivery and its terminal closeout settle. The hook did not select a new prompt or create another capture. Do not resend, recapture, rerun finalize/write/repair, force disk, or recycle; wait for the existing controller state edge unless it explicitly reports `needs_operator`. Do not send the final answer yet.",
        file.display(),
    );
    retained_queue_stop_response(message, stop_hook_active)
}

/// `#sdkretained`: an observed holder that the ownership predicate proves
/// cannot converge by waiting gets its terminating recovery, never a wait.
fn retained_write_queue_recovery_response(
    file: &Path,
    retry_key: &str,
    verdict: agent_doc_turn::write_ownership::RetainedWriteVerdict,
    stop_hook_active: bool,
) -> StopResponse {
    let display = file.display().to_string();
    let ownership = match verdict {
        agent_doc_turn::write_ownership::RetainedWriteVerdict::DeliveryRejected => {
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED
                .with_retained_projection(true)
                .with_delivery_rejected(true)
        }
        agent_doc_turn::write_ownership::RetainedWriteVerdict::ReplicaUnserved => {
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED
                .with_retained_projection(true)
                .with_replica_unserved(true)
        }
        agent_doc_turn::write_ownership::RetainedWriteVerdict::Stranded => {
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED
                .with_editor_route_unowned(true)
        }
        agent_doc_turn::write_ownership::RetainedWriteVerdict::EditorHoldsUnmergedVisibleText => {
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED
                .with_retained_projection(true)
                .with_editor_holds_unmerged_visible_text(true)
        }
        _ => agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED,
    };
    let message = format!(
        "agent-doc Stop hook kept `agent:queue auto` paused for {display}: the retained editor write (intent_id={retry_key}) cannot converge by waiting (verdict={}). {}. The hook did not select a new prompt or create another capture. Do not send the final answer yet.",
        verdict.as_str(),
        agent_doc_turn::write_ownership::retained_write_remedy(ownership, &display),
    );
    retained_queue_stop_response(message, stop_hook_active)
}

fn retained_queue_stop_response(message: String, stop_hook_active: bool) -> StopResponse {
    if stop_hook_active {
        StopResponse::RefuseExit { reason: message }
    } else {
        StopResponse::ContinueTurn {
            class: StopContinuationClass::Queue,
            reason: message,
        }
    }
}

fn log_slow_stop_closeout_phase(file: &Path, phase: &str, started: &mut std::time::Instant) {
    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(250) {
        eprintln!(
            "[perf] codex_stop.{} file={} elapsed_ms={}",
            phase,
            file.display(),
            elapsed.as_millis()
        );
    }
    *started = std::time::Instant::now();
}

/// Explain a closeout whose repair could not finish, with the remedy derived from who
/// (if anything) durably holds the write.
fn closeout_repair_retained_note(
    err: &anyhow::Error,
    ownership: agent_doc_turn::write_ownership::RetainedWriteOwnership,
    file: &Path,
) -> String {
    let error = format!("{err:#}").replace('\n', " ");
    let remedy = agent_doc_turn::write_ownership::retained_write_remedy(
        ownership,
        &file.display().to_string(),
    );
    if error.contains(&remedy) {
        format!(" The hook could not finish the required commit boundary: {error}.")
    } else {
        format!(" The hook could not finish the required commit boundary: {error}. {remedy}.")
    }
}

fn durable_owner_repair_deferral_response(
    file: &Path,
    note: &str,
    stop_hook_active: bool,
) -> StopResponse {
    let display = file.display();
    let message = format!(
        "agent-doc Stop hook retained the exact closeout for {display} under its durable keyed retry.{note} The controller owns delivery, the editor-native save receipt, and the terminal commit. Do not reopen or recapture the cycle, rerun finalize/write/repair, force disk, recycle the supervisor, or resend the response. Wait for the existing controller state edge unless it explicitly reports `needs_operator`; do not send the final answer yet."
    );
    if stop_hook_active {
        StopResponse::RefuseExit { reason: message }
    } else {
        StopResponse::ContinueTurn {
            class: StopContinuationClass::Recovery,
            reason: message,
        }
    }
}

/// Surface the exact terminal verdict reached by a retained closeout owner.
///
/// This mirrors the recursive-Stop behavior of
/// [`retained_closeout_needs_operator`]: the first Stop reports the blocker and
/// the recursive Stop lets that report through. Crucially, it does not render
/// the stale owner-contention note that preceded the terminal verdict.
fn captured_finalize_needs_operator_response(
    file: &Path,
    reason: &str,
    stop_hook_active: bool,
) -> StopResponse {
    if stop_hook_active {
        return StopResponse::AllowExit;
    }
    StopResponse::ContinueTurn {
        class: StopContinuationClass::Recovery,
        reason: format!(
            "agent-doc Stop hook's exact retained closeout for {} reached `needs_operator` after the owner state edge. {} This is an operator blocker, not an in-progress owner wait. Report this exact blocker and its recovery to the operator; do not restart, recapture, force disk, or claim that waiting will resolve it.",
            file.display(),
            reason.replace('\n', " "),
        ),
    }
}

/// Follow the exact keyed owner's controller projection before surfacing a
/// retained-closeout blocker.
///
/// The lease refusal is only a snapshot of ownership at repair time. The
/// controller's document-lifecycle graph is the authority for what happens
/// next: terminal/superseded projections are re-inspected, and an owner-release
/// edge permits one exact keyed resume before re-inspection. Only a bounded
/// wait with no progress (or a failed observation) preserves the fail-closed
/// deferral response.
fn await_durable_owner_repair_deferral(
    file: &Path,
    note: &str,
    input: &StopInput,
) -> Result<StopResponse> {
    let key = match agent_doc_repair_command_io::captured_finalize_resume_key(file) {
        Ok(Some(key)) => key,
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                "codex_stop_durable_owner_wait_skipped reason=missing_exact_resume_key action=fail_closed",
            );
            return Ok(durable_owner_repair_deferral_response(
                file,
                note,
                input.stop_hook_active,
            ));
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_durable_owner_wait_skipped reason=resume_key_read_failed err={} action=fail_closed",
                    format!("{err:#}").replace('\n', " "),
                ),
            );
            return Ok(durable_owner_repair_deferral_response(
                file,
                note,
                input.stop_hook_active,
            ));
        }
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_durable_owner_wait_started cycle_id={} capture_id={} action=await_controller_state_edge",
            key.cycle_id, key.capture_id,
        ),
    );
    let decision = match await_durable_owner_progress_with(
        file,
        &key.cycle_id,
        agent_doc_closeout_runtime_io::await_closeout_cycle_progress,
    ) {
        Ok(decision) => decision,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_durable_owner_wait_failed cycle_id={} err={} action=fail_closed",
                    key.cycle_id,
                    format!("{err:#}").replace('\n', " "),
                ),
            );
            return Ok(durable_owner_repair_deferral_response(
                file,
                note,
                input.stop_hook_active,
            ));
        }
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_durable_owner_wait_settled cycle_id={} decision={decision:?}",
            key.cycle_id,
        ),
    );
    match decision {
        DurableOwnerProgressDecision::Reinspect => {
            invalidate_stop_document_cache();
            apply_stop(input)
        }
        DurableOwnerProgressDecision::ResumeThenReinspect => {
            match try_resume_captured_finalize_in_hook(file) {
                CapturedFinalizeHookResume::Closed => {
                    invalidate_stop_document_cache();
                    apply_stop(input)
                }
                CapturedFinalizeHookResume::NeedsOperator { reason } => {
                    Ok(captured_finalize_needs_operator_response(
                        file,
                        &reason,
                        input.stop_hook_active,
                    ))
                }
                CapturedFinalizeHookResume::Pending => Ok(durable_owner_repair_deferral_response(
                    file,
                    note,
                    input.stop_hook_active,
                )),
            }
        }
        DurableOwnerProgressDecision::StillPending => Ok(durable_owner_repair_deferral_response(
            file,
            note,
            input.stop_hook_active,
        )),
    }
}

fn attempt_stop_closeout(
    file: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<StopCloseAttempt> {
    let mut phase_started = std::time::Instant::now();
    stop_phase("closeout_classify_payload");
    let payload =
        agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let has_response = matches!(
        payload,
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
    );
    stop_phase("closeout_reopen_terminal_cycle");
    if reopen_terminal_cycle_before_stop_capture(file, &payload)?
        == StopCaptureReopen::PayloadAlreadyCommitted
    {
        return Ok(StopCloseAttempt::AlreadyCommitted);
    }
    invalidate_stop_document_cache();
    stop_phase("closeout_detect_bypassed_patchback");
    let has_bypassed_patchback =
        agent_doc_session_check_io::detect_bypassed_response_write(file)?.is_some();
    if !has_response && !has_bypassed_patchback {
        return Ok(match payload {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
                StopCloseAttempt::StillOpen {
                    note: capture_blocked_stop_payload(
                        file,
                        &input.last_assistant_message,
                        &reason,
                        Some(state.last_prompt.as_str()),
                    ),
                }
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
                StopCloseAttempt::StillOpen {
                    note: capture_missing_stop_response(file, Some(state.last_prompt.as_str())),
                }
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_) => {
                StopCloseAttempt::NotPossible
            }
        });
    }

    stop_phase("closeout_active_queue_prompt");
    let active_queue_prompt = active_auto_queue_prompt(file)?;
    stop_phase("closeout_open_cycle_check");
    let queue_synthetic_cycle =
        active_queue_prompt.is_some() && open_cycle_started_from_unchanged_file(file)?;
    let captured_response_targets_queue_head = if queue_synthetic_cycle {
        match &payload {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                response_explicitly_targets_current_queue_head(
                    file,
                    response.as_ref(),
                    "codex_stop_captured_response_targets_queue_head",
                )?
            }
            _ => false,
        }
    } else {
        false
    };
    let queue_completion_ids = if queue_synthetic_cycle && captured_response_targets_queue_head {
        match &payload {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                let content_before_repair =
                    current_document_content(file, "codex_stop_auto_close_before_repair")?;
                agent_doc_queue::queue_consume::queue_targeted_completion_id_for_current_head(
                    file,
                    None,
                    &content_before_repair,
                    response.as_ref(),
                    &[],
                )?
                .into_iter()
                .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    log_slow_stop_closeout_phase(file, "intent_classification", &mut phase_started);

    let mut note = String::new();
    match payload {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            if let Some(capture_id) =
                materialized_cycle_capture_supersedes(file, response.as_ref())?
            {
                note.push_str(&format!(
                    " The cycle's retained response `{capture_id}` already owns this closeout, so an insufficient or duplicate closing chat message was not recaptured over it."
                ));
            } else {
                agent_doc_repair_io::pending::save_pending(file, response.as_ref())?;
                agent_doc_ops_log_io::log_op(file, "codex_stop_capture_saved");
                note.push_str(
                    " The latest assistant text was captured into the pending/capture ledger before auto-close.",
                );
            }
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            note.push_str(&capture_blocked_stop_payload(
                file,
                &input.last_assistant_message,
                &reason,
                Some(state.last_prompt.as_str()),
            ));
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {}
    }
    log_slow_stop_closeout_phase(file, "intent_capture", &mut phase_started);

    // `#stopretainedfailclosed`: a repair that cannot finish (typically an editor-owned
    // write retained under a deferred intent while the controller hands off) is a still-open
    // closeout whose owner the ownership predicate derives, not a hook failure. Propagating it here surfaced as "Stop hook failed closed" on fpe.md
    // (2026-09-30) while the same intent settled and committed 29s later.
    let repair_outcome = match agent_doc_repair_io::run_with_queue_completion_ids(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        &queue_completion_ids,
    ) {
        Ok(outcome) => outcome,
        Err(err) => {
            let ownership = agent_doc_capture_io::retained_write_ownership(file);
            let rendered = format!("{err:#}").replace('\n', " ");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_closeout_repair_retained verdict={:?} err={}",
                    ownership.verdict(),
                    rendered,
                ),
            );
            note.push_str(&closeout_repair_retained_note(&err, ownership, file));
            if repair_failure_is_binary_owned_deferral(&rendered, ownership.verdict()) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_closeout_repair_deferred verdict={} owner=durable_keyed_retry action=await_controller_state_edge",
                        ownership.verdict().as_str(),
                    ),
                );
                return Ok(StopCloseAttempt::RepairDeferredToDurableOwner { note });
            }
            return Ok(StopCloseAttempt::StillOpen { note });
        }
    };
    // Repair wrote the document; the pre-repair memo is now stale.
    invalidate_stop_document_cache();
    log_slow_stop_closeout_phase(file, "intent_repair", &mut phase_started);
    if repair_outcome.replayed_response() {
        note.push_str(" The hook replayed the response through the normal write path.");
    } else if repair_outcome.repaired() {
        note.push_str(" The hook repaired the pending closeout state before auto-close.");
    }
    let queue_repair_explicitly_closes_head = queue_synthetic_cycle
        && repair_outcome.replayed_response()
        && captured_response_targets_queue_head;
    if queue_repair_explicitly_closes_head {
        match consume_recovered_queue_head(
            file,
            active_queue_prompt.as_deref().unwrap_or_default(),
            &queue_completion_ids,
        ) {
            Ok(Some(outcome)) => {
                note.push_str(&format!(
                    " The hook consumed the completed queue head {:?} before commit.",
                    outcome.consumed_text
                ));
            }
            Ok(None) => {}
            Err(err) => {
                note.push_str(&format!(
                    " The hook wrote or recovered the response but could not consume the completed queue head: {err}."
                ));
                return Ok(StopCloseAttempt::StillOpen { note });
            }
        }
    } else if queue_synthetic_cycle && repair_outcome.repaired() {
        note.push_str(" The hook preserved the active queue head because the repair did not explicitly close it.");
    }

    if !agent_doc_git_io::status::is_in_git_repo(file) {
        note.push_str(
            " The document is not in a git repository, so the hook could not finish the required commit boundary automatically.",
        );
        return Ok(StopCloseAttempt::StillOpen { note });
    }

    // Strict repair replay already runs the required commit/session-check path.
    // Consume that receipt only while it still matches: queue maintenance or
    // operator edits after replay must cross a new closeout boundary.
    if repair_outcome.replayed_response()
        && agent_doc_flow_io::closeout::replay_closeout_still_proven(
            file,
            &fresh_document_content(file, "codex_stop_replay_terminal_receipt")?,
        )?
    {
        agent_doc_ops_log_io::log_op(
            file,
            "codex_stop_auto_close_success source=strict_replay_receipt",
        );
        return Ok(StopCloseAttempt::Closed);
    }
    match agent_doc_closeout_runtime_io::complete_required_closeout(file, false) {
        Ok(true) => {
            note.push_str(" The hook finished the commit boundary automatically.");
        }
        Ok(false) => {}
        Err(err) => {
            let ownership = agent_doc_capture_io::retained_write_ownership(file);
            let rendered = format!("{err:#}").replace('\n', " ");
            // `#stopnextedit`: this typed refusal proves the response is
            // already committed and only a newer operator-authored queue,
            // backlog, status, or other typed-component edit remains. Its
            // shared remedy explicitly says END THIS TURN and let the next
            // cycle answer that edit. Do not turn that successful boundary
            // into a Codex hook block by wrapping it as `StillOpen`.
            if unanswered_edit_pending_commit_refusal(&err) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_auto_close_next_cycle verdict=unanswered_edit_pending action=allow_exit err={rendered} (#stopnextedit)"
                    ),
                );
                return Ok(StopCloseAttempt::UnansweredEditPending);
            }
            match closeout_failure_disposition(&rendered, ownership.verdict()) {
                CloseoutFailureDisposition::DeferToDurableOwner => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "codex_stop_auto_close_closeout_deferred verdict={} reason=captured_response_not_materialized owner=durable_retained_capture err={rendered}",
                            ownership.verdict().as_str(),
                        ),
                    );
                    return Ok(StopCloseAttempt::DeferredToDurableOwner);
                }
                CloseoutFailureDisposition::AwaitDurableOwner => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "codex_stop_auto_close_closeout_deferred verdict={} reason=closeout_owner_active owner=durable_keyed_retry action=await_controller_state_edge err={rendered}",
                            ownership.verdict().as_str(),
                        ),
                    );
                    note.push_str(&closeout_repair_retained_note(&err, ownership, file));
                    return Ok(StopCloseAttempt::RepairDeferredToDurableOwner { note });
                }
                CloseoutFailureDisposition::FailClosed => {}
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!("codex_stop_auto_close_closeout_failed err={err}"),
            );
            note.push_str(&format!(
                " The hook wrote or recovered the response but could not finish the required commit boundary: {err}."
            ));
            return Ok(StopCloseAttempt::StillOpen { note });
        }
    }
    agent_doc_ops_log_io::log_op(file, "codex_stop_auto_close_success");
    Ok(StopCloseAttempt::Closed)
}

/// Whether the final commit boundary refused because a newer unanswered
/// typed-component edit must remain uncommitted for the next cycle.
///
/// This is a typed error-chain check, not diagnostic parsing: the refusal's
/// wording is operator guidance and may evolve independently.
fn unanswered_edit_pending_commit_refusal(error: &anyhow::Error) -> bool {
    agent_doc_commit_io::retained_write_commit_refusal_verdict(error)
        == Some(agent_doc_turn::write_ownership::RetainedWriteVerdict::UnansweredEditPending)
}

/// Whether a failed commit boundary is a deferral the binary already owns.
///
/// Only the missing-captured-response refusal qualifies, and only while the
/// shared ownership predicate says something durable holds the intent
/// (`Deferred`): the retained capture's keyed worker projects the body and
/// commits on the next delivery edge. Every other verdict, including a durable
/// but unowned capture, stays fail-closed so the agent is told to act.
fn closeout_failure_disposition(
    rendered_error: &str,
    verdict: agent_doc_turn::write_ownership::RetainedWriteVerdict,
) -> CloseoutFailureDisposition {
    if verdict != agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred {
        return CloseoutFailureDisposition::FailClosed;
    }
    if agent_doc_repair_command_io::captured_finalize_retry_at(rendered_error).is_some() {
        return CloseoutFailureDisposition::AwaitDurableOwner;
    }
    if agent_doc_git_io::capture_materialization_guard::is_missing_captured_response_refusal(
        rendered_error,
    ) {
        return CloseoutFailureDisposition::DeferToDurableOwner;
    }
    CloseoutFailureDisposition::FailClosed
}

/// Whether repair has already handed the exact write to the controller-owned
/// retained-intent retry. Validation, parse, and evidence failures are excluded:
/// those need a corrected owner response and cannot converge from a state edge.
fn repair_failure_is_binary_owned_deferral(
    rendered_error: &str,
    verdict: agent_doc_turn::write_ownership::RetainedWriteVerdict,
) -> bool {
    verdict == agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred
        && (agent_doc_turn::write_ownership::is_retained_write_refusal(rendered_error)
            || agent_doc_repair_command_io::captured_finalize_retry_at(rendered_error).is_some())
}

/// Mint a fresh capture cycle when the Stop payload arrives after a committed
/// predecessor.
///
/// `#fixlazilycodex`: a payload HEAD already holds verbatim is the committed
/// closeout's own text — a Stop after a proven terminal closeout. That is an
/// idempotent no-op ([`StopCaptureReopen::PayloadAlreadyCommitted`]), not a
/// failure: refusing it as an error made the hook fail closed on a cycle that
/// had converged and committed. A payload visible only in the uncommitted
/// current authority, or no HEAD at all, still fails closed.
fn reopen_terminal_cycle_before_stop_capture(
    file: &Path,
    payload: &agent_doc_template::replay_guard::ReplayPayloadClassification<'_>,
) -> Result<StopCaptureReopen> {
    let agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) =
        payload
    else {
        return Ok(StopCaptureReopen::NotNeeded);
    };
    let Some(previous_cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file) else {
        return Ok(StopCaptureReopen::NotNeeded);
    };
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        anyhow::bail!(
            "post-commit Stop closeout cannot mint a fresh capture cycle without a HEAD baseline"
        );
    };
    if agent_doc_turn::response_replay::response_materialized_in_content(response.as_ref(), &head) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "codex_stop_post_commit_payload_already_committed cycle_id={previous_cycle_id} capture_created=false reopened=false (#fixlazilycodex)"
            ),
        );
        return Ok(StopCaptureReopen::PayloadAlreadyCommitted);
    }

    let current = current_document_content(file, "codex_stop_post_commit_cycle_baseline")?;
    if agent_doc_turn::response_replay::response_materialized_in_content(
        response.as_ref(),
        &current,
    ) {
        anyhow::bail!(
            "post-commit Stop payload is already materialized in current document authority; refusing to capture it under a fresh cycle"
        );
    }

    let reopened = agent_doc_cycle_state_io::start_out_of_band_closeout_preflight(
        file,
        Some(&head),
        Some(&current),
    )?;
    anyhow::ensure!(
        reopened.cycle_id != previous_cycle_id,
        "post-commit Stop closeout failed to mint a fresh cycle: terminal cycle {} was reused",
        previous_cycle_id
    );
    anyhow::ensure!(
        agent_doc_flow_io::closeout::cycle_already_committed(file).is_none(),
        "post-commit Stop closeout minted cycle {} but the closeout projection remained terminal",
        reopened.cycle_id
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_post_commit_prompt_cycle_reopened previous_cycle_id={} cycle_id={} source=replayable_stop_payload",
            previous_cycle_id, reopened.cycle_id
        ),
    );
    Ok(StopCaptureReopen::Reopened)
}

/// `#stoprecapturesupersede`: the open cycle already holds a live capture whose
/// response is visible in the document, and the Stop hook's final chat message
/// is not. Codex routinely ends a turn with a short chat restatement after its
/// real `agent-doc respond`; recapturing that as the cycle's response superseded
/// the real one, and its later replay (headed "recovered response") replaced the
/// visible answer in `src/haiven-dev/tasks/api.md` on 2026-09-28, diverging the
/// controller from the editor that still held the original. Returns the
/// existing capture id when the recapture must be skipped.
fn materialized_cycle_capture_supersedes(file: &Path, message: &str) -> Result<Option<String>> {
    let Some(closeout) = agent_doc_cycle_state_io::load_closeout_projection(file)? else {
        return Ok(None);
    };
    if closeout.captured_response_retired_reason.is_some() {
        return Ok(None);
    }
    let Some(capture) = closeout
        .captured_response
        .filter(|capture| closeout.cycle_id.as_deref() == Some(capture.cycle_id.as_str()))
        .filter(|capture| !capture.response_body.trim().is_empty())
    else {
        return Ok(None);
    };
    let current = current_document_content(file, "codex_stop_existing_capture_check")?;
    let existing_visible = agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        &current,
    );
    let message_visible =
        agent_doc_turn::response_replay::response_materialized_in_content(message, &current);
    if message_visible {
        return Ok(None);
    }
    if !existing_visible {
        let Some(cycle) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
            .filter(|cycle| cycle.cycle_id == capture.cycle_id)
        else {
            return Ok(None);
        };
        let baseline = capture.baseline_content.as_deref();
        // The durable "free-text" selection also includes prompt-preset
        // invocations. The shared closeout validator therefore accepts either
        // an exact queue quote or every resolved preset expansion, exactly as
        // the later pre-write gate does.
        let missing_before = agent_doc_queue::queue_closeout_guard::selected_free_text_prompts_missing_response_evidence_for_closeout(
            baseline,
            &current,
            &capture.response_body,
            &cycle.selected_free_text_queue_heads,
            false,
        )?;
        if !missing_before.is_empty() {
            let missing_after = agent_doc_queue::queue_closeout_guard::selected_free_text_prompts_missing_response_evidence_for_closeout(
                baseline,
                &current,
                message,
                &cycle.selected_free_text_queue_heads,
                false,
            )?;
            if !missing_after.is_empty() {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_recapture_skipped file={} cycle_id={} capture_id={} reason=retained_capture_still_missing_selected_queue_response_evidence missing_before={} missing_after={}",
                        file.display(),
                        capture.cycle_id,
                        capture.capture_id,
                        missing_before.len(),
                        missing_after.len(),
                    ),
                );
                return Ok(Some(capture.capture_id));
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_retained_capture_owner_repair file={} cycle_id={} capture_id={} old_response_sha256={} new_response_sha256={} repaired_selected_queue_response_evidence={} action=replace_same_capture_id authority=current_document",
                    file.display(),
                    capture.cycle_id,
                    capture.capture_id,
                    capture.response_sha256,
                    agent_doc_hash::content_hash(message),
                    missing_before.len(),
                ),
            );
            return Ok(None);
        }
    }
    if !existing_visible {
        return Ok(None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_recapture_skipped file={} cycle_id={} capture_id={} reason=materialized_cycle_capture",
            file.display(),
            capture.cycle_id,
            capture.capture_id,
        ),
    );
    Ok(Some(capture.capture_id))
}

fn capture_assistant_text(file: &Path, state: &SessionState, input: &StopInput) -> String {
    // A capture writes the document; a later read in this same invocation must
    // re-materialize rather than serve the pre-write snapshot.
    invalidate_stop_document_cache();
    match agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message) {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
            capture_missing_stop_response(file, Some(state.last_prompt.as_str()))
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            match materialized_cycle_capture_supersedes(file, response.as_ref()) {
                Ok(Some(capture_id)) => {
                    return format!(
                        " The cycle's retained response `{capture_id}` already owns this closeout, so an insufficient or duplicate closing chat message was not recaptured over it."
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    return format!(
                        " The hook could not check the cycle's existing capture, so it did not recapture the final assistant text: {err}."
                    );
                }
            }
            match agent_doc_repair_io::pending::save_pending(file, response.as_ref()) {
                Ok(()) => {
                    agent_doc_ops_log_io::log_op(file, "codex_stop_capture_saved");
                    " The latest assistant text was captured into the pending/capture ledger before the turn stopped.".to_string()
                }
                Err(err) => format!(
                    " The hook could not capture the final assistant text before blocking the turn: {err}."
                ),
            }
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            capture_blocked_stop_payload(
                file,
                &input.last_assistant_message,
                &reason,
                Some(state.last_prompt.as_str()),
            )
        }
    }
}

fn capture_missing_stop_response(file: &Path, last_prompt: Option<&str>) -> String {
    invalidate_stop_document_cache();
    let reason = "the Stop hook received no final assistant closeout; this can happen when Codex stops after a tool-only or authentication step before the assistant emits the final response";
    match agent_doc_codex_hook_io::save_blocked_stop_payload(
        file,
        "",
        reason,
        "missing_last_assistant_message",
        last_prompt,
    ) {
        Ok(path) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_capture_missing_response path={} reason={reason}",
                    path.display()
                ),
            );
            format!(
                " The hook did not receive a non-empty `last_assistant_message`; this can happen when Codex stops after a tool-only or authentication step such as an MCP OAuth/authenticate flow before the final closeout is emitted. It saved a diagnostic record at `{}` with the tracked prompt so you can resume the turn, respond in the document, and still finish with `agent-doc finalize` / `agent-doc session-check`.",
                path.display()
            )
        }
        Err(err) => format!(
            " The hook did not receive a non-empty `last_assistant_message`; this can happen when Codex stops after a tool-only or authentication step such as an MCP OAuth/authenticate flow before the final closeout is emitted, and it could not save the diagnostic record: {err}.",
        ),
    }
}

fn capture_blocked_stop_payload(
    file: &Path,
    payload: &str,
    reason: &str,
    last_prompt: Option<&str>,
) -> String {
    invalidate_stop_document_cache();
    match agent_doc_codex_hook_io::save_blocked_stop_payload(
        file,
        payload,
        reason,
        "blocked_replay_payload",
        last_prompt,
    ) {
        Ok(path) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_stop_capture_blocked path={} reason={}",
                    path.display(),
                    reason
                ),
            );
            format!(
                " The hook captured the blocked `last_assistant_message` for diagnostics at `{}` and refused to replay it because {}.",
                path.display(),
                reason
            )
        }
        Err(err) => format!(
            " The hook refused to replay `last_assistant_message` because {} and could not save the blocked payload for diagnostics: {err}.",
            reason
        ),
    }
}

fn read_stdin_payload() -> Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("read hook payload from stdin")?;
    Ok(payload)
}

fn response_explicitly_targets_current_queue_head(
    file: &Path,
    response: &str,
    source: &str,
) -> Result<bool> {
    let content = current_document_content(file, source)?;
    let Some(queue_head) = agent_doc_queue::queue_heads::active_queue_head_text(&content)? else {
        return Ok(false);
    };
    Ok(
        agent_doc_queue::queue_response::response_explicitly_targets_queue_head(
            response,
            &queue_head,
        ),
    )
}

thread_local! {
    /// `#codexstopbudgetblind`: one materialized document per path, per Stop
    /// hook invocation.
    ///
    /// The hook is a read-only status gate, and it asks the same question from
    /// several places — the queue-clear check, the active-prompt/queue-head
    /// check, and the closeout attempt each resolve the document. On a live
    /// editor every one of those is a controller round-trip plus a full CRDT
    /// materialization; the dogfood ops log shows four for a 51KB document in a
    /// single invocation, all returning the identical `text_hash`.
    ///
    /// Memoizing also makes the hook's decisions SELF-CONSISTENT: without it,
    /// the queue-clear check and the queue-head check could read two different
    /// document versions and disagree within one invocation.
    ///
    /// Scoped to the worker thread and dropped with it, so no invocation can
    /// serve another one's content. Cleared explicitly by
    /// [`invalidate_stop_document_cache`] if the hook ever mutates the document.
    static STOP_DOCUMENT_CACHE: std::cell::RefCell<
        std::collections::HashMap<PathBuf, String>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Drop the Stop hook's memoized document content.
///
/// Call after any path that can change the document within one invocation, so a
/// later read in the same invocation re-materializes instead of serving a
/// pre-write snapshot.
fn invalidate_stop_document_cache() {
    STOP_DOCUMENT_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// The uncached resolution. Kept as its own function so the realtime-IO call
/// keeps the exact one-line shape the architecture guard in `tests/test_cli.rs`
/// pins, and so the memo above it is visibly a wrapper rather than a rewrite of
/// how the Stop hook reads a document.
fn resolve_document_content_uncached(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(file, source).with_context(
        || {
            format!(
                "{source}: failed to resolve current document {}",
                file.display()
            )
        },
    )
}

/// Re-resolve the document after this invocation may have written it.
///
/// Stale-memo receipt (stop-receipt-stale-memo): the strict replay receipt compares the CURRENT
/// document against the hash its terminal closeout recorded. Both recovery
/// paths memoize the document before repair (to classify the queue head), and
/// repair then writes the response and commits it. Reading the receipt through
/// the memo compared the pre-repair text against the post-commit hash, so the
/// receipt never matched and every replay ran a second, redundant full
/// closeout (commit authority + session-check, several seconds per pass and
/// proportionally more under load) inside the Stop hook's wall-clock budget.
fn fresh_document_content(file: &Path, source: &str) -> Result<String> {
    invalidate_stop_document_cache();
    current_document_content(file, source)
}

fn current_document_content(file: &Path, source: &str) -> Result<String> {
    if let Some(cached) = STOP_DOCUMENT_CACHE.with(|cache| cache.borrow().get(file).cloned()) {
        return Ok(cached);
    }
    let content = resolve_document_content_uncached(file, source)?;
    STOP_DOCUMENT_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(file.to_path_buf(), content.clone())
    });
    Ok(content)
}

fn active_auto_queue_prompt(file: &Path) -> Result<Option<String>> {
    // Single source of truth: the shared queue-continuation detector
    // (#codex-auto-queue-stalled-final-gate). Keeps the Stop-hook continuation
    // decision identical to the durable marker and `session-check` gate.
    //
    // The Stop hook already has an editor-authoritative content resolver. Do
    // not drop back to the file-backed adapter here: a native editor save may
    // still be converging after finalize has reaped the completed head, and
    // rereading disk would re-serve that stale head even though the CRDT model
    // already exposes the next one (`#stopcurrentqueuehead`).
    // This is deliberately uncached. Earlier phases in the same Stop-hook
    // invocation may have completed a repair/write after memoizing the
    // pre-write projection; continuation selection is the final gate and must
    // re-resolve authority after those mutations.
    let content = resolve_document_content_uncached(file, "stop_active_auto_queue_head")?;
    active_auto_queue_prompt_for_content(file, &content)
}

fn active_auto_queue_prompt_for_content(file: &Path, content: &str) -> Result<Option<String>> {
    Ok(
        agent_doc_queue_io::queue_continuation::detect_for_content(file, content)?
            .map(|continuation| continuation.head_prompt),
    )
}

fn open_cycle_started_from_unchanged_file(file: &Path) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    if !state.is_open() {
        return Ok(false);
    }
    Ok(
        match (&state.normalized_snapshot_hash, &state.normalized_file_hash) {
            (Some(snapshot), Some(file)) => snapshot == file,
            _ => match (&state.snapshot_hash, &state.file_hash) {
                (Some(snapshot), Some(file)) => snapshot == file,
                _ => false,
            },
        },
    )
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(
        state: agent_doc_controller::actor::ActorState,
    ) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session".to_string(),
            generation: 7,
            pane_id: "%218".to_string(),
            window_id: "@0".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "test".to_string(),
                timestamp: 1,
                prior_generation: 6,
                new_generation: 7,
            },
        }
    }

    #[test]
    fn harness_stop_uses_the_non_closed_document_actor_identity() {
        let actor = actor(agent_doc_controller::actor::ActorState::Ready);
        assert_eq!(
            stop_pane_identity(
                agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                Some(&actor),
            ),
            StopPaneIdentity::AuthoritativeActor("%218".to_string()),
        );
    }

    #[test]
    fn stop_actor_delegation_stays_closed_for_external_or_closed_bindings() {
        let ready = actor(agent_doc_controller::actor::ActorState::Ready);
        let closed = actor(agent_doc_controller::actor::ActorState::Closed);
        assert_eq!(
            stop_pane_identity(
                agent_doc_codex_hook_io::SessionIdentityOrigin::ExternalPrompt,
                Some(&ready),
            ),
            StopPaneIdentity::Ambient,
        );
        assert_eq!(
            stop_pane_identity(
                agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                Some(&closed),
            ),
            StopPaneIdentity::Ambient,
        );
        assert_eq!(
            stop_pane_identity(
                agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                None,
            ),
            StopPaneIdentity::Ambient,
        );
    }

    #[test]
    fn exact_actor_stop_retires_only_the_bound_panes_turn_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("task.md");
        std::fs::write(&file, "# task\n").expect("write task");
        agent_doc_turn_status_io::write_turn_active_marker(dir.path(), "%218")
            .expect("write bound marker");
        agent_doc_turn_status_io::write_turn_active_marker(dir.path(), "%219")
            .expect("write ambient marker");
        let input = StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "codex-turn".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        };

        retire_bound_turn_status_on_stop(Some(dir.path()), "%218", &input, &file);

        assert!(!agent_doc_turn_status_io::turn_active_for_pane(
            dir.path(),
            "%218"
        ));
        assert!(agent_doc_turn_status_io::turn_active_for_pane(
            dir.path(),
            "%219"
        ));
        assert_eq!(
            agent_doc_turn_status_io::pane_harness_session(dir.path(), "%218")
                .map(|(session_id, _)| session_id),
            Some("codex-session".to_string())
        );
    }

    /// `#loopreentrynoop`: the continuation instruction must remain actionable
    /// after the first iteration.
    ///
    /// The original text named only "invoke the `loop` skill", which is a no-op
    /// once `/loop` is loaded -- every iteration but the first. With the other
    /// two routes forbidden (this message bans shelling the trigger,
    /// `#preflightinbinary` bans shelling preflight), an agent that followed it
    /// literally had no legal way to continue.
    #[test]
    fn continuation_reason_names_the_noop_case_and_a_working_reentry() {
        let reason = claude_stop_continuation_reason("/p/doc.md", "do [#x]");
        for needle in [
            "ScheduleWakeup",
            "exactly the prompt `/loop agent-doc /p/doc.md`",
            "not the `loop` skill",
            "admits no cycle",
            "Do not shell `agent-doc /p/doc.md` or `agent-doc preflight`",
            "do [#x]",
            "(`#loopreentrynoop`)",
        ] {
            assert!(reason.contains(needle), "lost `{needle}`: {reason}");
        }
        // `#stopfeedbacknoterror`: concise. The previous text was ~800 bytes
        // and said "not an error" beside a UI that labeled it an error.
        assert!(
            reason.len() <= 360 + 2 * "/p/doc.md".len(),
            "continuation must stay concise ({} bytes): {reason}",
            reason.len()
        );
        assert!(!reason.contains("error"), "{reason}");
    }

    /// A long multi-line free-text head is previewed, not quoted in full.
    #[test]
    fn continuation_reason_previews_a_long_head() {
        let head = format!("{}\nsecond line with a fenced log", "é".repeat(400));
        let reason = claude_stop_continuation_reason("/p/doc.md", &head);
        assert!(!reason.contains("second line"), "{reason}");
        assert!(reason.contains('…'), "{reason}");
        assert!(reason.len() < 700, "{} bytes", reason.len());
    }

    /// `#loopskillnoadmit`: a `loop` Skill call admits no cycle on ANY
    /// iteration, so the instruction must never order one — it leads with the
    /// scheduled submitted re-entry and only names the Skill call to rule it out.
    #[test]
    fn continuation_never_orders_a_loop_skill_call() {
        let reason = claude_stop_continuation_reason("/p/doc.md", "do [#x]");
        assert!(!reason.contains("Invoke the `loop` skill"), "{reason}");
        let wake = reason.find("ScheduleWakeup").unwrap();
        let skill = reason.find("loop` skill").unwrap();
        assert!(
            wake < skill,
            "the working re-entry must come first: {reason}"
        );
    }

    /// `#stopfeedbacknoterror`: the exact Claude Code Stop-hook output for a
    /// queue continuation is non-error `additionalContext` feedback (stdout
    /// JSON, exit 0), never a `decision: "block"`, which Claude Code labels a
    /// hook error. The fail-closed branch keeps `decision: "block"` because a
    /// hook failure is an error.
    #[test]
    fn claude_continuation_output_is_non_error_feedback_json() {
        let output = ClaudeStopContinuation {
            reason: claude_stop_continuation_reason("/p/doc.md", "do [#x]"),
        }
        .to_hook_output();
        assert_eq!(
            output,
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "Stop",
                    "additionalContext": claude_stop_continuation_reason("/p/doc.md", "do [#x]"),
                }
            })
        );
        assert!(output.get("decision").is_none(), "{output}");
        assert!(output.get("reason").is_none(), "{output}");
    }

    /// Codex's protocol calls a continuation `decision: "block"`; our typed
    /// outcome keeps that host spelling separate from a true refusal and makes
    /// every continuation's model-visible reason say that it is not an error.
    #[test]
    fn codex_stop_output_shapes_distinguish_continuation_from_refusal() {
        assert_eq!(
            serde_json::to_value(StopResponse::ContinueTurn {
                class: StopContinuationClass::SessionWriteback,
                reason: "r".to_string(),
            })
            .unwrap(),
            serde_json::json!({
                "decision": "block",
                "reason": "Session persistence continuation, not a refusal or error: r"
            })
        );
        assert_eq!(
            serde_json::to_value(StopResponse::ContinueTurn {
                class: StopContinuationClass::Queue,
                reason: "q".to_string(),
            })
            .unwrap(),
            serde_json::json!({
                "decision": "block",
                "reason": "Queue continuation, not an error: q"
            })
        );
        assert_eq!(
            serde_json::to_value(StopResponse::ContinueTurn {
                class: StopContinuationClass::StatusSync,
                reason: "s".to_string(),
            })
            .unwrap(),
            serde_json::json!({
                "decision": "block",
                "reason": "Final status synchronization, not an error: s"
            })
        );
        assert_eq!(
            serde_json::to_value(StopResponse::ContinueTurn {
                class: StopContinuationClass::Recovery,
                reason: "x".to_string(),
            })
            .unwrap(),
            serde_json::json!({
                "decision": "block",
                "reason": "Recovery continuation, not a refusal or error: x"
            })
        );
        assert_eq!(
            serde_json::to_value(StopResponse::AllowExit).unwrap(),
            serde_json::json!({"continue": true})
        );
        assert_eq!(
            serde_json::to_value(StopResponse::RefuseExit {
                reason: "s".to_string()
            })
            .unwrap(),
            serde_json::json!({"continue": false, "stopReason": "s"})
        );
    }

    /// The continuation feedback is a harness record inside the turn, so it
    /// must not end the search for an armed re-entry (`#stoploopalreadyarmed`).
    #[test]
    fn continuation_feedback_is_not_an_operator_prompt() {
        let reason = claude_stop_continuation_reason("/p/doc.md", "do [#x]");
        assert!(is_stop_hook_feedback(&reason));
        assert!(is_stop_hook_feedback(
            "<system-reminder>\nStop hook feedback"
        ));
        assert!(!is_stop_hook_feedback("please fix the parser"));
    }

    /// `#statuspatchdrop` (c): on 2026-10-28 the final checkpoint was answered
    /// "no change" from the agent's memory of a `patch:status` the write path
    /// had dropped. The checkpoint now quotes the document's current payload.
    #[test]
    fn final_status_sync_checkpoint_quotes_the_current_payload() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            concat!(
                "<!-- agent:status sync=\"Keep the release current\" -->\n",
                "<!-- status:sync -->\nLatest release: v0.35.558\n<!-- /status:sync -->\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();

        let reason = final_status_sync_continuation(&doc, false)
            .unwrap()
            .expect("a sync directive surfaces the final checkpoint");
        assert!(reason.contains(FINAL_STATUS_SYNC_MARKER), "{reason}");
        assert!(reason.contains("> Latest release: v0.35.558"), "{reason}");
        assert!(
            reason.contains("not against what you remember writing"),
            "{reason}"
        );
        // The recursion bit still passes the following Stop through.
        assert_eq!(final_status_sync_continuation(&doc, true).unwrap(), None);
    }

    #[test]
    fn refused_admission_does_not_leak_another_panes_cycle_into_later_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_auto_queue_doc(&dir, &["fix queue retrieval"]);
        init_git_repo(dir.path(), &doc);
        let input = agent_doc_codex_hook_io::UserPromptSubmitInput {
            session_id: "codex-session".into(),
            turn_id: "refused-turn".into(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        };
        agent_doc_codex_hook_io::apply_user_prompt_submit(&input).unwrap();
        agent_doc_codex_hook_io::record_preflight_admission(&input, false).unwrap();

        // A later ordinary prompt must not replace the refusal with an active
        // document binding. That used to make Stop attempt the live owner's
        // commit and prescribe the same impossible command back to this pane.
        let newer = agent_doc_codex_hook_io::UserPromptSubmitInput {
            turn_id: "new-turn".into(),
            prompt: "Fix the admission defect".into(),
            ..input.clone()
        };
        agent_doc_codex_hook_io::apply_user_prompt_submit(&newer).unwrap();
        let (_, refused) = load_bound_session_for_stop(dir.path(), "codex-session")
            .unwrap()
            .unwrap();
        assert_eq!(refused.last_turn_id, "refused-turn");
        assert_eq!(refused.preflight_admitted, Some(false));

        let before = fs::read_to_string(&doc).unwrap();
        let result = apply_stop(&StopInput {
            session_id: newer.session_id.clone(),
            turn_id: newer.turn_id.clone(),
            cwd: newer.cwd.clone(),
            last_assistant_message: "Preflight refused; pending content is retained.".into(),
            stop_hook_active: false,
        })
        .unwrap();
        assert!(matches!(result, StopResponse::AllowExit));
        assert_eq!(fs::read_to_string(&doc).unwrap(), before);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        assert!(
            load_bound_session_for_stop(dir.path(), "codex-session")
                .unwrap()
                .is_none(),
            "Stop must retire a binding that never acquired the document"
        );
    }
    use std::fs;
    use std::process::Command as ProcessCommand;

    #[test]
    fn stop_hook_budget_override_is_positive_and_bounded_by_default() {
        assert!(
            STOP_HOOK_STATE_DB_BUSY_TIMEOUT < std::time::Duration::from_secs(STOP_HOOK_BUDGET_SECS),
            "exact-thread binding lookup must leave time for document inspection and a fail-closed response"
        );
        assert_eq!(
            resolve_stop_hook_budget(None),
            std::time::Duration::from_secs(STOP_HOOK_BUDGET_SECS)
        );
        assert_eq!(
            resolve_stop_hook_budget(Some("3")),
            std::time::Duration::from_secs(3)
        );
        assert_eq!(
            resolve_stop_hook_budget(Some("0")),
            std::time::Duration::from_secs(STOP_HOOK_BUDGET_SECS)
        );
    }

    #[test]
    fn stop_hook_deadline_defaults_to_the_wall_clock_budget_and_opts_into_worker_completion() {
        assert_eq!(
            resolve_stop_hook_deadline(None, None),
            Some(std::time::Duration::from_secs(STOP_HOOK_BUDGET_SECS))
        );
        assert_eq!(
            resolve_stop_hook_deadline(Some("anything-else"), Some("3")),
            Some(std::time::Duration::from_secs(3))
        );
        assert_eq!(
            resolve_stop_hook_deadline(Some(STOP_HOOK_DEADLINE_WORKER_COMPLETION), Some("3")),
            None
        );
    }

    /// `#stophookdeadlineflake`: with no wall-clock deadline the hook returns
    /// the worker's real response however long the worker takes. The worker
    /// is gated on a channel event from another thread, and no timer exists
    /// anywhere in the path, so the outcome cannot depend on scheduling.
    #[test]
    fn stop_hook_worker_completion_deadline_waits_for_the_actual_response() {
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let releaser = std::thread::spawn(move || {
            release.send(()).unwrap();
        });
        let run = run_stop_hook_task_within_budget(None, move || {
            gate.recv().unwrap();
            Ok(StopResponse::AllowExit)
        })
        .unwrap();
        releaser.join().unwrap();
        assert!(!run.timed_out);
        assert!(matches!(run.response, StopResponse::AllowExit));
    }

    #[test]
    fn stop_hook_budget_returns_a_fail_closed_response_before_outer_timeout() {
        let run =
            run_stop_hook_task_within_budget(Some(std::time::Duration::from_millis(1)), || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                Ok(StopResponse::AllowExit)
            })
            .unwrap();

        assert!(run.timed_out);
        assert!(matches!(run.response, StopResponse::RefuseExit { .. }));
    }

    #[test]
    fn stop_hook_timeout_flushes_response_and_terminates_the_process() {
        const CHILD_ENV: &str = "AGENT_DOC_CODEX_STOP_HOOK_TIMEOUT_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            handle_stop().unwrap();
            panic!("timed-out hook must terminate the process after flushing its response");
        }

        let mut child = ProcessCommand::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::stop_hook_timeout_flushes_response_and_terminates_the_process",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .env(STOP_HOOK_BUDGET_ENV, "1")
            // `#remainingwallclock`: the worker effectively never finishes
            // (10 minutes), so a hook that waited for it can only hang; the
            // ceiling below turns that into a failure. A hook that terminates
            // after flushing returns in about its 1s budget at any load.
            .env(STOP_HOOK_TEST_DELAY_MS_ENV, "600000")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                br#"{"session_id":"session","turn_id":"turn","cwd":"/tmp","last_assistant_message":"","stop_hook_active":false}"#,
            )
            .unwrap();

        const HANG_CEILING: std::time::Duration = std::time::Duration::from_secs(60);
        let child_pid = child.id();
        let (output_tx, output_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let _ = output_tx.send(child.wait_with_output());
        });
        let output = match output_rx.recv_timeout(HANG_CEILING) {
            Ok(output) => output.unwrap(),
            Err(_) => {
                let _ = ProcessCommand::new("kill")
                    .args(["-KILL", &child_pid.to_string()])
                    .status();
                let _ = waiter.join();
                panic!("timed-out hook waited for the detached worker past {HANG_CEILING:?}");
            }
        };
        waiter.join().unwrap();
        assert!(output.status.success(), "child failed: {output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("\"continue\":false"), "{stdout}");
        assert!(
            stdout.contains("exceeded its 1s internal budget"),
            "{stdout}"
        );
        // `#codexstopbudgetblind`: the EMITTED message must carry the phase
        // report, not just the ledger in isolation. Without this assertion the
        // report can be dropped from the message with the suite still green --
        // verified by mutation.
        assert!(
            stdout.contains("Phases:"),
            "a timed-out hook must report where its budget went: {stdout}"
        );
    }

    /// `#codexstopbudgetblind`: a timed-out hook must say where the budget went.
    ///
    /// The old message named only the budget, so an operator (and the agent
    /// reading the stop reason) learned that 45s elapsed and nothing else. A
    /// search of every log on the dogfood machine returned zero `codex_stop.`
    /// perf lines despite live overruns, because the only phase timer sat
    /// downstream of the work that is actually slow.
    #[test]
    fn a_timed_out_stop_hook_reports_the_phase_it_is_stuck_in() {
        let ledger = StopPhaseLedger::default();
        let worker = ledger.clone();
        // Two phases complete, the third is still running when the budget expires.
        worker.enter("load_bound_session");
        worker.enter("session_check_inspect");
        worker.enter("auto_queue_continuation");

        let rendered = ledger.render();
        assert!(
            rendered.contains("load_bound_session="),
            "a completed phase must carry its duration: {rendered}"
        );
        assert!(
            rendered.contains("session_check_inspect="),
            "every completed phase must be listed: {rendered}"
        );
        assert!(
            rendered.contains("auto_queue_continuation=RUNNING_"),
            "the phase still running is the one that matters — it must be \
             distinguishable from a completed one: {rendered}"
        );
        assert!(
            !rendered.contains("none recorded"),
            "a populated ledger must not render as empty: {rendered}"
        );
    }

    /// An unrecorded ledger still renders, rather than producing a message with
    /// a dangling `Phases:` and nothing after it.
    #[test]
    fn an_empty_phase_ledger_renders_explicitly() {
        assert_eq!(StopPhaseLedger::default().render(), "none recorded");
    }

    /// `#codexstopbudgetblind`: the hook asks for the same document from several
    /// places per invocation. On a live editor each resolution is a controller
    /// round-trip plus a full CRDT materialization — the dogfood ops log shows
    /// four for one 51KB document, all returning the identical `text_hash`.
    #[test]
    fn the_stop_hook_materializes_a_document_once_per_invocation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "---\nagent_doc_format: template\n---\n\nbody\n").unwrap();

        invalidate_stop_document_cache();
        let first = current_document_content(&doc, "test_first").unwrap();

        // Change the file underneath. A memoized read must still answer with the
        // content this invocation already resolved, which is what proves the
        // second call did not go back to the resolver.
        std::fs::write(&doc, "---\nagent_doc_format: template\n---\n\nCHANGED\n").unwrap();
        let second = current_document_content(&doc, "test_second").unwrap();
        assert_eq!(
            first, second,
            "the second read must be served from the invocation memo"
        );
        assert!(
            !second.contains("CHANGED"),
            "a memoized read must not re-resolve: {second}"
        );

        // A write inside the invocation drops the memo, so the next read is fresh.
        invalidate_stop_document_cache();
        let third = current_document_content(&doc, "test_third").unwrap();
        assert!(
            third.contains("CHANGED"),
            "invalidation must force a re-materialization: {third}"
        );
        invalidate_stop_document_cache();
    }

    /// Stale-memo receipt: a terminal-receipt read must observe the document
    /// the repair just wrote, not the pre-repair memo. Reading it through the
    /// memo compared pre-repair text against the post-commit hash, so the strict
    /// replay receipt never matched and the hook re-ran a full closeout.
    #[test]
    fn terminal_receipt_reads_bypass_the_pre_repair_memo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "---\nagent_doc_format: template\n---\n\nbefore\n").unwrap();

        invalidate_stop_document_cache();
        let before = current_document_content(&doc, "test_before_repair").unwrap();
        assert!(before.contains("before"));

        // Stand-in for the repair's write + commit.
        std::fs::write(&doc, "---\nagent_doc_format: template\n---\n\nrepaired\n").unwrap();
        let receipt = fresh_document_content(&doc, "test_terminal_receipt").unwrap();
        assert!(
            receipt.contains("repaired"),
            "the terminal receipt must see the repaired document: {receipt}"
        );
        // The fresh read re-seeds the memo with the post-write content.
        let after = current_document_content(&doc, "test_after_receipt").unwrap();
        assert_eq!(after, receipt);
        invalidate_stop_document_cache();
    }

    /// Both replay paths must drop the memo after repair and read the strict
    /// replay receipt fresh.
    #[test]
    fn replay_paths_read_the_terminal_receipt_after_invalidating_the_memo() {
        let source = include_str!("lib.rs");
        for (function, receipt_source) in [
            (
                "fn try_recover_repeated_queue_head_response(",
                "codex_stop_queue_replay_terminal_receipt",
            ),
            (
                "fn attempt_stop_closeout(",
                "codex_stop_replay_terminal_receipt",
            ),
        ] {
            let body = source.split(function).nth(1).unwrap();
            let body = &body[..body.find("\nfn ").unwrap()];
            let repair = body
                .find("run_with_queue_completion_ids(")
                .expect("path runs repair");
            let invalidate = body[repair..]
                .find("invalidate_stop_document_cache();")
                .map(|offset| repair + offset)
                .expect("repair must drop the pre-repair memo");
            let receipt = body
                .find(&format!(
                    "fresh_document_content(file, \"{receipt_source}\")"
                ))
                .expect("terminal receipt must be read fresh");
            assert!(
                invalidate < receipt,
                "{function} invalidates before the receipt"
            );
        }
    }

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        // Hermetic Codex transcript lookup: an opted-in Stop hook scans
        // `<home>/.codex/sessions`, and the operator's real one (GBs, thousands
        // of rollouts) made these tests take minutes and time out under load.
        // Thread-scoped (libtest runs each test on its own thread), so no
        // process-wide `HOME` mutation races parallel tests.
        std::mem::forget(
            agent_doc_codex_hook_io::override_codex_home_for_current_thread(
                &dir.path().join("codex-home"),
            ),
        );
        // `#stopreopenflake`: hermetic active Codex session. The session id is
        // read from process-global `CODEX_THREAD_ID`; a parallel test binding
        // it (or a run launched from a Codex pane) made an unrelated Stop see an
        // active session and reclassify post-commit drift. Pin "no session" on
        // this thread; tests that need one bind it with
        // `codex_session_bound_to_this_thread`, never `std::env::set_var`.
        std::mem::forget(
            agent_doc_codex_hook_io::override_codex_session_id_for_current_thread(None),
        );
        dir
    }

    /// Bind the active Codex session (`codex-session`, the id `track_doc`
    /// records) for the current test thread only (`#stopreopenflake`).
    fn codex_session_bound_to_this_thread() -> agent_doc_codex_hook_io::CodexSessionIdOverrideGuard
    {
        agent_doc_codex_hook_io::override_codex_session_id_for_current_thread(Some("codex-session"))
    }

    fn write_codex_mcp_config(root: &Path) {
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            format!(
                "[mcp_servers.agent-doc]\ncommand = \"agent-doc\"\ndefault_tools_approval_mode = \"approve\"\nargs = [\"mcp\", \"serve\"]\n# project root: {}\n",
                root.display()
            ),
        )
        .unwrap();
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        let relative = tracked.strip_prefix(root).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", relative.to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_template_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("task.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_status_sync_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("status-sync.md");
        let content = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status sync=\"Keep the published release current\" -->\n",
            "<!-- status:sync -->\nPublished: v1\n<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\nCompleted.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_auto_queue_doc(dir: &tempfile::TempDir, prompts: &[&str]) -> PathBuf {
        let doc = dir.path().join("task.md");
        let queue = prompts
            .iter()
            .map(|prompt| format!("- {prompt}\n"))
            .collect::<String>();
        let content = format!(
            "---\n\
session: sid\n\
agent_doc_format: template\n\
queue_active: true\n\
---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\n\
Done.\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue auto go -->\n\
{queue}\
<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    /// A queue whose head is live session work that the Stop hook's in-session
    /// continuation does not drive: `queue: start` is supervisor-scoped. With no
    /// control at all the queue would default to `go` and the continuation
    /// block would answer first (`#queuegodefaultdrain`).
    fn write_manual_queue_doc(dir: &tempfile::TempDir, prompts: &[&str]) -> PathBuf {
        let doc = dir.path().join("task.md");
        let queue = prompts
            .iter()
            .map(|prompt| format!("- {prompt}\n"))
            .collect::<String>();
        let content = format!(
            "---\n\
session: sid\n\
agent_doc_format: template\n\
queue: start\n\
---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\n\
Done.\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue -->\n\
{queue}\
<!-- /agent:queue -->\n\
<!-- no-free-text-queue-head-guard -->\n"
        );
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_nested_template_doc(dir: &tempfile::TempDir) -> PathBuf {
        let nested = dir.path().join("nested");
        fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn track_doc(dir: &tempfile::TempDir, doc: &Path, turn_id: &str) {
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: turn_id.to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();
    }

    #[test]
    fn stop_auto_closes_open_cycle_from_last_assistant_message() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "### Re: Hello — gpt-5\n\nFinal assistant response."
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);

        assert!(
            agent_doc_capture_io::load_active(&doc).unwrap().is_none(),
            "pending capture should be cleared after recovery"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Final assistant response."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
        let log = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline", "-1"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&log.stdout).contains("agent-doc(task):"),
            "expected auto-close commit, got: {}",
            String::from_utf8_lossy(&log.stdout)
        );
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(state.last_turn_id.is_empty());
        assert!(state.last_prompt.is_empty());
    }

    /// `#stoprecapturesupersede`: the 2026-09-28 `src/haiven-dev/tasks/api.md`
    /// shape. The cycle's real response is captured and visible; Codex then
    /// stops with a heading-less chat restatement. That restatement must not
    /// become the cycle's capture, or its replay replaces the visible answer.
    /// (Through `apply_stop` the captured-finalize resume normally closes the
    /// cycle first; the recapture only ran when that resume was blocked by a
    /// retained write, so the predicate is exercised directly.)
    #[test]
    fn a_visible_cycle_capture_blocks_recapturing_a_restatement() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        let real = "### Re: Hello — gpt-5\n\nThe real response, already written.\n";
        let restatement = "Nothing else to do; the answer is above.";

        // Nothing captured yet: recapture is allowed.
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, restatement).unwrap(),
            None
        );

        let capture = agent_doc_capture_io::capture_response(&doc, real).unwrap();
        // Captured but not yet visible: the Stop hook may still capture.
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, restatement).unwrap(),
            None
        );

        fs::write(
            &doc,
            original.replace("❯ Hello\n", &format!("❯ Hello\n\n{real}")),
        )
        .unwrap();
        invalidate_stop_document_cache();
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, restatement).unwrap(),
            Some(capture.capture_id.clone()),
            "a visible cycle response must not be superseded by the closing chat message"
        );
        // The message IS the visible response: nothing to protect.
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, real).unwrap(),
            None
        );

        agent_doc_cycle_state_io::retire_projected_captured_response(
            &doc,
            &capture.cycle_id,
            &capture.capture_id,
            "test",
        )
        .unwrap();
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, restatement).unwrap(),
            None
        );
    }

    /// `#retainedevidencerepair`: a response capture that reached the durable
    /// ledger without its selected free-text quote must be replaceable by the
    /// authoritative owner's corrected response. Another incomplete closing
    /// restatement must not churn the capture hash and wake duplicate replays.
    #[test]
    fn owner_quote_repair_replaces_retained_capture_and_fences_stale_resume() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- 🚧 I cannot login.\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        agent_doc_cycle_state_io::record_selected_free_text_queue_heads(
            &doc,
            &["I cannot login.".to_string()],
        )
        .unwrap();

        let incomplete = "### Re: Hello — gpt-5\n\nI fixed the login flow.\n";
        agent_doc_repair_io::pending::save_pending(&doc, incomplete).unwrap();
        let old_key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .unwrap();

        assert_eq!(
            materialized_cycle_capture_supersedes(
                &doc,
                "### Re: Hello — gpt-5\n\nThe login flow is fixed.\n",
            )
            .unwrap(),
            Some(old_key.capture_id.clone()),
            "another response missing the selected queue quote must not churn the retained capture",
        );

        // Model the live incident: disk still has the pre-write projection,
        // while the editor owns a newer cut. The predicate and replacement
        // baseline must both use that authoritative cut without forcing disk.
        let authoritative = original.replace(
            "<!-- /agent:queue -->\n",
            "<!-- /agent:queue -->\n\nOperator draft stays authoritative.\n",
        );
        invalidate_stop_document_cache();
        STOP_DOCUMENT_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .insert(doc.clone(), authoritative.clone());
        });
        let corrected = concat!(
            "### Re: Hello — gpt-5\n\n",
            "> **Queue prompt:** I cannot login.\n\n",
            "I fixed the login flow.\n",
        );
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, corrected).unwrap(),
            None,
            "the owner's exact queue quote must authorize same-cycle repair",
        );
        agent_doc_repair_io::pending::save_pending_with_current_content(
            &doc,
            corrected,
            &authoritative,
        )
        .unwrap();

        let repaired = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
        let repaired_key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(repaired.capture_id, old_key.capture_id);
        assert_eq!(repaired.cycle_id, old_key.cycle_id);
        assert_ne!(repaired.response_sha256, old_key.response_sha256);
        assert!(
            repaired
                .response_body
                .contains("> **Queue prompt:** I cannot login.")
        );
        assert_eq!(
            repaired.baseline_content.as_deref(),
            Some(authoritative.as_str())
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            original,
            "capture repair must preserve editor/disk authority until normal replay",
        );
        assert_eq!(
            agent_doc_repair_command_io::resume_captured_finalize(&doc, &old_key),
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded,
            "the old response hash must fence a stale replay worker",
        );
        assert_eq!(
            repaired_key.capture_id, old_key.capture_id,
            "repair replaces the response under the exact same capture operation",
        );
        assert_ne!(repaired_key.response_sha256, old_key.response_sha256);
        invalidate_stop_document_cache();
    }

    /// Prompt-preset heads use the same retained-repair contract, but their
    /// mandatory evidence is the resolved expansion rather than only a literal
    /// head echo (`#presetretainedevidencerepair`).
    #[test]
    fn owner_preset_expansion_repair_replaces_retained_capture() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let head = "#gh-fix https://github.com/btakita/agent-doc/issues/104";
        let original = format!(
            concat!(
                "---\nsession: sid\nagent_doc_format: template\n",
                "prompt_presets:\n  '#gh-fix': fix then closerelease\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ Why is this taking so long?\n",
                "<!-- /agent:exchange -->\n\n",
                "## Queue\n\n",
                "<!-- agent:queue -->\n",
                "- 🚧 {}\n",
                "<!-- /agent:queue -->\n",
            ),
            head,
        );
        fs::write(&doc, &original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::record_selected_free_text_queue_heads(&doc, &[head.to_string()])
            .unwrap();

        let incomplete = "### Re: timing — gpt-5\n\nThe work is still running.\n";
        agent_doc_repair_io::pending::save_pending(&doc, incomplete).unwrap();
        let old_key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            materialized_cycle_capture_supersedes(
                &doc,
                "### Re: timing — gpt-5\n\nThe release is almost ready.\n",
            )
            .unwrap(),
            Some(old_key.capture_id.clone()),
            "another answer missing the resolved preset expansion must not churn the capture",
        );

        let corrected = format!(
            concat!(
                "### Re: GH #104 — gpt-5\n\n",
                "> **Queue prompt:** {}\n\n",
                "fix then closerelease\n\n",
                "GH #104 is fixed, fully tested, installed, and ready to close and release.\n",
            ),
            head,
        );
        assert_eq!(
            materialized_cycle_capture_supersedes(&doc, &corrected).unwrap(),
            None,
            "resolved preset evidence from the owner must authorize same-cycle repair",
        );
        agent_doc_repair_io::pending::save_pending(&doc, &corrected).unwrap();

        let repaired = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
        let repaired_key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(repaired.capture_id, old_key.capture_id);
        assert_eq!(repaired.cycle_id, old_key.cycle_id);
        assert_ne!(repaired.response_sha256, old_key.response_sha256);
        assert!(repaired.response_body.contains(head));
        assert!(repaired.response_body.contains("fix then closerelease"));
        assert_eq!(fs::read_to_string(&doc).unwrap(), original);
        assert_eq!(
            agent_doc_repair_command_io::resume_captured_finalize(&doc, &old_key),
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded,
        );
        assert_eq!(repaired_key.capture_id, old_key.capture_id);
        assert_ne!(repaired_key.response_sha256, old_key.response_sha256);
    }

    #[test]
    fn both_stop_capture_sites_consult_the_existing_capture_first() {
        let source = include_str!("lib.rs");
        // `#stopretainedfailclosed`: a repair error after capture must become a still-open
        // closeout, never a `?` that reaches the "failed closed" catch-all.
        let closeout = source.split("fn attempt_stop_closeout(").nth(1).unwrap();
        let closeout = &closeout[..closeout.find("\nfn ").unwrap()];
        let repair = closeout
            .split("run_with_queue_completion_ids(")
            .nth(1)
            .expect("closeout runs repair");
        let call_end = repair.find("\n    )").expect("repair call closes");
        assert!(
            !repair[call_end..].starts_with("\n    )?"),
            "attempt_stop_closeout must not propagate a repair error with `?`"
        );
        assert!(closeout.contains("closeout_repair_retained_note(&err, ownership, file)"));
        assert!(closeout.contains("repair_failure_is_binary_owned_deferral("));
        assert!(closeout.contains("StopCloseAttempt::RepairDeferredToDurableOwner"));

        for anchor in ["fn attempt_stop_closeout(", "fn capture_assistant_text("] {
            let body = source.split(anchor).nth(1).unwrap();
            let body = &body[..body.find("\nfn ").unwrap()];
            let guard = body
                .find("materialized_cycle_capture_supersedes(")
                .unwrap_or(usize::MAX);
            let save = body.find("pending::save_pending(").unwrap();
            assert!(
                guard < save,
                "{anchor} must check the existing capture before save_pending"
            );
        }
    }

    /// haiven-dev cycle-1790914570041 (2026-10-02): the post-commit closeout
    /// committed before the retained capture was projected, the commit refused
    /// with the missing-captured-response guard, and the hook surfaced a hard
    /// block for a state the keyed retry committed on its own 15s later.
    #[test]
    fn missing_captured_response_refusal_with_durable_owner_is_a_deferral() {
        use agent_doc_turn::write_ownership::{RetainedWriteOwnership, RetainedWriteVerdict};
        let refusal = format!(
            "captured response body is not present in the staged snapshot for /p/sdk.md even though the snapshot already matches HEAD; refusing already-committed closeout ({}). The retained capture or projection is already durable.",
            agent_doc_git_io::capture_materialization_guard::MISSING_CAPTURED_RESPONSE_REFUSAL_TOKEN,
        );
        let owned = RetainedWriteOwnership::new_with_phase(true, true, false);
        assert_eq!(owned.verdict(), RetainedWriteVerdict::Deferred);
        assert_eq!(
            closeout_failure_disposition(&refusal, owned.verdict()),
            CloseoutFailureDisposition::DeferToDurableOwner,
        );

        // Fail closed whenever nothing durable owns the intent.
        for verdict in [
            RetainedWriteVerdict::Stranded,
            RetainedWriteVerdict::CaptureResumeUnowned,
            RetainedWriteVerdict::AwaitingTerminalCommit,
            RetainedWriteVerdict::UnansweredEditPending,
            RetainedWriteVerdict::DeliveryRejected,
            RetainedWriteVerdict::EditorHoldsUnmergedVisibleText,
        ] {
            assert_eq!(
                closeout_failure_disposition(&refusal, verdict),
                CloseoutFailureDisposition::FailClosed,
                "{verdict:?} must stay fail-closed",
            );
        }
        // Any other commit failure stays fail-closed even with a durable owner.
        assert_eq!(
            closeout_failure_disposition(
                "git commit failed: index.lock exists",
                RetainedWriteVerdict::Deferred,
            ),
            CloseoutFailureDisposition::FailClosed,
        );
    }

    /// `#stopnextedit`: tasks/software/tsift.md, 2026-10-21. Codex Stop had
    /// already committed the response while the operator edited a later queue
    /// line. The final commit correctly returned the typed
    /// `UnansweredEditPending` refusal, but Stop erased that type and wrapped
    /// the refusal as "could not finish closeout", contradicting its own END
    /// THIS TURN remedy and producing another hook-blocked model turn.
    #[test]
    fn unanswered_typed_component_refusal_is_next_cycle_stop_state() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &["current work", "release + publish: include P"]);
        init_git_repo(dir.path(), &doc);

        let committed = fs::read_to_string(&doc).unwrap();
        let edited = committed.replace(
            "release + publish: include P",
            "release + publish: include Issue #29",
        );
        fs::write(&doc, edited).unwrap();

        let err = agent_doc_commit_io::commit(&doc)
            .expect_err("a newer queue edit must stay uncommitted for the next cycle");
        assert!(
            unanswered_edit_pending_commit_refusal(&err),
            "Stop must preserve the commit layer's typed next-cycle verdict: {err:#}"
        );
        let rendered = format!("{err:#}");
        assert!(rendered.contains("END THIS TURN"), "{rendered}");
        assert!(rendered.contains("UNANSWERED DOCUMENT EDIT"), "{rendered}");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .expect("committed document");
        assert!(head.contains("release + publish: include P"), "{head}");
        assert!(!head.contains("Issue #29"), "{head}");
        assert!(
            fs::read_to_string(&doc).unwrap().contains("Issue #29"),
            "the operator edit must remain live for the next cycle"
        );
    }

    #[test]
    fn stop_closeout_classifies_commit_failure_before_reporting_still_open() {
        let source = include_str!("lib.rs");
        let closeout = source.split("fn attempt_stop_closeout(").nth(1).unwrap();
        let closeout = &closeout[..closeout.find("\nfn ").unwrap()];
        let commit = closeout
            .split("complete_required_closeout(file, false)")
            .nth(1)
            .expect("closeout crosses the commit boundary");
        let next_cycle = commit
            .find("unanswered_edit_pending_commit_refusal(&err)")
            .expect("typed unanswered edits must be preserved for the next cycle");
        let deferral = commit
            .find("closeout_failure_disposition(")
            .expect("commit failure must consult the durable-owner deferral");
        let still_open = commit
            .find("StopCloseAttempt::StillOpen")
            .expect("unowned failures stay still-open");
        assert!(next_cycle < deferral);
        assert!(deferral < still_open);
        assert!(commit.contains("StopCloseAttempt::UnansweredEditPending"));
        assert!(commit.contains("StopCloseAttempt::DeferredToDurableOwner"));
        assert!(commit.contains("CloseoutFailureDisposition::AwaitDurableOwner"));
        assert!(commit.contains("StopCloseAttempt::RepairDeferredToDurableOwner"));
    }

    #[test]
    fn retained_repair_refusal_with_durable_owner_is_a_distinct_deferral() {
        use agent_doc_turn::write_ownership::{
            AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN, RetainedWriteOwnership, RetainedWriteVerdict,
        };
        let retry_key = "1790972821780862047-1-b19f206141461c9678dcc6f01db3826c43654fdd137ef7b7fcb00ecaa416e667";
        let refusal = format!(
            "serialized_atomic_write: retained editor-owned write while its native save receipt converges (intent_id={retry_key}) [{AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN}]"
        );
        let owned = RetainedWriteOwnership::new_with_phase(true, true, false)
            .with_retained_projection(true);
        assert_eq!(owned.verdict(), RetainedWriteVerdict::Deferred);
        assert!(repair_failure_is_binary_owned_deferral(
            &refusal,
            owned.verdict(),
        ));

        assert!(!repair_failure_is_binary_owned_deferral(
            "pre-write validation rejected missing queue prompt evidence",
            RetainedWriteVerdict::Deferred,
        ));
        assert!(!repair_failure_is_binary_owned_deferral(
            &refusal,
            RetainedWriteVerdict::CaptureResumeUnowned,
        ));

        let err = anyhow::anyhow!(refusal);
        let note = closeout_repair_retained_note(&err, owned, Path::new("/p/fpe.md"));
        let first = durable_owner_repair_deferral_response(Path::new("/p/fpe.md"), &note, false);
        assert!(matches!(first, StopResponse::ContinueTurn { .. }));
        let recursive = durable_owner_repair_deferral_response(Path::new("/p/fpe.md"), &note, true);
        assert!(matches!(recursive, StopResponse::RefuseExit { .. }));
        let rendered = serde_json::to_string(&recursive).unwrap();
        assert!(
            rendered.contains(retry_key),
            "stable retry key preserved: {rendered}"
        );
        assert!(rendered.contains("Do not reopen or recapture the cycle"));
        assert!(rendered.contains("Wait for the existing controller state edge"));
        assert!(rendered.contains("this exact retained intent"));
        assert!(rendered.contains("different cycle"));
        assert!(
            !rendered.contains("Run `agent-doc session-check /p/fpe.md`"),
            "an unkeyed current-cycle check can race queue advancement: {rendered}"
        );
        assert!(rendered.contains("needs_operator"));
        assert!(!rendered.contains("run `agent-doc repair"));
    }

    #[test]
    fn captured_finalize_owner_race_is_a_durable_deferral_not_an_unfinished_cycle() {
        use agent_doc_turn::write_ownership::{RetainedWriteOwnership, RetainedWriteVerdict};

        let refusal = format!(
            "closeout operation is already in progress for cycle cycle-1 by captured-finalize-1 pid=7 role=captured_finalize_resume until=123456 [{}123456]",
            agent_doc_write_runtime_io::CLOSEOUT_OWNER_RETRY_AT_MARKER,
        );
        let owned = RetainedWriteOwnership::new_with_phase(true, true, false);
        assert_eq!(owned.verdict(), RetainedWriteVerdict::Deferred);
        assert!(repair_failure_is_binary_owned_deferral(
            &refusal,
            owned.verdict(),
        ));
        assert_eq!(
            closeout_failure_disposition(&refusal, owned.verdict()),
            CloseoutFailureDisposition::AwaitDurableOwner,
            "the same owned lease must wait whether repair or final closeout observes it",
        );

        let note = closeout_repair_retained_note(
            &anyhow::anyhow!(refusal),
            owned,
            Path::new("/p/sample.md"),
        );
        let response =
            durable_owner_repair_deferral_response(Path::new("/p/sample.md"), &note, false);
        let rendered = serde_json::to_string(&response).unwrap();
        assert!(rendered.contains("durable keyed retry"));
        assert!(rendered.contains("Wait for the existing controller state edge"));
        assert!(!rendered.contains("unfinished document cycle"));
        assert!(!rendered.contains("run `agent-doc repair"));

        assert!(
            !repair_failure_is_binary_owned_deferral(
                &format!(
                    "closeout operation is already in progress [{}123456]",
                    agent_doc_write_runtime_io::CLOSEOUT_OWNER_RETRY_AT_MARKER,
                ),
                RetainedWriteVerdict::CaptureResumeUnowned,
            ),
            "a lease marker without retained capture ownership must remain fail-closed",
        );
        assert_eq!(
            closeout_failure_disposition(
                &format!(
                    "closeout operation is already in progress [{}123456]",
                    agent_doc_write_runtime_io::CLOSEOUT_OWNER_RETRY_AT_MARKER,
                ),
                RetainedWriteVerdict::CaptureResumeUnowned,
            ),
            CloseoutFailureDisposition::FailClosed,
            "the final closeout phase must also fail closed without retained ownership",
        );
    }

    #[test]
    fn durable_owner_progress_waits_for_terminal_before_surfacing_a_blocker() {
        use agent_doc_closeout_runtime_io::CloseoutCycleWaitOutcome;

        let decision = await_durable_owner_progress_with(
            Path::new("/p/sample.md"),
            "cycle-1",
            |file, cycle_id| {
                assert_eq!(file, Path::new("/p/sample.md"));
                assert_eq!(cycle_id, "cycle-1");
                Ok(CloseoutCycleWaitOutcome::Terminal)
            },
        )
        .unwrap();
        assert_eq!(decision, DurableOwnerProgressDecision::Reinspect);
        assert_eq!(
            durable_owner_progress_decision(CloseoutCycleWaitOutcome::Terminal),
            DurableOwnerProgressDecision::Reinspect,
        );
        assert_eq!(
            durable_owner_progress_decision(CloseoutCycleWaitOutcome::Superseded),
            DurableOwnerProgressDecision::Reinspect,
        );
        assert_eq!(
            durable_owner_progress_decision(CloseoutCycleWaitOutcome::OwnerReleased),
            DurableOwnerProgressDecision::ResumeThenReinspect,
        );
        assert_eq!(
            durable_owner_progress_decision(CloseoutCycleWaitOutcome::TimedOut),
            DurableOwnerProgressDecision::StillPending,
        );
        assert!(
            !matches!(
                durable_owner_progress_decision(CloseoutCycleWaitOutcome::Terminal),
                DurableOwnerProgressDecision::StillPending
            ),
            "a terminal controller edge must be re-inspected instead of rendered as Blocked by hook",
        );
    }

    /// Regression for `agent-doc-bugs.md` cycle `cycle-1792271879541`:
    /// owner release woke the Stop hook, the exact resume reached a pre-write
    /// `needs_operator` verdict, and the boolean resume boundary discarded that
    /// verdict and rendered the superseded owner-contention note instead.
    #[test]
    fn durable_owner_resume_surfaces_needs_operator_instead_of_stale_wait() {
        let file = Path::new("/p/agent-doc-bugs.md");
        let reason = "[finalize] pre-write gate: active prompt requested backlog capture but no backlog mutations were recorded this cycle";

        let response = captured_finalize_needs_operator_response(file, reason, false);
        let rendered = serde_json::to_string(&response).unwrap();
        assert!(rendered.contains("reached `needs_operator`"), "{rendered}");
        assert!(rendered.contains(reason), "{rendered}");
        assert!(
            rendered.contains("not an in-progress owner wait"),
            "{rendered}"
        );
        assert!(!rendered.contains("Wait for the existing controller state edge"));
        assert!(!rendered.contains("durable keyed retry"));

        assert_eq!(
            captured_finalize_needs_operator_response(file, reason, true),
            StopResponse::AllowExit,
            "the recursive Stop must let the exact operator-blocker report through",
        );
    }

    /// `#sdkretained` (haiven-dev `tasks/sdk.md`, cycle-1791805221920,
    /// 2026-10-12): the gate fired on the post-commit compact projection, then
    /// re-read the WRITE intent to name it, found none, and told the agent to
    /// wait on a placeholder id no controller edge carries. The holder is now
    /// the one the gate observed, and a wait always names it.
    #[test]
    fn retained_queue_deferral_names_the_observed_holder_never_a_placeholder() {
        use agent_doc_document_realtime_io::RetainedSessionCloseoutHolder;
        let file = Path::new("/p/sdk.md");
        let compact = RetainedSessionCloseoutHolder::CompactProjection {
            continuation_id: "compact-projection-24db9b85-721b0afe".into(),
        };
        for stop_hook_active in [false, true] {
            let rendered = serde_json::to_string(&retained_write_queue_deferral_response(
                file,
                &compact,
                stop_hook_active,
            ))
            .unwrap();
            assert!(
                rendered.contains("continuation_id=compact-projection-24db9b85-721b0afe"),
                "{rendered}"
            );
            assert!(rendered.contains("compact projection"), "{rendered}");
            assert!(
                rendered.contains("compact_projection_completed"),
                "{rendered}"
            );
            assert!(!rendered.contains("intent_id="), "{rendered}");
        }
        let write = RetainedSessionCloseoutHolder::WriteIntent {
            intent_id: "1791805765963133062-1-24db".into(),
        };
        let rendered =
            serde_json::to_string(&retained_write_queue_deferral_response(file, &write, false))
                .unwrap();
        assert!(
            rendered.contains("intent_id=1791805765963133062-1-24db"),
            "{rendered}"
        );

        // The placeholder id is gone from the hook entirely.
        let placeholder = concat!("unobserved", "-retained-intent");
        assert!(!include_str!("lib.rs").contains(placeholder));
    }

    /// `#sdkretained`: an observed write intent the ownership predicate proves
    /// cannot converge (the endpoint answered NO, or the editor is not serving
    /// its replica) gets that verdict's terminating recovery, not a wait.
    #[test]
    fn retained_queue_gate_names_recovery_when_the_holder_cannot_converge() {
        use agent_doc_turn::write_ownership::RetainedWriteVerdict;
        let file = Path::new("/p/sdk.md");
        let rejected = serde_json::to_string(&retained_write_queue_recovery_response(
            file,
            "intent-1",
            RetainedWriteVerdict::DeliveryRejected,
            true,
        ))
        .unwrap();
        assert!(rejected.contains("intent_id=intent-1"), "{rejected}");
        assert!(rejected.contains("verdict=delivery_rejected"), "{rejected}");
        assert!(
            rejected.contains("REJECTED the delivery receipt"),
            "{rejected}"
        );
        assert!(!rejected.contains("wait for the existing controller state edge"));

        let unserved = serde_json::to_string(&retained_write_queue_recovery_response(
            file,
            "intent-2",
            RetainedWriteVerdict::ReplicaUnserved,
            false,
        ))
        .unwrap();
        assert!(unserved.contains("admin reload-lib"), "{unserved}");

        let stranded = serde_json::to_string(&retained_write_queue_recovery_response(
            file,
            "intent-3",
            RetainedWriteVerdict::Stranded,
            false,
        ))
        .unwrap();
        assert!(stranded.contains("UNREGISTERED"), "{stranded}");

        // `#splicesessionack`: an editor that refuses the save after every
        // replica rebuild names the operator reconcile, not a wait.
        let unmerged = serde_json::to_string(&retained_write_queue_recovery_response(
            file,
            "intent-4",
            RetainedWriteVerdict::EditorHoldsUnmergedVisibleText,
            false,
        ))
        .unwrap();
        assert!(
            unmerged.contains("verdict=editor_holds_unmerged_visible_text"),
            "{unmerged}"
        );
        assert!(
            unmerged.contains("operator_action=reconcile_editor_buffer_with_retained_write"),
            "{unmerged}"
        );

        // The gate consults the shared predicate before rendering a wait.
        let source = include_str!("lib.rs");
        let continuation = source
            .split("fn auto_queue_continuation_response(")
            .nth(1)
            .unwrap();
        let continuation = &continuation[..continuation.find("\nfn ").unwrap()];
        let predicate = continuation
            .find("retained_queue_gate(")
            .expect("the gate derives from write_ownership");
        let wait = continuation
            .find("retained_write_queue_deferral_response(")
            .unwrap();
        assert!(predicate < wait);
        assert!(continuation.contains("retained_write_ownership(file)"));
    }

    /// fpe.md (2026-10-02): the response cycle committed while its exact
    /// editor-native save remained retained, and recursive Stop selected the
    /// next auto-queue prompt. The retained write must gate before queue-head
    /// observation or session-state advancement.
    #[test]
    fn retained_editor_delivery_pauses_auto_queue_before_prompt_selection() {
        let source = include_str!("lib.rs");
        let continuation = source
            .split("fn auto_queue_continuation_response(")
            .nth(1)
            .unwrap();
        let continuation = &continuation[..continuation.find("\nfn ").unwrap()];
        let retained_gate = continuation
            .find("retained_session_closeout_holder(")
            .expect("auto queue must inspect retained editor delivery");
        let prompt_selection = continuation
            .find("active_auto_queue_prompt(file)")
            .expect("auto queue selects a prompt");
        let state_advance = continuation
            .find("save_state_across_roots(")
            .expect("auto queue records its selected head");
        assert!(retained_gate < prompt_selection);
        assert!(retained_gate < state_advance);
        assert!(continuation.contains("queue_advanced=false capture_created=false"));
        assert!(continuation.contains("codex_stop_retained_predecessor_queue_child_quarantined"));
        assert!(continuation.contains("repaired_state.last_auto_queue_head = None"));

        let prompts =
            ["I want to demo chatting and triggering the FPE then it showing up in the dashboard."];
        assert!(should_quarantine_retained_queue_child(
            agent_doc_turn::CyclePhase::PreflightStarted,
            false,
            false,
            "cycle-1790973209870",
            Some("cycle-1790972278134"),
            Some(prompts[0]),
            prompts.iter().copied(),
        ));
        for unsafe_shape in [
            should_quarantine_retained_queue_child(
                agent_doc_turn::CyclePhase::ResponseCaptured,
                true,
                true,
                "child",
                Some("parent"),
                Some(prompts[0]),
                prompts.iter().copied(),
            ),
            should_quarantine_retained_queue_child(
                agent_doc_turn::CyclePhase::PreflightStarted,
                false,
                false,
                "same-cycle",
                Some("same-cycle"),
                Some(prompts[0]),
                prompts.iter().copied(),
            ),
            should_quarantine_retained_queue_child(
                agent_doc_turn::CyclePhase::PreflightStarted,
                false,
                false,
                "child",
                Some("parent"),
                Some("a different queue head"),
                prompts.iter().copied(),
            ),
        ] {
            assert!(
                !unsafe_shape,
                "only the proven empty auto-queue child is disposable"
            );
        }

        let retry_key = "1790972821780862047-1-b19f206141461c9678dcc6f01db3826c43654fdd137ef7b7fcb00ecaa416e667";
        for stop_hook_active in [false, true] {
            let response = retained_write_queue_deferral_response(
                Path::new("/p/fpe.md"),
                &agent_doc_document_realtime_io::RetainedSessionCloseoutHolder::WriteIntent {
                    intent_id: retry_key.to_string(),
                },
                stop_hook_active,
            );
            assert_eq!(
                matches!(&response, StopResponse::RefuseExit { .. }),
                stop_hook_active,
            );
            let rendered = serde_json::to_string(&response).unwrap();
            assert!(rendered.contains(retry_key));
            assert!(rendered.contains("did not select a new prompt or create another capture"));
            assert!(rendered.contains("same owner pane continues only after"));
            assert!(rendered.contains("needs_operator"));
        }
    }

    /// `#stopcurrentqueuehead`: a native editor save can lag the authoritative
    /// CRDT projection after closeout has reaped the completed head. The Stop
    /// hook must continue from that authoritative projection instead of
    /// re-serving the stale on-disk head.
    #[test]
    fn auto_queue_prompt_uses_editor_authoritative_content_over_stale_disk() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let document_with_head = |head: &str| {
            format!(
                "---\nsession: sid\nagent_doc_format: template\n---\n\n\
                 ## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n\
                 ## Queue\n\n<!-- agent:queue go -->\n- {head}\n<!-- /agent:queue -->\n"
            )
        };
        let stale_disk = document_with_head("🚧 do [#backlogqueueitems]");
        let authoritative = document_with_head("do [#rsprereq]");
        fs::write(&doc, stale_disk).unwrap();

        assert_eq!(
            active_auto_queue_prompt_for_content(&doc, &authoritative)
                .unwrap()
                .as_deref(),
            Some("do [#rsprereq]")
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            document_with_head("🚧 do [#backlogqueueitems]"),
            "Stop-hook observation must not force the editor-owned cut to disk"
        );
        let source = include_str!("lib.rs");
        let resolver = source
            .split("fn active_auto_queue_prompt(file: &Path)")
            .nth(1)
            .unwrap();
        let resolver = &resolver[..resolver.find("\nfn ").unwrap()];
        assert!(resolver.contains("resolve_document_content_uncached("));
        assert!(resolver.contains("active_auto_queue_prompt_for_content(file, &content)"));
    }

    #[test]
    fn retained_predecessor_quarantines_already_created_empty_queue_child_metadata_only() {
        let dir = setup_project();
        let prompt =
            "I want to demo chatting and triggering the FPE then it showing up in the dashboard.";
        let doc = write_auto_queue_doc(&dir, &[prompt]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();

        let parent =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        agent_doc_repair_io::pending::save_pending(
            &doc,
            "### Re: prior retained response — gpt-5\n\nCompleted once.\n",
        )
        .unwrap();
        let retained_target = original.replace("Done.\n", "Done.\n\nCompleted once.\n");
        let intent_id = agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &doc,
            &original,
            &retained_target,
            "serialized_atomic_write",
            agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();

        let child =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        assert_ne!(child.cycle_id, parent.cycle_id);
        agent_doc_cycle_state_io::record_selected_free_text_queue_heads(
            &doc,
            &[prompt.to_string()],
        )
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let state = SessionState {
            identity_origin: Default::default(),
            harness: None,
            session_id: "codex-session".to_string(),
            doc_path: doc.display().to_string(),
            last_turn_id: "turn-child".to_string(),
            last_prompt: format!("agent-doc {}", doc.display()),
            last_auto_queue_head: Some(prompt.to_string()),
            last_context_clear_at: None,
            last_prompt_cycle: None,
            preflight_admitted: None,
            updated_at: 20,
        };
        save_state(&root, &state).unwrap();
        let roots = tracking_roots(dir.path(), Some(&doc));
        let response = auto_queue_continuation_response(
            &doc,
            &roots,
            &root,
            &state,
            &StopInput {
                session_id: "codex-session".to_string(),
                turn_id: "turn-child".to_string(),
                cwd: dir.path().display().to_string(),
                last_assistant_message: "prior closeout status".to_string(),
                stop_hook_active: true,
            },
        )
        .unwrap()
        .expect("retained predecessor blocks queue continuation");
        let rendered = serde_json::to_string(&response).unwrap();
        assert!(rendered.contains(&intent_id), "{rendered}");
        assert!(rendered.contains("did not select a new prompt or create another capture"));

        let quarantined = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(quarantined.cycle_id, child.cycle_id);
        assert_eq!(quarantined.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(
            quarantined.last_event,
            "codex_stop_retained_predecessor_queue_child_quarantined",
        );
        let pending = agent_doc_document_realtime_io::pending_document_write(&doc)
            .expect("quarantine preserves the predecessor retained write");
        assert_eq!(pending.intent_id, intent_id);
        assert_eq!(
            pending.continuation.as_ref().map(|it| it.cycle_id.as_str()),
            Some(parent.cycle_id.as_str()),
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), original);
        assert_eq!(
            load_state(&root, "codex-session")
                .unwrap()
                .unwrap()
                .last_auto_queue_head,
            None,
        );
    }

    #[test]
    fn closeout_repair_failure_note_carries_error_and_derived_remedy() {
        // `#stopretainedfailclosed`
        let err = anyhow::anyhow!("editor projection pending").context(
            "serialized_atomic_write: retained editor-owned write for /p/fpe.md before retrying \
             live model reconciliation (intent_id=abc)",
        );
        let owned = agent_doc_turn::write_ownership::RetainedWriteOwnership {
            cycle_open: true,
            retained_capture: true,
            write_applied: false,
            retained_projection: true,
            unanswered_edit: false,
            capture_resume_unowned: false,
            delivery_rejected: false,
            editor_route_unowned: false,
            replica_unserved: false,
            editor_holds_unmerged_visible_text: false,
        };
        let note = closeout_repair_retained_note(&err, owned, Path::new("/p/fpe.md"));
        assert!(note.contains("intent_id=abc"), "{note}");
        assert!(
            note.contains("editor projection pending"),
            "full chain kept: {note}"
        );
        assert!(
            note.contains(&agent_doc_turn::write_ownership::retained_write_remedy(
                owned,
                "/p/fpe.md"
            )),
            "{note}"
        );
        assert!(!note.contains('\n'), "one line: {note}");
    }

    #[test]
    fn closeout_repair_failure_note_does_not_repeat_an_embedded_remedy() {
        let owned = agent_doc_turn::write_ownership::RetainedWriteOwnership {
            cycle_open: true,
            retained_capture: true,
            write_applied: false,
            retained_projection: true,
            unanswered_edit: false,
            capture_resume_unowned: false,
            delivery_rejected: false,
            editor_route_unowned: false,
            replica_unserved: false,
            editor_holds_unmerged_visible_text: false,
        };
        let remedy = agent_doc_turn::write_ownership::retained_write_remedy(owned, "/p/fpe.md");
        let err = anyhow::anyhow!("editor projection pending. {remedy}");

        let note = closeout_repair_retained_note(&err, owned, Path::new("/p/fpe.md"));

        assert_eq!(note.matches(&remedy).count(), 1, "{note}");
    }

    #[test]
    fn ordinary_same_thread_followup_after_clean_closeout_auto_closes_fresh_cycle() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let first_stop = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Initial closeout.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();
        assert_eq!(first_stop, StopResponse::AllowExit);
        let first_cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

        let prompt = "Why did you not acknowledge this in the agent document?";
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: prompt.to_string(),
        })
        .unwrap();

        let followup_stop = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "I only acknowledged it in chat.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();
        assert_eq!(
            followup_stop,
            StopResponse::AllowExit,
            "same-thread follow-up should cross the binary-owned write boundary automatically"
        );
        let followup_cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(followup_cycle.phase.as_str(), "committed");
        assert_ne!(followup_cycle.cycle_id, first_cycle.cycle_id);
        assert!(
            fs::read_to_string(&doc)
                .unwrap()
                .contains("I only acknowledged it in chat."),
            "follow-up response was not persisted in the agent document"
        );
        let capture = agent_doc_capture_io::load_by_id(&doc, &followup_cycle.cycle_id)
            .unwrap()
            .expect("follow-up capture under fresh cycle");
        assert_eq!(capture.cycle_id, followup_cycle.cycle_id);
    }

    #[test]
    fn stop_auto_closes_prompt_bearing_diff_when_cycle_never_started() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let current = original.replace(
            "<!-- /agent:exchange -->",
            "❯ Why was startup missed?\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &current).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "### Re: startup miss — gpt-5\n\nRecovered through Stop.\n"
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Why was startup missed?"));
        assert!(content.contains("Recovered through Stop."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_direct_chat_manual_queue_response_after_recursive_guard() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(
            &dir,
            &["I'm getting lint rejected for too long. Is 2300 words too long? Why"],
        );
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "The lint failure is counting characters, not words."
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("counting characters"),
            "Stop must persist a replayable same-thread answer before allowing exit"
        );
        assert!(
            agent_doc_repair_io::pending::load_active_pending_response(&doc)
                .unwrap()
                .is_none(),
            "a successful binary-owned closeout must leave no pending capture"
        );
        assert!(
            agent_doc_flow_io::closeout::cycle_already_committed(&doc).is_some(),
            "allow-exit requires committed closeout proof"
        );
    }

    #[test]
    fn stop_auto_closes_consecutive_direct_chat_manual_queue_answers() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &["Remove the max character count cap"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let first = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "First direct-chat answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();
        assert_eq!(first, StopResponse::AllowExit);

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "Remove the max character count cap".to_string(),
        })
        .unwrap();

        let second = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Second direct-chat answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(second, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("First direct-chat answer."), "{content}");
        assert!(content.contains("Second direct-chat answer."), "{content}");
        assert!(
            agent_doc_repair_io::pending::load_active_pending_response(&doc)
                .unwrap()
                .is_none(),
            "both replayable answers must cross the binary-owned boundary"
        );
    }

    #[test]
    fn stop_blocks_when_parent_submodule_pointer_closeout_fails() {
        let parent_dir = tempfile::tempdir().unwrap();
        let sub_src_dir = tempfile::tempdir().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();
        fs::create_dir_all(parent.join(".agent-doc")).unwrap();

        git(&sub_src, &["init"]);
        fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init", "--no-verify"]);

        git(&parent, &["init"]);
        fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init", "--no-verify"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );
        git(&parent, &["commit", "-m", "add submodule", "--no-verify"]);

        let submodule_root = parent.join("src/submodule");
        fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(submodule_root.join(".agent-doc/state/cycles")).unwrap();
        let doc = submodule_root.join("session.md");
        let original = concat!(
            "---\n",
            "agent_doc_session: sid\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Are the false positives fixed now?\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git(&submodule_root, &["add", "session.md"]);
        git(&submodule_root, &["commit", "-m", "add doc", "--no-verify"]);
        git(&parent, &["add", "src/submodule"]);
        git(
            &parent,
            &["commit", "-m", "record doc commit", "--no-verify"],
        );

        let parent_git_dir = ProcessCommand::new("git")
            .current_dir(&parent)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(parent_git_dir.status.success());
        let parent_git_dir = PathBuf::from(String::from_utf8_lossy(&parent_git_dir.stdout).trim());
        fs::write(parent_git_dir.join("index.lock"), "held by test").unwrap();

        track_doc(&parent_dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: parent.display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: false-positive status — gpt-5\n\n",
                "Yes, the direct-chat answer was written through the Stop hook.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let blocked_after_submodule_commit = match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(
                    reason.contains("could not finish the required commit boundary"),
                    "block reason should name closeout failure, got: {reason}"
                );
                let names_parent_pointer = reason
                    .contains("parent submodule pointer is not committed")
                    && reason.contains("agent-doc commit");
                let names_open_cycle = reason.contains("finalize left cycle")
                    && reason.contains("agent-doc session-check");
                assert!(
                    names_parent_pointer || names_open_cycle,
                    "block reason should name the missing parent layer or the earlier open-cycle closeout boundary, got: {reason}"
                );
                names_parent_pointer
            }
            other => panic!("expected recoverable block response, got {other:?}"),
        };

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("direct-chat answer was written through the Stop hook"));
        if blocked_after_submodule_commit {
            assert!(
                agent_doc_git_io::submodule::submodule_pointer_drift(&doc)
                    .unwrap()
                    .is_some(),
                "parent gitlink should remain stale while index.lock is held"
            );
        }
        let root = project_root_for(&doc).unwrap();
        assert!(
            load_state(&root, "codex-session").unwrap().is_some(),
            "hook state must remain so a retry can finish closeout"
        );
    }

    #[test]
    fn stop_auto_closes_visible_template_response_without_last_assistant_message() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "### Re: #8zjh — gpt-5\n\n",
            "Recovered from visible response.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, current).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Recovered from visible response."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_patch_payload_with_safe_leading_commentary() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are some #next-steps?\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "Reviewing the current plan and repo conventions so I can turn `#next-steps` into concrete backlog items in the session document.\n",
            "I have the plan context. Next I’m checking how this repo formats backlog items so the patch matches existing session-doc conventions instead of inventing a new shape.\n\n",
            "<!-- patch:exchange -->\n",
            "### Re: #next-steps — gpt-5\n\n",
            "What changed: added prioritized follow-up items.\n\n",
            "Verification: backlog patch replayed through Stop hook closeout.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#bpcontract] Write the contract first.\n",
            "<!-- /patch:backlog -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(
            content.contains("[#bpcontract] Write the contract first."),
            "structured backlog patch was lost:\n{content}"
        );
        assert!(
            !content.contains("Reviewing the current plan and repo conventions"),
            "leading commentary should be stripped from the replayed closeout"
        );
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_guard_prefixed_patch_payload() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "<!-- no-pending-capture -->\n",
            "<!-- patch:exchange -->\n",
            "### Re: Please reply — gpt-5\n\n",
            "Hook closeout body.\n",
            "<!-- /patch:exchange -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: Please reply — gpt-5"));
        assert!(content.contains("Hook closeout body."));
        assert!(
            !dir.path()
                .join(".agent-doc/codex-hooks/blocked-stop")
                .exists(),
            "guard-prefixed patch payload should not be captured as blocked"
        );
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_partial_backlog_patch_against_structured_backlog() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are some #next-steps?\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "### 1. Existing\n",
            "- [ ] [#base] Keep the existing top item.\n",
            "\n",
            "### 2. Later\n",
            "- [ ] [#later] Keep the later section item.\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #next-steps — gpt-5\n\n",
            "What changed: added prioritized follow-up items.\n\n",
            "Verification: backlog patch replayed through Stop hook closeout.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "### 1. Existing\n",
            "- [ ] [#base] Keep the existing top item.\n",
            "- [ ] [#bpcontract] Write the contract first.\n",
            "<!-- /patch:backlog -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(
            content.contains("[#bpcontract] Write the contract first."),
            "structured backlog patch was lost:\n{content}"
        );
        assert!(content.contains("### 2. Later"));
        let capture = agent_doc_capture_io::latest_committed(&doc)
            .unwrap()
            .expect("committed capture should exist");
        assert!(
            !capture.response_body.contains("<!-- patch:backlog -->"),
            "captured response should be stripped of backlog patches after normalization"
        );
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    /// A Stop hook can commit the captured response before the agent supplies a
    /// late tracked-work outcome. `write --commit --backlog-only --done` uses
    /// best-effort commit mode, but it still crosses a commit boundary and must
    /// publish the done archive and row removal as one projection. Leaving an
    /// intermediate `[x]` row makes the command fail its own
    /// `guard_completed_pending_reap` and incorrectly requires another
    /// preflight/repair cycle.
    #[test]
    fn hook_committed_response_accepts_late_backlog_only_done_in_one_closeout() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Explain the outside-activity clause.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#late-done] Confirm the outside-activity interpretation.\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: outside-activity clause — gpt-5\n\n",
                "The clause reaches paid and unpaid outside activity.\n",
                "<!-- /patch:exchange -->\n"
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();
        assert_eq!(response, StopResponse::AllowExit);
        agent_doc_capture_io::latest_committed(&doc)
            .unwrap()
            .expect("Stop hook should commit the captured response");

        let mut options = agent_doc_write_command_io::CommandOptions::repair_replay(
            &doc,
            false,
            false,
            false,
            &[],
        );
        options.pending_only = true;
        options.pending_done = vec!["late-done".to_string()];
        agent_doc_write_runtime_io::run_command_with_response(
            options,
            agent_doc_write_command_io::CommitMode::BestEffort,
            String::new(),
        )
        .expect("late backlog-only --done should reap and commit without repair");

        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("- [ ] [#late-done]") && !content.contains("- [x] [#late-done]"),
            "late done must not leave an open or intermediate completed row:\n{content}"
        );
        assert!(
            content.contains("<!-- agent:done -->") && content.contains("[#late-done]"),
            "late done should archive the completed row in the same projection:\n{content}"
        );
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("late done should need no preflight/repair cycle, got {other:?}"),
        }
    }

    /// `#stopterminalreapfirst`: the Stop payload still holds the predecessor
    /// turn's closing text when terminal reap residue is the first guard. That
    /// text may name the next queue head while explicitly saying it is pending;
    /// it must not be captured as the head's response or consume the head.
    #[test]
    fn stop_reaps_committed_residue_before_handing_off_adjacent_queue_head() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(
            &dir,
            &[
                "do https://haiven-inc.atlassian.net/browse/HAVN-1489",
                "do https://haiven-inc.atlassian.net/browse/HAVN-1490",
            ],
        );
        let with_completed = fs::read_to_string(&doc).unwrap()
            + concat!(
                "\n## Pending / Not Built\n\n",
                "<!-- agent:backlog -->\n",
                "- [x] [#sdkjira1361] Complete predecessor SDK work.\n",
                "<!-- /agent:backlog -->\n",
            );
        fs::write(&doc, &with_completed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &with_completed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let terminal_cycle = agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(&with_completed),
            Some(&with_completed),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&with_completed),
            Some(&with_completed),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => assert!(
                agent_doc_session_check_io::is_completed_pending_reap_interruption(&message),
                "{message}"
            ),
            other => panic!("expected terminal reap interruption, got {other:?}"),
        }

        let stale_predecessor_text = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: SDK queue progress — gpt-5\n\n",
            "HAVN-1489 and HAVN-1490 remain queued for their owning turns.\n",
            "<!-- /patch:exchange -->\n",
        );
        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: stale_predecessor_text.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("HAVN-1489") && reason.contains("in-pane")),
            "the untouched next head must be handed to its owner: {response:?}"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("HAVN-1489 and HAVN-1490 remain queued"),
            "predecessor text must not be replayed:\n{content}"
        );
        assert!(
            content.contains("- do https://haiven-inc.atlassian.net/browse/HAVN-1489"),
            "the adjacent head must remain active:\n{content}"
        );
        assert!(
            !content.contains("- ~~do https://haiven-inc.atlassian.net/browse/HAVN-1489~~"),
            "the adjacent head must not be consumed:\n{content}"
        );
        assert!(
            !content.contains("- [x] [#sdkjira1361]"),
            "terminal maintenance must reap the completed row:\n{content}"
        );
        let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            cycle.cycle_id, terminal_cycle.cycle_id,
            "no response cycle may reopen"
        );
        assert_eq!(cycle.phase.as_str(), "committed");
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_terminal_reap_maintenance_settled"),
            "{ops}"
        );
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
        assert!(!ops.contains("codex_stop_capture_saved"), "{ops}");
    }

    #[test]
    fn stop_auto_closes_active_session_post_commit_drift() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let drifted = format!("{original}\nPost-closeout active-session drift.\n");
        fs::write(&doc, &drifted).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let _session = codex_session_bound_to_this_thread();

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active harness session changed this document"));
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message:
                "### Re: post-closeout drift — gpt-5\n\nRecovered post-closeout drift.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Post-closeout active-session drift."));
        assert!(content.contains("Recovered post-closeout drift."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    /// `#donearchive-stop-race`: the editor may publish `agent:done` content
    /// into its configured archive while Stop is checking a just-committed
    /// turn. That projection is not fresh prompt debt and must not cause Stop
    /// to mint a response cycle for its console restatement.
    #[test]
    fn stop_does_not_reopen_for_done_archive_projection_after_commit() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: completed work — gpt-5\n\n",
            "The completed work is persisted.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Done\n\n",
            "<!-- agent:done archive=\"tasks/task.done.md\" -->\n",
            "- 2026-10-08 [#completed] Completed item.\n",
            "  Archived detail that is not shaped like a tracked-item row.\n",
            "<!-- /agent:done -->\n",
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let archive = dir.path().join("tasks/task.done.md");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        let archive_before = "# Agent Doc Completed Work\n\n";
        fs::write(&archive, archive_before).unwrap();
        git(dir.path(), &["add", "tasks/task.done.md"]);
        git(
            dir.path(),
            &["commit", "-m", "seed done archive", "--no-verify"],
        );
        let previous =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original))
                .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(original),
            Some(original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(original),
            Some(original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(original),
            Some(original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let archived = original.replace(
            "- 2026-10-08 [#completed] Completed item.\n  Archived detail that is not shaped like a tracked-item row.\n",
            "",
        );
        let removed_body = concat!(
            "- 2026-10-08 [#completed] Completed item.\n",
            "  Archived detail that is not shaped like a tracked-item row.\n",
        );
        fs::write(&archive, format!("{archive_before}{removed_body}")).unwrap();
        fs::write(&doc, &archived).unwrap();

        // `#stopreopenflake`: a real Codex Stop hook runs with the tracked
        // session bound (`CODEX_THREAD_ID`). This test used to read that
        // process-global ambiently, so it passed alone (no session) and failed
        // whenever a parallel test had bound one. Bind it explicitly: the
        // proven archive drain must stay neutral to the active-session
        // post-commit drift guard as well as to the prompt-bearing diff.
        let _session = codex_session_bound_to_this_thread();
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(_) => {}
            other => panic!("a proven done-archive drain is not post-commit drift: {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Console restatement of the persisted response.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            cycle.cycle_id, previous.cycle_id,
            "Stop minted a fresh cycle"
        );
        assert_eq!(cycle.phase.as_str(), "committed");
        assert!(
            agent_doc_capture_io::load_active(&doc).unwrap().is_none(),
            "archive projection must not retain the console restatement"
        );
        assert_eq!(
            fs::read_to_string(&archive).unwrap(),
            format!("{archive_before}{removed_body}"),
            "the test must retain the exact archive append that proves the source drain"
        );
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("Console restatement"),
            "Stop replayed console text over an archive-only projection"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    #[test]
    fn stop_reopens_for_operator_edit_inside_external_done_component() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: completed work — gpt-5\n\n",
            "The completed work is persisted.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Done\n\n",
            "<!-- agent:done archive=\"tasks/task.done.md\" -->\n",
            "Original operator completion note.\n",
            "<!-- /agent:done -->\n",
        );
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let archive = dir.path().join("tasks/task.done.md");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(&archive, "# Agent Doc Completed Work\n\n").unwrap();
        git(dir.path(), &["add", "tasks/task.done.md"]);
        git(
            dir.path(),
            &["commit", "-m", "seed done archive", "--no-verify"],
        );
        let previous =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original))
                .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(original),
            Some(original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let revised = original.replace(
            "Original operator completion note.",
            "Operator revised the completion note.",
        );
        fs::write(&doc, &revised).unwrap();

        let _session = codex_session_bound_to_this_thread();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message:
                "### Re: completion correction — gpt-5\n\nPreserved the operator correction."
                    .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Operator revised the completion note."));
        assert!(content.contains("Preserved the operator correction."));
        let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_ne!(
            cycle.cycle_id, previous.cycle_id,
            "operator edit was hidden"
        );
        assert_eq!(cycle.phase.as_str(), "committed");
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    #[test]
    fn stop_auto_closes_open_cycle_across_nested_roots_and_turn_drift() {
        let dir = setup_project();
        let doc = write_nested_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!(
                "agent-doc nested/{}",
                doc.file_name().unwrap().to_string_lossy()
            ),
        })
        .unwrap();

        let nested_root = project_root_for(doc.parent().unwrap()).unwrap();
        assert!(
            load_state(&nested_root, "codex-session").unwrap().is_some(),
            "expected state to be mirrored into nested project root"
        );

        let response =
            apply_stop(&StopInput {
                session_id: "codex-session".to_string(),
                turn_id: "turn-2".to_string(),
                cwd: doc.parent().unwrap().display().to_string(),
                last_assistant_message:
                    "### Re: nested root drift — gpt-5\n\nRecovered from nested root drift."
                        .to_string(),
                stop_hook_active: false,
            })
            .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Recovered from nested root drift."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }

        let outer_root = project_root_for(dir.path()).unwrap();
        for root in [&outer_root, &nested_root] {
            let state = load_state(root, "codex-session").unwrap().unwrap();
            assert!(state.last_turn_id.is_empty());
            assert!(state.last_prompt.is_empty());
        }
    }

    #[test]
    fn stop_auto_closes_active_session_drift_when_prompt_has_instruction_preamble() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        fs::write(
            &doc,
            format!("{original}\nVisible drift after committed closeout.\n"),
        )
        .unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!(
                "# AGENTS.md instructions for {}\n\n```\nagent-doc <FILE>\n```\n\nagent-doc {}\n",
                dir.path().display(),
                doc.display()
            ),
        })
        .unwrap();

        let _session = codex_session_bound_to_this_thread();

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active harness session changed this document"));
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message:
                "### Re: preamble prompt tracking — gpt-5\n\nRecovered after preamble prompt tracking."
                    .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Visible drift after committed closeout."));
        assert!(content.contains("Recovered after preamble prompt tracking."));
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_blocks_open_cycle_without_recoverable_response() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("unfinished document cycle"));
                assert!(reason.contains("agent-doc repair"));
                assert!(reason.contains("tool-only or authentication step"));
                assert!(reason.contains("blocked-stop"));
            }
            other => panic!("expected block response, got {other:?}"),
        }

        let blocked_dir = dir.path().join(".agent-doc/codex-hooks/blocked-stop");
        let captures: Vec<_> = fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked-stop capture");
        let blocked_payload = fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("\"kind\": \"missing_last_assistant_message\""));
        assert!(blocked_payload.contains(&format!("agent-doc {}", doc.display())));
    }

    #[test]
    fn stop_blocks_transcript_shaped_last_assistant_message() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let transcript_dump = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: hook proof — gpt-5\n",
            "Hook closeout body.\n",
            "<!-- /agent:exchange -->\n",
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: transcript_dump.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("unfinished document cycle"));
                assert!(reason.contains("refused to replay"));
                assert!(reason.contains("blocked-stop"));
            }
            other => panic!("expected block response, got {other:?}"),
        }

        assert!(
            agent_doc_capture_io::load_active(&doc).unwrap().is_none(),
            "transcript-shaped payload should not be stored as replayable pending content"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content, original, "document should remain unchanged");

        let blocked_dir = dir.path().join(".agent-doc/codex-hooks/blocked-stop");
        let captures: Vec<_> = fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked-stop capture");
        let blocked_payload = fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("agent:exchange"));
        assert!(blocked_payload.contains("component dump"));
    }

    #[test]
    fn stop_passes_through_committed_cycle() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
    }

    #[test]
    fn codex_stop_rechecks_status_sync_once_at_the_final_boundary() {
        let dir = setup_project();
        let doc = write_status_sync_doc(&dir);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-status-sync");
        complete_run(&doc);

        let mut input = StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-status-sync".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        };
        let response = apply_stop(&input).unwrap();
        match response {
            StopResponse::ContinueTurn { class, reason } => {
                assert_eq!(class, StopContinuationClass::StatusSync);
                assert!(reason.contains(FINAL_STATUS_SYNC_MARKER), "{reason}");
                assert!(
                    reason.contains("Keep the published release current"),
                    "{reason}"
                );
                assert!(reason.contains("agent-doc status set"), "{reason}");
                assert!(reason.contains("agent-doc commit"), "{reason}");
                assert!(
                    reason.contains("If the answer did not change, make no write"),
                    "{reason}"
                );
            }
            other => panic!("expected final status-sync continuation, got {other:?}"),
        }

        input.stop_hook_active = true;
        assert_eq!(apply_stop(&input).unwrap(), StopResponse::AllowExit);
    }

    #[test]
    fn stop_passes_through_committed_cycle_with_stopped_queue_head() {
        // A held queue is `queue: pause` (`#queuestopremove`). A leftover `go`
        // marker beside it is an explicit control that wins on every activation
        // reader (`#qbindingone`, GH #79), so it is not a held queue.
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = "---\nsession: sid\nqueue: pause\n---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: #advance-review — gpt-5\n\n\
Reviewed the gated items.\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue priority -->\n\
- #advance-review\n\
<!-- /agent:queue -->\n";
        fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(original),
            Some(original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-stopped-queue");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-stopped-queue".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Processed #advance-review.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
    }

    #[test]
    fn stop_passes_through_committed_cycle_with_gated_review_head() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &["do [#liveverify]"]);
        let original = format!(
            "{}\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\
             <!-- agent:review -->\n\
             - [/] [#liveverify] [operator-verify] Verify unsaved editor switching.\n\
             <!-- /agent:review -->\n",
            fs::read_to_string(&doc).unwrap()
        );
        fs::write(&doc, &original).unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-gated-review");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-gated-review".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Implementation is installed; live verification remains gated."
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
    }

    #[test]
    fn pending_queue_writeback_skips_gated_heads_but_preserves_real_work() {
        let document = |queue: &str| {
            format!(
                "<!-- agent:queue -->\n{queue}\n<!-- /agent:queue -->\n\
                 <!-- agent:backlog -->\n- [ ] [#ready] Implement ready work.\n\
                 <!-- /agent:backlog -->\n\
                 <!-- agent:review -->\n\
                 - [/] [#verify] [operator-verify] Observe a real editor edit.\n\
                 <!-- /agent:review -->\n"
            )
        };
        for (queue, expected) in [
            ("- do [#verify]", None),
            ("- do [#verify]\n- do [#ready]", Some("do [#ready]")),
            ("- Fix the remaining issue", Some("Fix the remaining issue")),
            (
                "- complete [#verify]: verified. looks good",
                Some("complete [#verify]: verified. looks good"),
            ),
            ("--- stop\n- do [#ready]", None),
        ] {
            assert_eq!(
                first_active_queue_prompt_in_content(
                    &document(queue),
                    &agent_doc_queue::queue_claim::ClaimedQueueItems::none(),
                    &Default::default(),
                )
                .as_deref(),
                expected,
                "queue: {queue}"
            );
        }
    }

    /// Drive the `#fixrunturn` shape up to the turn's Stop: an admitted Codex
    /// turn whose own closeout is in flight (`write_applied`) receives the exact
    /// trigger again as Codex steering (Run Agent Doc typed into the busy pane),
    /// then that closeout commits. `pre_fix_refusal` replays what the hook did
    /// before the fix — refresh the binding and stamp a refusal receipt — instead
    /// of the coalesced repeat, which leaves the binding untouched.
    fn stop_after_same_turn_repeat_during_closeout(pre_fix_refusal: bool) -> StopResponse {
        let dir = setup_project();
        let doc = write_auto_queue_doc(
            &dir,
            &["What is the merge order for my accepted PRs on haiven-infra?"],
        );
        init_git_repo(dir.path(), &doc);
        let content = fs::read_to_string(&doc).unwrap();
        let trigger = UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-infra".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        };
        apply_user_prompt_submit(&trigger).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_codex_hook_io::record_preflight_admission(&trigger, true).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&content),
            Some(&content),
            "sha-infra-response",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&content),
            Some(&content),
        )
        .unwrap();

        // The steered repeat lands while the turn's own closeout is in flight.
        let repeat = agent_doc_codex_hook_io::same_turn_admitted_invocation(&trigger)
            .unwrap()
            .expect("an exact same-turn repeat during closeout must keep its admission");
        assert!(matches!(
            repeat.kind,
            agent_doc_codex_hook_io::SameTurnRepeatKind::CloseoutInFlight { .. }
        ));
        if pre_fix_refusal {
            apply_user_prompt_submit(&trigger).unwrap();
            agent_doc_codex_hook_io::record_preflight_admission(&trigger, false).unwrap();
        }

        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&content),
            Some(&content),
        )
        .unwrap();

        apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-infra".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "PR #253 is green; recorded the review gate.".to_string(),
            stop_hook_active: false,
        })
        .unwrap()
    }

    /// `#fixrunturn` (infra.md, 2026-10-12): the operator's queued prompt must
    /// be re-dispatched by the Stop hook after an overlapping Run Agent Doc
    /// arrives mid-finalize — not dropped behind a refusal receipt.
    #[test]
    fn same_turn_repeat_during_closeout_keeps_stop_queue_continuation() {
        match stop_after_same_turn_repeat_during_closeout(false) {
            StopResponse::ContinueTurn { class, reason } => {
                assert_eq!(class, StopContinuationClass::Queue);
                assert!(
                    reason.contains("What is the merge order for my accepted PRs"),
                    "the pending operator prompt must be re-dispatched: {reason}"
                );
            }
            other => panic!("the coalesced repeat must keep the queue continuation: {other:?}"),
        }
    }

    /// The pre-fix shape, kept as the guard's witness: a refusal receipt for the
    /// steered repeat makes Stop retire the binding and let the turn end with
    /// the queued prompt unanswered. If this ever starts continuing, the
    /// refusal fence changed and the regression above no longer proves the fix.
    #[test]
    fn refusal_receipt_for_same_turn_repeat_strands_the_queue_prompt() {
        assert_eq!(
            stop_after_same_turn_repeat_during_closeout(true),
            StopResponse::AllowExit
        );
    }

    #[test]
    fn stop_blocks_clean_closeout_when_auto_queue_has_next_prompt() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { class, reason } => {
                assert_eq!(class, StopContinuationClass::Queue);
                assert!(reason.contains("agent:queue auto"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
                assert!(reason.contains("send the final answer"), "{reason}");
                // #codex-self-reinvoke-prevent (Option B): the continuation must
                // drive an in-pane answer + `finalize`, NOT instruct a recursive
                // `agent-doc <FILE>` re-run from the owner pane.
                assert!(reason.contains("in-pane"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(
                    reason.contains("Do NOT run `agent-doc"),
                    "continuation must warn against the recursive self-invocation: {reason}"
                );
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    /// `#codexsubagentstop`: a collaboration worker shares the coordinator's
    /// Codex session id but has its own turn id. Its Stop must not inherit the
    /// coordinator's ready queue head and report `Blocked by hook` as the
    /// worker result.
    #[test]
    fn foreign_subagent_turn_does_not_inherit_parent_queue_continuation() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["release + publish"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "coordinator-turn");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "subagent-turn".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "branch, commit, and tests".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_turn_id, "coordinator-turn");
        assert_eq!(state.last_auto_queue_head, None);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- release + publish"));
    }

    /// `#queuegodefaultdrain`: a queue with no control on either surface (the
    /// shape every drained queue has after `#queuestopretire`) is in its
    /// default `go`, so the Stop hook keeps it moving like an explicit `go`.
    #[test]
    fn stop_blocks_clean_closeout_when_control_less_queue_has_next_prompt() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &["Remove the max character count cap"]);
        let content = fs::read_to_string(&doc)
            .unwrap()
            .replacen("queue: start\n", "", 1);
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("max character count cap"), "{reason}");
                assert!(reason.contains("send the final answer"), "{reason}");
            }
            other => panic!("expected control-less queue continuation block, got {other:?}"),
        }
    }

    #[test]
    fn claude_stop_blocks_clean_closeout_and_names_loop_reentry() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");

        let response = apply_claude_stop(&ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        })
        .unwrap()
        .expect("clean Claude closeout must keep draining");

        assert!(response.reason.contains("fix the next queue item"));
        assert!(response.reason.contains("`loop` skill"));
        assert!(response.reason.contains("Do not shell `agent-doc"));
        assert!(
            response.to_hook_output()["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .is_some()
        );
    }

    #[test]
    fn claude_stop_rechecks_status_sync_once_at_the_final_boundary() {
        let dir = setup_project();
        let doc = write_status_sync_doc(&dir);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        complete_run(&doc);

        let mut input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };
        let response = apply_claude_stop(&input)
            .unwrap()
            .expect("first final boundary must re-evaluate status sync");
        assert!(
            response.reason.contains(FINAL_STATUS_SYNC_MARKER),
            "{}",
            response.reason
        );
        assert!(
            response
                .reason
                .contains("Keep the published release current"),
            "{}",
            response.reason
        );

        input.stop_hook_active = true;
        assert!(
            apply_claude_stop(&input).unwrap().is_none(),
            "recursive Stop must pass through even when the result stayed unchanged"
        );
    }

    /// `#queueclaim`: a head dispatched to a subagent is claimed. When every
    /// remaining head is claimed, the Stop hook must let the turn end quietly
    /// (no continuation) instead of re-entering for work already in flight;
    /// releasing the claim restores the continuation.
    #[test]
    fn claude_stop_does_not_reenter_for_claimed_heads() {
        // Both claims stay unacknowledged on purpose: "every head claimed" is a
        // quiet stop only inside the dispatch grace. Pin the claim clock so the
        // outcome does not depend on run speed (`#finalgrace`).
        let _claim_clock = agent_doc_queue_io::queue_claim::pin_claim_clock_for_current_thread(
            agent_doc_queue_io::queue_claim::claim_clock_now_secs(),
        );
        let dir = setup_project();
        let doc = write_auto_queue_doc(
            &dir,
            &[
                "#gh-fix https://github.com/o/r/issues/109",
                "#gh-fix https://github.com/o/r/issues/110",
            ],
        );
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        complete_run(&doc);
        let input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };
        let no_supervisor =
            |_: &Path| agent_doc_controller::status::SupervisorDrainReadiness::NoLiveSupervisor;

        // One head claimed: the continuation names the OTHER head.
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            "#gh-fix https://github.com/o/r/issues/109",
            "subagent:gh109",
            3600,
        )
        .unwrap();
        let next = apply_claude_stop_with_drain_readiness(&input, no_supervisor)
            .unwrap()
            .expect("the unclaimed head still continues");
        assert!(next.reason.contains("issues/110"), "{}", next.reason);
        assert!(!next.reason.contains("issues/109"), "{}", next.reason);

        // Every head claimed: quiet stop, no continuation, no block.
        complete_run(&doc);
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            "#gh-fix https://github.com/o/r/issues/110",
            "subagent:gh110",
            3600,
        )
        .unwrap();
        assert!(
            apply_claude_stop_with_drain_readiness(&input, no_supervisor)
                .unwrap()
                .is_none(),
            "all heads claimed: the turn must end quietly"
        );
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(ops.contains("state=waiting_on_claims"), "{ops}");

        // Releasing a claim restores drainability.
        complete_run(&doc);
        agent_doc_queue_io::queue_claim::release(&doc, "#gh-fix https://github.com/o/r/issues/109")
            .unwrap();
        let resumed = apply_claude_stop_with_drain_readiness(&input, no_supervisor)
            .unwrap()
            .expect("a released head is drainable again");
        assert!(resumed.reason.contains("issues/109"), "{}", resumed.reason);
    }

    #[test]
    fn claude_stop_blocks_an_overdue_claim_without_worker_acknowledgement() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["#subagents do [#gh301]"]);
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "#gh301", "subagent:gh301", 600).unwrap();

        let blocked = claimed_heads_stop_continuation_at(&doc, claim.claimed_at_secs + 10, 10)
            .unwrap()
            .expect("an overdue unstarted claim must block Stop");
        assert!(
            blocked.reason.contains("coordinator obligation"),
            "{}",
            blocked.reason
        );
        assert!(blocked.reason.contains("queue start"), "{}", blocked.reason);
        assert!(
            blocked.reason.contains("--worker-id <worker-id>"),
            "{}",
            blocked.reason,
        );
        assert!(
            blocked.reason.contains("queue release"),
            "{}",
            blocked.reason
        );
        assert!(blocked.reason.contains("--requeue"), "{}", blocked.reason);

        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "#gh301",
            "subagent:gh301",
            "worker:gh301",
            600,
            claim.generation,
        )
        .unwrap();
        assert!(
            claimed_heads_stop_continuation_at(&doc, claim.claimed_at_secs + 100, 10)
                .unwrap()
                .is_none(),
            "a worker-bound claim remains a quiet in-flight wait"
        );
    }

    /// `#stopneedsclosedcycle`: the marker is document-level and outlives the
    /// cycle that wrote it, so a clean closeout's marker used to satisfy the
    /// "completed cycle" gate for a LATER turn whose write was still retained.
    /// The hook then forbade the final answer and named a head the failed turn
    /// had already answered and reaped in authority — the disk mirror the head
    /// is read from is precisely what a retained write has not updated. Seen on
    /// tasks/agent-doc/agent-doc-bugs.md and tasks/software/lazily.md the same
    /// day. A failed closeout heads SKILL.md's exhaustive skip list, so an open
    /// cycle must beat the marker.
    #[test]
    fn claude_stop_does_not_loop_a_cycle_whose_write_is_still_retained() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        // An earlier clean closeout leaves the marker behind.
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        assert!(
            apply_claude_stop(&ClaudeStopInput {
                session_id: "codex-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: false,
                transcript_path: None,
            })
            .unwrap()
            .is_some(),
            "a closed cycle with a marker must still drive the loop"
        );

        // This turn captures a response and never reaches a write/commit —
        // exactly the `session-check INTERRUPTED ... response_captured` state.
        let original = std::fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();

        assert!(
            apply_claude_stop(&ClaudeStopInput {
                session_id: "codex-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: false,
                transcript_path: None,
            })
            .unwrap()
            .is_none(),
            "a retained write is a failed closeout: the agent must be free to report it"
        );
    }

    /// Complete one run against `doc`, producing a fresh committed cycle id.
    fn complete_run(doc: &std::path::Path) -> String {
        let content = std::fs::read_to_string(doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(doc, Some(&content), Some(&content)).unwrap();
        agent_doc_capture_io::capture_response_with_current_content(
            doc,
            "fixture response",
            &content,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            doc,
            "commit_success",
            Some(&content),
            Some(&content),
        )
        .unwrap()
        .cycle_id
    }

    /// A committed Claude run whose document then gained an operator exchange
    /// prompt (`#steerinterruptexit`). No queue, so only steering can hold the stop.
    fn committed_run_with_post_commit_steering() -> (tempfile::TempDir, PathBuf) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nagent_doc_session: sid\nagent_doc_format: template\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n### Re: done — opus\n\nCompleted.\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        complete_run(&doc);
        let committed = fs::read_to_string(&doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        fs::write(
            &doc,
            committed.replace(
                "Completed.\n",
                "Completed.\n\n❯ Also check the CI run for the release.\n",
            ),
        )
        .unwrap();
        (dir, doc)
    }

    /// `#steerinterruptexit`: once `session-check` reports post-commit steering
    /// as exit-0 `steering pending`, the closeout no longer "fails" into an
    /// implicit stop, so the Claude Stop gate holds the final answer once per
    /// run for an exchange prompt the operator added, names it verbatim, and
    /// names the one re-entry that admits a cycle. Bounded like the queue
    /// continuation: never inside a recursive stop, never twice per run.
    #[test]
    fn claude_stop_holds_the_final_answer_once_for_post_commit_steering() {
        use agent_doc_controller::status::SupervisorDrainReadiness;
        let (dir, _doc) = committed_run_with_post_commit_steering();
        let input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };
        let recursive = ClaudeStopInput {
            stop_hook_active: true,
            ..input.clone()
        };
        assert!(
            apply_claude_stop_with_drain_readiness(&recursive, |_| {
                SupervisorDrainReadiness::NoLiveSupervisor
            })
            .unwrap()
            .is_none(),
            "a recursive stop must never re-block"
        );

        let response = apply_claude_stop_with_drain_readiness(&input, |_| {
            SupervisorDrainReadiness::NoLiveSupervisor
        })
        .unwrap()
        .expect("pending exchange steering must not be dropped at the stop");
        assert!(
            response
                .reason
                .contains("Also check the CI run for the release."),
            "{}",
            response.reason
        );
        assert!(
            response.reason.contains("not a failed closeout"),
            "{}",
            response.reason
        );
        assert!(
            response.reason.contains("/loop agent-doc"),
            "{}",
            response.reason
        );

        assert!(
            apply_claude_stop_with_drain_readiness(&input, |_| {
                SupervisorDrainReadiness::NoLiveSupervisor
            })
            .unwrap()
            .is_none(),
            "one continuation request per run"
        );
    }

    /// `#steerinterruptexit` + `#steeringwake`: a ready supervisor whose idle
    /// steering wake will submit the trigger owns the re-entry, so the hook
    /// does not block for it.
    #[test]
    fn claude_stop_leaves_post_commit_steering_to_a_ready_supervisor_wake() {
        use agent_doc_controller::status::SupervisorDrainReadiness;
        let (dir, _doc) = committed_run_with_post_commit_steering();
        let input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };
        assert!(
            apply_claude_stop_with_drain_readiness(&input, |_| SupervisorDrainReadiness::Ready {
                supervisor_pid: 7
            })
            .unwrap()
            .is_none(),
            "the supervisor's steering wake owns the re-entry"
        );
    }

    /// `#stopblocksupervisorowned`: with a live, fresh supervisor the idle-queue
    /// watch continues the queue, so the hook must not raise a block (Claude
    /// Code shows every block as "Stop hook error"). Without one it still blocks.
    #[test]
    fn claude_stop_leaves_continuation_to_a_ready_supervisor() {
        use agent_doc_controller::status::SupervisorDrainReadiness;
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        complete_run(&doc);
        let input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };

        assert!(
            apply_claude_stop_with_drain_readiness(&input, |_| SupervisorDrainReadiness::Ready {
                supervisor_pid: 7
            })
            .unwrap()
            .is_none(),
            "a ready supervisor owns the continuation"
        );
        assert!(
            apply_claude_stop_with_drain_readiness(&input, |_| {
                SupervisorDrainReadiness::NoLiveSupervisor
            })
            .unwrap()
            .is_some(),
            "with no live supervisor the hook must still block"
        );
    }

    /// `#stopnoresponserun`: the stale-lock repair closes an abandoned
    /// `PreflightStarted` cycle with no captured response, and the next trigger
    /// is refused admission. That closed run drained nothing, so the hook must let
    /// the agent report the refusal instead of ordering a `/loop` that hits it again.
    #[test]
    fn claude_stop_does_not_loop_a_closed_run_that_captured_no_response() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(&content),
            Some(&content),
        )
        .unwrap();

        let input = ClaudeStopInput {
            session_id: "codex-session".to_string(),
            cwd: dir.path().display().to_string(),
            stop_hook_active: false,
            transcript_path: None,
        };
        assert!(
            apply_claude_stop(&input).unwrap().is_none(),
            "a run that answered nothing must not be continued"
        );

        // The same closed state with a captured response is a real drain step.
        complete_run(&doc);
        assert!(apply_claude_stop(&input).unwrap().is_some());
    }

    /// `#stopneedsclosedcycle`, second half: one continuation request per run.
    ///
    /// `stop_hook_active` bounds recursion within one stop, not across turns.
    /// The cross-turn bound used to live on `ContinuationMarker`, a record queue
    /// reconciliation creates and deletes, so it was absent whenever the marker
    /// was and disarmed whenever a reconcile cleared it — 24 blocks against 2
    /// skips on tasks/agent-doc/agent-doc-bugs.md 2026-09-28.
    ///
    /// The third step is the one that changed meaning. The old test asserted
    /// "an advanced head is progress, not churn", and the production log
    /// disproved it: at 00:30:47 the head moved 62 -> 26 bytes while `turn`
    /// stayed on `cycle-1790552355729`. A head moves when the operator edits the
    /// document or a reconcile rewrites the queue. Only a completed run is
    /// progress, so only a completed run re-earns a request.
    #[test]
    fn claude_stop_makes_at_most_one_continuation_request_per_run() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");

        let stop = |dir: &tempfile::TempDir| {
            apply_claude_stop(&ClaudeStopInput {
                session_id: "codex-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: false,
                transcript_path: None,
            })
            .unwrap()
        };

        assert!(
            stop(&dir).is_some(),
            "the first request for a run must still drive the loop"
        );

        // Same run, unchanged head: the drain did not strike it.
        assert!(
            stop(&dir).is_none(),
            "re-asking within one run is churn; the agent must be able to report it"
        );

        // Same run, head MOVED. Not drain progress — no run has completed since
        // the request, so the run being asked has already ended either way.
        let advanced = write_auto_queue_doc(&dir, &["fix the following queue item"]);
        assert_eq!(advanced, doc, "the fixture must rewrite the same document");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        assert!(
            stop(&dir).is_none(),
            "a head that moves without a run is an edit, not a drain"
        );

        // A completed run re-earns the request: this is ordinary loop progress,
        // and the guard must not have latched continuation off.
        complete_run(&doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        assert!(
            stop(&dir).is_some(),
            "a completed run with a new head is progress, not churn"
        );

        // `#qchurn`: the run completed and the head survived it. Still a repeat.
        complete_run(&doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");
        assert!(
            stop(&dir).is_none(),
            "a run that completes without striking the head must not be re-asked"
        );
    }

    /// Every path that can refuse a final answer must be bounded, including the
    /// fail-closed one. Hook errors are overwhelmingly persistent — an
    /// unreadable document, a refused authority resolve, a state ledger that
    /// will not open — so a branch that blocks without consulting
    /// `stop_hook_active` re-runs the same failing check forever. This was the
    /// one path through the hook that never consulted it.
    ///
    /// Driven through the public entry point rather than `apply_claude_stop`,
    /// because the branch under test lives in `handle_claude_stop`'s error arm
    /// and a test that called the inner function would prove nothing about it.
    #[test]
    fn the_fail_closed_branch_is_bounded_by_stop_hook_active() {
        // Valid JSON, wrong shape: `ClaudeStopInput` will not parse, so the
        // hook errors — but `stop_hook_active` is still recoverable, which is
        // exactly what the bound needs.
        let first = claude_stop_response(Some(r#"{"stop_hook_active": false}"#)).unwrap();
        assert_eq!(
            first["decision"], "block",
            "the first stop must fail closed so the operator hears about it"
        );
        let repeat = claude_stop_response(Some(r#"{"stop_hook_active": true}"#)).unwrap();
        assert_eq!(
            repeat,
            serde_json::json!({}),
            "re-blocking re-runs the same failing check against the same inputs"
        );
    }

    /// A payload that is not JSON at all names no session and no document, so a
    /// refusal would be issued on no evidence — and could not be bounded by a
    /// `stop_hook_active` it cannot read.
    #[test]
    fn a_payload_that_is_not_json_does_not_refuse_the_final_answer() {
        assert_eq!(
            claude_stop_response(Some("not json")).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            claude_stop_response(Some("")).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(claude_stop_response(None).unwrap(), serde_json::json!({}));
    }

    /// The DISARM half: queue reconciliation clears the marker between two
    /// stops, and the bound must survive it.
    ///
    /// Scoped deliberately. The other half — the hook reaching its block with no
    /// marker at all, because the drain-stall projection proved the continuation
    /// on its own, where the old arming write was a documented no-op — cannot be
    /// reached from here: with no marker and no stall projection the hook never
    /// blocks, so the fixture cannot put itself in that state. It is pinned
    /// instead by `queue_continuation::tests::the_continuation_request_arms_with_no_marker_present`,
    /// which is the test that reddens when arming is made marker-dependent
    /// again. Naming that claim here would have been a false green: restoring the
    /// shipped semantics leaves this test passing.
    #[test]
    fn claude_stop_bound_survives_a_reconcile_clearing_the_marker() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");

        let stop = || {
            apply_claude_stop(&ClaudeStopInput {
                session_id: "codex-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: false,
                transcript_path: None,
            })
            .unwrap()
        };

        assert!(stop().is_some(), "the first request must drive the loop");

        // Queue reconciliation drops the marker between the two stops. Under the
        // old storage this deleted the bound along with it.
        agent_doc_queue_io::continuation_marker::clear_continuation_marker(&doc).unwrap();
        assert!(
            agent_doc_queue_io::continuation_marker::load_continuation_marker(&doc)
                .unwrap()
                .is_none()
        );

        assert!(
            stop().is_none(),
            "clearing the marker must not disarm the recursion bound"
        );
    }

    #[test]
    fn claude_stop_is_exact_session_scoped_and_recursion_bounded() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["fix the next queue item"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "session-check")
            .expect("continuation required");

        assert!(
            apply_claude_stop(&ClaudeStopInput {
                session_id: "another-claude-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: false,
                transcript_path: None,
            })
            .unwrap()
            .is_none(),
            "an unrelated Claude session must not inherit this document"
        );
        assert!(
            apply_claude_stop(&ClaudeStopInput {
                session_id: "codex-session".to_string(),
                cwd: dir.path().display().to_string(),
                stop_hook_active: true,
                transcript_path: None,
            })
            .unwrap()
            .is_none(),
            "the hook must not recursively block its own second Stop"
        );
    }

    #[test]
    fn stop_passes_through_clean_closeout_when_auto_queue_has_clear_command() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/clear", "do #fix1"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);

        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_passes_through_raw_clear_queue_body_with_whitespace() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let content = concat!(
            "---\n",
            "session: sid\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue auto -->\n",
            "\n   /clear   \n\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);

        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_blocks_clean_closeout_when_auto_queue_has_generic_slash_command() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/model sonnet", "do #fix1"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("queued slash command"), "{reason}");
                assert!(reason.contains("\"/model sonnet\""), "{reason}");
                assert!(!reason.contains("Run `/clear`"), "{reason}");
            }
            other => panic!("expected auto-queue command continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_open_cycle_then_blocks_for_next_auto_queue_head() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #fix1 — gpt-5\n\n",
                "Done.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #fix1 — gpt-5"));
        assert!(content.contains("- ~~do #fix1~~"));
        assert!(content.contains("- do #fix2"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_auto_queue_continuation_prefers_configured_mcp_tools() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #fix1 — gpt-5\n\n",
                "Done.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(
                    reason.contains("configured `agent-doc` MCP server"),
                    "{reason}"
                );
                assert!(reason.contains("agent_doc_admit"), "{reason}");
                assert!(reason.contains("agent_doc_plan"), "{reason}");
                assert!(reason.contains("agent_doc_finalize"), "{reason}");
                assert!(reason.contains("agent_doc_session_check"), "{reason}");
                assert!(
                    reason.contains("agent-doc finalize")
                        && reason.contains("MCP tools are unavailable"),
                    "{reason}"
                );
                assert!(reason.contains("send the final answer"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation")
                && ops_log.contains("source=tracked_state")
                && ops_log.contains("mcp_configured=true")
                && ops_log.contains(&agent_doc_hash::content_hash("do #fix2")),
            "Stop hook should log tracked queue-continuation proof:\n{ops_log}"
        );
        assert!(
            ops_log.contains("queue_consume_proof_recorded")
                && ops_log.contains("stage=BeforeMutation")
                && ops_log.contains("stage=AfterMutation"),
            "Stop hook closeout should record queue-consumption proofs:\n{ops_log}"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("- do #fix1"), "{content}");
        assert!(content.contains("- do #fix2"), "{content}");
    }

    #[test]
    fn unbound_codex_thread_ignores_another_threads_durable_agent_doc_marker() {
        // Two Codex threads share one project. Thread A explicitly entered
        // agent-doc and left a durable auto-queue marker. Ambient Stop hooks for
        // pure thread B must remain a no-op instead of inheriting A's document.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-a");
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "pure-codex-thread-b".to_string(),
            turn_id: "turn-b".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("codex_stop_queue_continuation"),
            "pure thread B must not run thread A's agent-doc queue:\n{ops_log}"
        );
    }

    #[test]
    fn stop_passes_through_context_clear_from_durable_marker_when_session_state_missing() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/clear"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
    }

    /// `#codexstopterse`: Codex prints the Stop continuation `reason` verbatim in
    /// the operator's pane. A suppressed background context reset is an ops.log
    /// diagnostic only: it must not leak into that payload, swap the instruction
    /// path, or grow the fixed text beyond the operator-pane budget.
    fn assert_operator_pane_continuation_is_terse(reason: &str, doc: &Path) {
        for leaked in [
            "context reset",
            "automatic context clearing",
            "session accretion",
            "exchange was compacted",
            "transcript context",
            "Run `/clear`",
        ] {
            assert!(
                !reason.contains(leaked),
                "operator-pane continuation leaked {leaked:?}: {reason}"
            );
        }
        let fixed = reason
            .replace(&doc.display().to_string(), "")
            .replace("do [#seopdp] deploy product page", "");
        let budget =
            agent_doc_turn::codex_stop_continuation::PROMPT_CONTINUATION_INSTRUCTION_BUDGET + 120;
        assert!(
            fixed.chars().count() <= budget,
            "continuation is a wall of text ({} fixed chars > {budget}): {reason}",
            fixed.chars().count()
        );
    }

    #[test]
    fn stop_tracked_session_suppresses_background_clear_after_exchange_compaction() {
        let dir = setup_project();
        // The legacy `agent_doc_queue_context_reset` opt-in never clears. With
        // no ready supervisor to take the queue auto-clear hand-off (none in
        // this fixture, `#bgclearpolicy`), the Stop hook keeps the continuation
        // in-pane.
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        track_doc(&dir, &doc, "turn-x");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert_operator_pane_continuation_is_terse(&reason, &doc);
            }
            other => panic!("expected in-pane continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_background_context_clear_suppressed")
                && ops_log.contains("source=tracked_state")
                && ops_log.contains("result=in_pane_continuation"),
            "fresh-context continuation should be kept in-pane, not handed to supervisor:\n{ops_log}"
        );
        assert!(
            ops_log.contains("exchange was compacted after the last tracked context clear"),
            "suppression proof should retain the reset reason:\n{ops_log}"
        );
    }

    /// Live repro (lazily.md, 2026-10-12): an MCP-configured Codex session at
    /// accretion `block` got the CLI-only instruction plus a sentence saying a
    /// background reset "was requested ... but automatic context clearing is
    /// disabled". The reset reason must not swap the MCP instruction path or
    /// reach the operator pane; it stays an ops.log diagnostic.
    #[test]
    fn stop_tracked_reset_reason_keeps_mcp_guidance_and_terse_payload() {
        let dir = setup_project();
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        track_doc(&dir, &doc, "turn-x");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let display = doc.display().to_string();
        match response {
            StopResponse::ContinueTurn { class, reason } => {
                assert_eq!(class, StopContinuationClass::Queue);
                assert!(
                    reason.ends_with(&render_prompt_continuation_instruction(&display, true)),
                    "reset reason must not change the MCP instruction path: {reason}"
                );
                assert_operator_pane_continuation_is_terse(&reason, &doc);
            }
            other => panic!("expected tracked continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_background_context_clear_suppressed")
                && ops_log.contains("result=in_pane_continuation")
                && ops_log.contains("exchange was compacted after the last tracked context clear"),
            "the suppressed reset must stay observable in ops.log:\n{ops_log}"
        );
    }

    #[test]
    fn stop_tracked_state_suppresses_background_clear_continuation() {
        let dir = setup_project();
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert_operator_pane_continuation_is_terse(&reason, &doc);
            }
            other => panic!("expected in-pane continuation block, got {other:?}"),
        }
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(
            state.last_auto_queue_head.as_deref(),
            Some("do [#seopdp] deploy product page")
        );
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation")
                && ops_log.contains("source=tracked_state")
                && ops_log.contains("codex_background_context_clear_suppressed")
                && ops_log.contains("source=tracked_state")
                && ops_log.contains("result=in_pane_continuation"),
            "tracked fresh-context continuation should be logged and kept in-pane:\n{ops_log}"
        );
    }

    /// `#bgclearpolicy`: with queue auto-clear on (the default) and a ready
    /// supervisor, the Codex Stop hook allows the stop so the supervisor can
    /// restart Codex fresh and dispatch the next head, instead of continuing
    /// the queue in the already-loaded context.
    #[test]
    fn stop_auto_clear_yields_queue_boundary_to_ready_supervisor() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        let response = codex_stop_auto_clear_yield_with(
            &doc,
            "do [#seopdp] deploy product page",
            "tracked_state",
            |_| agent_doc_controller::status::SupervisorDrainReadiness::Ready { supervisor_pid: 7 },
        );
        assert_eq!(response, Some(StopResponse::AllowExit));
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation_yield")
                && ops_log.contains("result=supervisor_auto_clear")
                && ops_log.contains("supervisor_drain_readiness=ready"),
            "{ops_log}"
        );
    }

    /// `#bgclearpolicy`: `disable-auto-clear` keeps the in-pane continuation and
    /// never probes the supervisor.
    #[test]
    fn stop_disable_auto_clear_keeps_in_pane_continuation() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        let content = fs::read_to_string(&doc).unwrap().replace(
            "<!-- agent:queue auto go -->",
            "<!-- agent:queue auto go disable-auto-clear -->",
        );
        fs::write(&doc, &content).unwrap();
        let response = codex_stop_auto_clear_yield_with(
            &doc,
            "do [#seopdp] deploy product page",
            "tracked_state",
            |_| panic!("disable-auto-clear must not probe the supervisor"),
        );
        assert_eq!(response, None);
    }

    /// `#bgclearpolicy`: a yield nobody receives would stall the queue, so a
    /// missing supervisor keeps the in-pane continuation and says why.
    #[test]
    fn stop_auto_clear_without_ready_supervisor_keeps_in_pane_continuation() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        let response = codex_stop_auto_clear_yield_with(
            &doc,
            "do [#seopdp] deploy product page",
            "tracked_state",
            |_| agent_doc_controller::status::SupervisorDrainReadiness::NoLiveSupervisor,
        );
        assert_eq!(response, None);
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_auto_clear_kept_in_pane")
                && ops_log.contains("supervisor_drain_readiness=no_live_supervisor"),
            "{ops_log}"
        );
    }

    /// `#clearcodex`: the Codex Stop-hook continuation now emits structured
    /// proof lines to ops.log when opted in, so an operator can verify the
    /// queue-turn clear decision instead of guessing. The canonical
    /// `[s760] clear-decision` line plus a `[clearcodex] codex-continuation`
    /// companion (with the accretion/compaction reason and the
    /// `clear_instructed=false` outcome) must both be present.
    #[test]
    fn stop_codex_continuation_logs_structured_clear_proof_when_opted_in() {
        let dir = setup_project();
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        track_doc(&dir, &doc, "turn-x");

        apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .expect("ops.log should exist after an opted-in continuation");
        assert!(
            ops_log.contains("[s760] clear-decision optIn=true"),
            "missing canonical s760 marker:\n{ops_log}"
        );
        assert!(
            ops_log.contains("pct=none clear=false"),
            "without a readable Codex token_count transcript, the s760 gate must fail safe:\n{ops_log}"
        );
        assert!(
            ops_log.contains("[clearcodex] codex-continuation optIn=true"),
            "missing codex-continuation companion marker:\n{ops_log}"
        );
        assert!(
            ops_log.contains("clear_instructed=false")
                && ops_log.contains("background_clear_suppressed=true"),
            "compaction-after-clear should suppress automatic /clear:\n{ops_log}"
        );
    }

    #[test]
    fn stop_codex_continuation_suppresses_clear_when_token_count_crosses_threshold() {
        let dir = setup_project();
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\nagent_doc_clear_threshold = 15\n",
        )
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard =
            agent_doc_codex_hook_io::override_codex_home_for_current_thread(home.path());
        let sessions = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("15");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout-current.jsonl"),
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":20000,\"cached_input_tokens\":20000,\"output_tokens\":0}},\"model_context_window\":100000}}}}}}\n",
                dir.path().display()
            ),
        )
        .unwrap();

        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        track_doc(&dir, &doc, "turn-x");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert_operator_pane_continuation_is_terse(&reason, &doc);
            }
            other => panic!("expected in-pane continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .expect("ops.log should exist after threshold clear");
        assert!(
            ops_log.contains("[s760] clear-decision optIn=true threshold=15 pct=40.0 clear=true"),
            "threshold clear decision should use Codex token_count:\n{ops_log}"
        );
        assert!(
            ops_log.contains("transcript context 40.0% >= clear threshold 15%")
                && ops_log.contains("codex_background_context_clear_suppressed")
                && ops_log.contains("result=in_pane_continuation"),
            "threshold crossing should be logged but kept in-pane:\n{ops_log}"
        );
    }

    /// `#clearcodex`: without the `agent_doc_queue_context_reset` opt-in the
    /// Codex continuation must stay silent — no pre-emptive `/clear` and no
    /// structured clear-decision noise in ops.log.
    #[test]
    fn stop_codex_continuation_emits_no_clear_proof_when_not_opted_in() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        track_doc(&dir, &doc, "turn-x");

        apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("[s760] clear-decision"),
            "no s760 clear-decision should be logged when not opted in:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[clearcodex] codex-continuation"),
            "no codex-continuation marker should be logged when not opted in:\n{ops_log}"
        );
    }

    #[test]
    fn stop_auto_queue_allows_in_pane_after_tracked_clear_following_compaction() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        let compaction_ts =
            agent_doc_session_accretion_io::recent_exchange_compaction_timestamp(&doc)
                .unwrap()
                .expect("compaction marker should be visible");
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                identity_origin: Default::default(),
                harness: None,
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(compaction_ts),
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: compaction_ts,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(!reason.contains("Run `/clear`"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
            }
            other => panic!("expected normal in-pane auto-queue continuation, got {other:?}"),
        }
    }

    #[test]
    fn stop_tracked_continuation_prefers_configured_mcp_tools() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_continuation::reconcile_marker(&doc, "commit")
            .expect("continuation required");
        track_doc(&dir, &doc, "turn-x");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(
                    reason.contains("do [#seopdp] deploy product page"),
                    "{reason}"
                );
                assert!(
                    reason.contains("configured `agent-doc` MCP server"),
                    "{reason}"
                );
                assert!(reason.contains("agent_doc_admit"), "{reason}");
                assert!(reason.contains("agent_doc_finalize"), "{reason}");
                assert!(reason.contains("agent_doc_session_check"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
            }
            other => panic!("expected tracked continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_repair_preserves_auto_queue_when_response_targets_other_prompt() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #next-steps — gpt-5\n\n",
                "Captured unrelated follow-up response.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("do #fix1"), "{reason}");
                assert!(!reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(content.contains("<!-- agent:queue auto go -->"));
        assert!(content.contains("queue_active: true"));
        assert!(content.contains("- do #fix1"));
        assert!(!content.contains("- ~~do #fix1~~"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_replays_plain_final_answer_when_auto_queue_continuation_makes_no_progress() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                identity_origin: Default::default(),
                harness: None,
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.\n\nVerification: Codex stop-hook simulation."
                .to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(
                    reason.contains("recovered the previous queue response"),
                    "{reason}"
                );
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected recovered no-progress block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: do #fix1 — gpt-5"));
        assert!(content.contains("Verification: Codex stop-hook simulation."));
        assert!(content.contains("- ~~do #fix1~~"));
        assert!(content.contains("- do #fix2"));
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_blocks_when_repeated_auto_queue_head_has_no_replayable_response() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                identity_origin: Default::default(),
                harness: None,
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("could not safely replay"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
            }
            other => panic!("expected repeated-head recovery block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("### Re: do #fix1 — gpt-5"));
        assert!(content.contains("- do #fix1"));
        assert!(!content.contains("- ~~do #fix1~~"));
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_allows_repeated_auto_queue_blocks_after_head_advances() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix2", "do #fix3"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                identity_origin: Default::default(),
                harness: None,
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::ContinueTurn { reason, .. } => {
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected continued auto-queue block, got {other:?}"),
        }
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn retained_delivery_and_write_applied_interruptions_remain_binary_owned() {
        assert!(is_binary_owned_closeout_interruption(
            "[session-check] INTERRUPTED: binary-owned response delivery `intent-1` is retained for `/repo/tasks/example.md` (reason=merge_unsaved_editor_cut_with_deferred_target); the same capture will resume automatically after editor/controller delivery converges. Same-capture recovery remains pending: retained target has not reached exact canonical/disk convergence."
        ));
        assert!(is_binary_owned_closeout_interruption(
            "[session-check] INTERRUPTED: cycle `cycle-1` is still `write_applied` — response write landed but no terminal commit followed."
        ));
        assert!(!is_binary_owned_closeout_interruption(
            "[session-check] INTERRUPTED: cycle `cycle-1` is still `preflight_started` — cycle started but no write/commit followed."
        ));
        let needs_operator = format!(
            "[session-check] INTERRUPTED: binary-owned response delivery `intent-1` is retained; {}. The endpoint rejected delivery.",
            agent_doc_session_check_io::RETAINED_PENDING_NEEDS_OPERATOR_TOKEN,
        );
        assert!(retained_closeout_needs_operator(&needs_operator));
        assert!(
            !is_binary_owned_closeout_interruption(&needs_operator),
            "an operator-gated capture must not re-enter the automatic Stop retry loop"
        );
    }

    #[test]
    fn stop_fails_closed_after_one_auto_continue() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Still open.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::RefuseExit { reason } => {
                assert!(reason.contains("already continued once"));
                assert!(reason.contains("cycle is still open"));
            }
            other => panic!("expected stop response, got {other:?}"),
        }
    }

    #[test]
    fn stop_retires_prompt_debt_when_its_observed_open_cycle_commits() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "Also fix the recurring queue item.".to_string(),
        })
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let tracked = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(
            tracked
                .last_prompt_cycle
                .as_ref()
                .map(|cycle| cycle.was_open),
            Some(true),
            "{tracked:?}"
        );
        assert!(
            committed_cycle_settled_prompt_debt(&doc, &tracked).unwrap(),
            "{tracked:?}"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Short console handoff after finalize.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert!(matches!(response, StopResponse::AllowExit), "{response:?}");
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(state.last_prompt.is_empty());
        assert!(state.last_turn_id.is_empty());
    }

    /// `#postcommitpromptownership`: tools.md, 2026-10-14. A queue edit
    /// appeared after the turn's response was already committed, while Codex's
    /// first Stop still carried that predecessor turn's nonverbatim console
    /// summary. The summary must not become the response to the new queue item.
    #[test]
    fn stop_opens_empty_successor_instead_of_capturing_predecessor_summary_for_new_queue_work() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &[]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let predecessor =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        track_doc(&dir, &doc, "turn-1");
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();

        let queue_prompt = "Can we base #22 on the";
        let with_new_queue_work = original.replace(
            "<!-- /agent:queue -->",
            &format!("- {queue_prompt}\n<!-- /agent:queue -->"),
        );
        fs::write(&doc, &with_new_queue_work).unwrap();
        invalidate_stop_document_cache();

        let (loaded_root, tracked) = load_bound_session_for_stop(dir.path(), "codex-session")
            .unwrap()
            .expect("bound session");
        assert!(
            committed_cycle_settled_prompt_debt(&doc, &tracked).unwrap(),
            "the committed predecessor must own the turn's original prompt"
        );
        let roots = tracking_roots(dir.path(), Some(&doc));
        let stale_summary = "No—the PR title does not generally trigger it.";
        let response = active_session_prompt_requires_writeback_with(
            &doc,
            &roots,
            &loaded_root,
            &tracked,
            &StopInput {
                session_id: "codex-session".to_string(),
                turn_id: "turn-1".to_string(),
                cwd: dir.path().display().to_string(),
                last_assistant_message: stale_summary.to_string(),
                stop_hook_active: false,
            },
            |_| agent_doc_controller::status::SupervisorDrainReadiness::NoLiveSupervisor,
        )
        .unwrap()
        .expect("new queue work must block the first Stop");

        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("opened successor cycle")
                    && reason.contains(queue_prompt)
                    && reason.contains("deliberately not captured")),
            "{response:?}"
        );
        let successor = agent_doc_cycle_state_io::load(&doc)
            .unwrap()
            .expect("post-commit successor cycle");
        assert_ne!(successor.cycle_id, predecessor.cycle_id);
        assert_eq!(successor.phase.as_str(), "preflight_started");
        assert_eq!(
            successor.last_event,
            agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT
        );
        assert!(successor.capture_id.is_none());
        assert!(successor.response_sha256.is_none());
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains(queue_prompt));
        assert!(!content.contains(stale_summary));
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_prompt_successor_opened")
                && ops.contains("codex_stop_post_commit_prompt_supervisor_yield_unavailable")
                && ops.contains("supervisor_drain_readiness=no_live_supervisor")
                && ops.contains("capture_skipped=true")
                && !ops.contains("codex_stop_capture_saved"),
            "{ops}"
        );
    }

    /// A ready supervisor already watches the settled steering set reactively.
    /// Yield before opening a successor so the new prompt remains visible to
    /// that wake and Codex never has to render its continuation as
    /// `Blocked by hook` (`#postcommitpromptownership`, `#steeringwake`).
    #[test]
    fn stop_yields_fresh_post_commit_prompt_to_ready_supervisor() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &[]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let predecessor =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        track_doc(&dir, &doc, "turn-1");
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();

        let fresh_prompt = "I merged https://github.com/haiven-dev/haiven-api/pull/740 into main. Will the Release Please run?";
        let with_new_work = original.replace(
            "<!-- /agent:exchange -->",
            &format!(
                "\n<!-- agent:boundary:committed -->\n❯ {fresh_prompt}\n<!-- /agent:exchange -->"
            ),
        );
        fs::write(&doc, &with_new_work).unwrap();
        invalidate_stop_document_cache();

        let (loaded_root, tracked) = load_bound_session_for_stop(dir.path(), "codex-session")
            .unwrap()
            .expect("bound session");
        let roots = tracking_roots(dir.path(), Some(&doc));
        let response = active_session_prompt_requires_writeback_with(
            &doc,
            &roots,
            &loaded_root,
            &tracked,
            &StopInput {
                session_id: "codex-session".to_string(),
                turn_id: "turn-1".to_string(),
                cwd: dir.path().display().to_string(),
                last_assistant_message: "The predecessor is complete.".to_string(),
                stop_hook_active: false,
            },
            |_| agent_doc_controller::status::SupervisorDrainReadiness::Ready {
                supervisor_pid: 42,
            },
        )
        .unwrap()
        .expect("ready supervisor must own the wake");

        assert_eq!(response, StopResponse::AllowExit);
        let still_predecessor = agent_doc_cycle_state_io::load(&doc)
            .unwrap()
            .expect("committed predecessor");
        assert_eq!(still_predecessor.cycle_id, predecessor.cycle_id);
        assert_eq!(still_predecessor.phase.as_str(), "committed");
        assert!(still_predecessor.capture_id.is_some());
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains(fresh_prompt));
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_prompt_supervisor_yield")
                && ops.contains("supervisor_pid=42")
                && ops.contains("successor_opened=false")
                && ops.contains("action=allow_exit")
                && !ops.contains("codex_stop_post_commit_prompt_successor_opened"),
            "{ops}"
        );
    }

    #[test]
    fn stop_retires_prompt_debt_when_a_later_cycle_commits() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        let predecessor =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "Also fix the later queued implementation.".to_string(),
        })
        .unwrap();

        let later =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        assert_ne!(later.cycle_id, predecessor.cycle_id);
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let mut tracked = load_state(&root, "codex-session").unwrap().unwrap();
        let committed = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            tracked
                .last_prompt_cycle
                .as_ref()
                .map(|cycle| cycle.cycle_id.as_str()),
            Some(predecessor.cycle_id.as_str()),
            "{tracked:?}"
        );
        assert_eq!(committed.cycle_id, later.cycle_id);
        // Keep the ordering deterministic even when the test's transitions all
        // occur inside one wall-clock second.
        tracked.updated_at = committed.started_at.saturating_sub(1);
        assert!(
            committed_cycle_settled_prompt_debt(&doc, &tracked).unwrap(),
            "{tracked:?} {committed:?}"
        );

        save_state(&root, &tracked).unwrap();
        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Short console handoff after finalize.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();
        assert!(matches!(response, StopResponse::AllowExit), "{response:?}");
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(state.last_prompt.is_empty());
        assert!(state.last_turn_id.is_empty());
    }

    /// `#fixlazilycodex`: a committed status line, a verdict per payload
    /// placement. HEAD already holds it ⇒ idempotent no-op (no fresh cycle);
    /// visible only in the uncommitted current authority ⇒ still fails closed;
    /// absent everywhere ⇒ a fresh cycle is minted for capture.
    #[test]
    fn post_commit_reopen_is_a_noop_only_for_a_payload_already_in_head() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        let status =
            "Cycle `cycle-1` converged and committed successfully. No queue work is drainable.";
        let committed = fs::read_to_string(&doc).unwrap().replace(
            "❯ Hello\n<!-- /agent:exchange -->",
            &format!("❯ Hello\n\n### Re: recovered response\n\n{status}\n<!-- /agent:exchange -->"),
        );
        fs::write(&doc, &committed).unwrap();
        init_git_repo(dir.path(), &doc);
        let previous =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&committed), Some(&committed))
                .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(&committed),
            Some(&committed),
        )
        .unwrap();

        let in_head = agent_doc_template::replay_guard::classify_replay_payload(status);
        assert_eq!(
            reopen_terminal_cycle_before_stop_capture(&doc, &in_head).unwrap(),
            StopCaptureReopen::PayloadAlreadyCommitted
        );
        let still = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            still.cycle_id, previous.cycle_id,
            "no fresh cycle for a committed payload"
        );
        assert_eq!(still.phase.as_str(), "committed");
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_payload_already_committed"),
            "{ops}"
        );
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );

        // Negative control: visible in current authority but NOT committed.
        let uncommitted = "An uncommitted closing line the editor already shows.";
        fs::write(
            &doc,
            committed.replace(
                "<!-- /agent:exchange -->",
                &format!("\n{uncommitted}\n<!-- /agent:exchange -->"),
            ),
        )
        .unwrap();
        let in_current = agent_doc_template::replay_guard::classify_replay_payload(uncommitted);
        let err = reopen_terminal_cycle_before_stop_capture(&doc, &in_current).unwrap_err();
        assert!(
            format!("{err:#}").contains("already materialized in current document authority"),
            "{err:#}"
        );
        fs::write(&doc, &committed).unwrap();
        invalidate_stop_document_cache();

        // Negative control: a genuinely unrecorded payload is still captured.
        let fresh = agent_doc_template::replay_guard::classify_replay_payload(
            "### Re: Hello — gpt-5\n\nA genuinely new answer.\n",
        );
        assert_eq!(
            reopen_terminal_cycle_before_stop_capture(&doc, &fresh).unwrap(),
            StopCaptureReopen::Reopened
        );
        assert_ne!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            previous.cycle_id
        );
    }

    fn write_claimed_subagent_queue_doc(dir: &tempfile::TempDir, status: &str) -> PathBuf {
        let doc = dir.path().join("task.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: recovered response\n\n\
{status}\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue -->\n\
- ~~do [#done]~~\n\
- do [#held]\n\
<!-- /agent:queue -->\n\n\
## Backlog\n\n\
<!-- agent:backlog -->\n\
- [ ] [#held] Pin the released gitlinks after the other sessions settle.\n\
<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    /// `#fixlazilycodex` (tasks/software/lazily.md, cycle-1791827706347): the
    /// only queue head was claimed by a subagent worker, the cycle had
    /// converged and committed, and Codex's closing status was already in HEAD.
    /// The Stop hook read the claimed head as session prompt debt, reopened the
    /// committed cycle, and failed closed ("post-commit Stop payload is already
    /// materialized in HEAD"); the three Stops before it had each captured a
    /// fresh status line as a `### Re: recovered response`. Now the claimed head
    /// is not prompt debt and the Stop is allowed with no capture.
    #[test]
    fn committed_stop_with_claimed_head_and_payload_in_head_allows_without_capture() {
        let dir = setup_project();
        let status =
            "Cycle `cycle-1` converged and committed successfully. No queue work is drainable.";
        let doc = write_claimed_subagent_queue_doc(&dir, status);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_claim::claim(&doc, "do [#held]", "subagent:held", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "do [#held]",
            "subagent:held",
            "worker:held",
            3600,
            1,
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");
        let committed_cycle = complete_run(&doc);
        let before = fs::read_to_string(&doc).unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: status.to_string(),
            stop_hook_active: false,
        })
        .expect("a Stop after a committed closeout must never fail closed");

        assert_eq!(response, StopResponse::AllowExit);
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            before,
            "no recovered response"
        );
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            committed_cycle,
            "the committed cycle must not be reopened"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    /// A worker can report after the coordinator's predecessor cycle commits
    /// (and even after that coordinator session is replaced). Unlike a running
    /// attempt, this ☑️ row is coordinator writeback debt: Stop must hand it
    /// back instead of accepting the already-committed status and stranding the
    /// result in the outbox.
    #[test]
    fn committed_stop_with_reported_head_hands_back_writeback_debt() {
        let dir = setup_project();
        let status =
            "Cycle `cycle-1` converged and committed successfully. No queue work is drainable.";
        let doc = write_claimed_subagent_queue_doc(&dir, status);
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_claim::claim(&doc, "do [#held]", "subagent:held", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "do [#held]",
            "subagent:held",
            "worker:held",
            3600,
            1,
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");
        let committed_cycle = complete_run(&doc);

        agent_doc_queue_io::queue_claim::report_attempt(&doc, "do [#held]", Some(1)).unwrap();
        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: status.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("do [#held]")),
            "{response:?}\n{}",
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default()
        );
        let cycle = agent_doc_cycle_state_io::load(&doc)
            .unwrap()
            .expect("reported writeback debt retains a cycle record");
        if cycle.cycle_id != committed_cycle {
            assert_eq!(cycle.phase.as_str(), "preflight_started");
            assert_eq!(
                cycle.last_event,
                agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT,
            );
            assert!(cycle.capture_id.is_none());
            assert!(cycle.response_sha256.is_none());
        }
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let claims = agent_doc_queue_io::queue_claim::claim_states_for_content(
            &doc,
            &fs::read_to_string(&doc).unwrap(),
        )
        .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported,
            "writeback debt remains fenced from redispatch"
        );
    }

    /// `#fixlazilycodex`: the same committed closeout with an unresolved
    /// exchange prompt is real session work. The committed status is not its
    /// answer, so the hook neither reopens/captures nor fails closed: it hands
    /// the prompt back in-pane.
    #[test]
    fn committed_stop_with_open_prompt_and_payload_in_head_hands_back_without_capture() {
        let dir = setup_project();
        let status =
            "Cycle `cycle-1` converged and committed successfully. No queue work is drainable.";
        let doc = write_claimed_subagent_queue_doc(
            &dir,
            &format!("{status}\n\n❯ Explain the release pin order"),
        );
        init_git_repo(dir.path(), &doc);
        agent_doc_queue_io::queue_claim::claim(&doc, "do [#held]", "subagent:held", 3600).unwrap();
        track_doc(&dir, &doc, "turn-1");
        let committed_cycle = complete_run(&doc);
        let before = fs::read_to_string(&doc).unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: status.to_string(),
            stop_hook_active: false,
        })
        .expect("a committed payload is never a fail-closed error");

        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("Explain the release pin order")),
            "{response:?}\n{}",
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default()
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), before);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            committed_cycle
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_payload_already_committed"),
            "{ops}"
        );
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    #[test]
    fn post_commit_reopen_records_current_authority_not_head_as_file_baseline() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let head = fs::read_to_string(&doc).unwrap();
        let previous =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&head), Some(&head)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&head),
            Some(&head),
        )
        .unwrap();

        let current = head.replace("❯ Hello", "❯ Prompt from current authority");
        fs::write(&doc, &current).unwrap();
        let payload = agent_doc_template::replay_guard::classify_replay_payload(
            "### Re: Prompt from current authority — gpt-5\n\nDone.\n",
        );

        assert_eq!(
            reopen_terminal_cycle_before_stop_capture(&doc, &payload).unwrap(),
            StopCaptureReopen::Reopened
        );

        let reopened = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_ne!(reopened.cycle_id, previous.cycle_id);
        assert_eq!(
            reopened.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&head).as_str())
        );
        assert_eq!(
            reopened.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&current).as_str())
        );
        assert_ne!(reopened.snapshot_hash, reopened.file_hash);
        assert_eq!(
            reopened.queue_selection_authority,
            agent_doc_cycle_state_io::QueueSelectionAuthority::OutOfBandCloseout,
            "Stop auto-reopen must not inherit unrelated visible queue markers",
        );
        assert!(reopened.selected_free_text_queue_heads.is_empty());
    }

    #[test]
    fn stop_does_not_replay_a_restatement_over_the_turns_committed_cycle() {
        // `#fpestopreplay`: the turn's own cycle committed its answer, then a
        // fresh prompt diff appeared (a recurring queue head), so session-check
        // is interrupted. Codex's closing chat message restates the committed
        // answer; it must not reopen a cycle and land in the document.
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        let seeded = fs::read_to_string(&doc).unwrap().replace(
            "❯ Hello\n<!-- /agent:exchange -->",
            "❯ Hello\n\n### Re: Hello — gpt-5\n\nInitial response.\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &seeded).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &seeded,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let turn_cycle =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        track_doc(&dir, &doc, "turn-1");
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "Answer the recurring review head.".to_string(),
        })
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let root = project_root_for(dir.path()).unwrap();
        let tracked = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(
            committed_cycle_settled_prompt_debt(&doc, &tracked).unwrap(),
            "{tracked:?}"
        );
        fs::write(
            &doc,
            original.replace(
                "<!-- /agent:exchange -->",
                "\n❯ Review the recurring head again\n<!-- /agent:exchange -->",
            ),
        )
        .unwrap();
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            // `#steerinterruptexit`: `steering pending` (no Codex thread in the
            // environment) or the active-session drift `INTERRUPTED` (a Codex
            // thread bound); the Stop hook takes the same path for both.
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message)
            | agent_doc_session_check_io::SessionCheckStatus::SteeringPending(message) => {
                assert!(is_committed_prompt_diff_interruption(&message), "{message}");
            }
            other => panic!("expected committed prompt-diff session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Closing console status restating the committed answer."
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("fresh unresolved exchange work")),
            "the fresh diff must be handed back in-pane: {response:?}"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("Closing console status"),
            "the restatement must not be written into the document:\n{content}"
        );
        let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            cycle.cycle_id, turn_cycle.cycle_id,
            "no cycle may be reopened"
        );
        assert_eq!(cycle.phase.as_str(), "committed");
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_replay_skipped"),
            "{ops}"
        );
        assert!(
            !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    /// `#steeringwake`: the committed-prompt-diff branch is selected when
    /// session-check returns `SteeringPending`, so it must yield to the same
    /// ready supervisor as the clean-session post-commit prompt path instead of
    /// surfacing Codex's `Blocked by hook` continuation.
    #[test]
    fn stop_yields_committed_prompt_diff_to_ready_supervisor() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        let seeded = fs::read_to_string(&doc).unwrap().replace(
            "❯ Hello\n<!-- /agent:exchange -->",
            "❯ Hello\n\n### Re: Hello — gpt-5\n\nInitial response.\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &seeded).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &seeded,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let predecessor =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        track_doc(&dir, &doc, "turn-1");
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "Answer the recurring review head.".to_string(),
        })
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(&original),
            Some(&original),
            "response-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let fresh_prompt = "release + publish latest updates";
        fs::write(
            &doc,
            original.replace(
                "<!-- /agent:exchange -->",
                &format!("\n❯ {fresh_prompt}\n<!-- /agent:exchange -->"),
            ),
        )
        .unwrap();
        invalidate_stop_document_cache();

        let reason = match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message)
            | agent_doc_session_check_io::SessionCheckStatus::SteeringPending(message) => message,
            other => panic!("expected committed prompt-diff session-check status, got {other:?}"),
        };
        assert!(is_committed_prompt_diff_interruption(&reason), "{reason}");

        let response = committed_prompt_diff_stop_response_with(&doc, &reason, false, |_| {
            agent_doc_controller::status::SupervisorDrainReadiness::Ready { supervisor_pid: 42 }
        })
        .unwrap()
        .expect("committed prompt diff response");

        assert_eq!(response, StopResponse::AllowExit);
        let still_predecessor = agent_doc_cycle_state_io::load(&doc)
            .unwrap()
            .expect("committed predecessor");
        assert_eq!(still_predecessor.cycle_id, predecessor.cycle_id);
        assert_eq!(still_predecessor.phase.as_str(), "committed");
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains(fresh_prompt));
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_prompt_supervisor_yield")
                && ops.contains("supervisor_pid=42")
                && ops.contains("source=committed_prompt_diff")
                && !ops.contains("codex_stop_post_commit_prompt_cycle_reopened"),
            "{ops}"
        );
    }

    #[test]
    fn stop_reopens_and_closes_prompt_debt_observed_after_cycle_committed() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let previous =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original))
                .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-2");
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: "A genuinely new prompt after closeout.".to_string(),
        })
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Unpersisted answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert!(matches!(response, StopResponse::AllowExit), "{response:?}");
        assert!(
            fs::read_to_string(&doc)
                .unwrap()
                .contains("Unpersisted answer."),
            "post-commit response was not written"
        );
        let closed = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(closed.phase.as_str(), "committed");
        assert_ne!(closed.cycle_id, previous.cycle_id);
        let capture = agent_doc_capture_io::load_by_id(&doc, &closed.cycle_id)
            .unwrap()
            .expect("fresh cycle capture");
        assert_eq!(capture.cycle_id, closed.cycle_id);
        assert!(capture.response_body.contains("Unpersisted answer."));
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_auto_close_success source=strict_replay_receipt"),
            "Stop must consume strict repair's successful closeout receipt"
        );
        assert_eq!(
            ops.matches("terminal_closeout_proof_recorded ").count(),
            1,
            "the post-commit follow-up must run only one full closeout"
        );
        let current = fs::read_to_string(&doc).unwrap();
        assert!(agent_doc_flow_io::closeout::replay_closeout_still_proven(&doc, &current).unwrap());
        assert!(
            !agent_doc_flow_io::closeout::replay_closeout_still_proven(
                &doc,
                &format!("{current}\nA new operator edit.\n"),
            )
            .unwrap(),
            "an intervening visible edit must invalidate receipt reuse"
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &format!("{current}\nChanged baseline.\n"),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert!(
            !agent_doc_flow_io::closeout::replay_closeout_still_proven(&doc, &current).unwrap(),
            "changed snapshot evidence must invalidate receipt reuse"
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &current,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert!(
            agent_doc_capture_io::load_by_id(&doc, &previous.cycle_id)
                .unwrap()
                .is_none(),
            "the new response must not be captured under the terminal predecessor"
        );
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        assert!(
            !agent_doc_flow_io::closeout::replay_closeout_still_proven(&doc, &current).unwrap(),
            "a new cycle must supersede the old receipt even with unchanged content"
        );
    }

    fn transcript_record(kind: &str, content: serde_json::Value) -> String {
        serde_json::json!({"type": kind, "message": {"role": kind, "content": content}}).to_string()
    }

    fn schedule_wakeup(prompt: &str) -> String {
        transcript_record(
            "assistant",
            serde_json::json!([{"type": "tool_use", "name": "ScheduleWakeup",
                "input": {"delaySeconds": 60, "prompt": prompt}}]),
        )
    }

    /// `#stoploopalreadyarmed`: this session, 2026-09-29. Every drained item
    /// ended with a "Stop hook blocking error" asking for a `/loop` re-entry
    /// the turn had already scheduled with `ScheduleWakeup`.
    #[test]
    fn a_scheduled_loop_reentry_arms_the_continuation() {
        let file = Path::new("/work/tasks/agent-doc/agent-doc-bugs.md");
        let loop_prompt = "/loop agent-doc /work/tasks/agent-doc/agent-doc-bugs.md";
        let turn_start = transcript_record(
            "user",
            serde_json::json!("<command-name>/loop</command-name>"),
        );
        let feedback = transcript_record(
            "user",
            serde_json::json!(
                "Stop hook feedback:\nagent-doc Stop hook kept the active queue moving"
            ),
        );
        let tool_result = transcript_record(
            "user",
            serde_json::json!([{"type": "tool_result", "content": "Next wakeup scheduled"}]),
        );

        let armed = [
            turn_start.clone(),
            schedule_wakeup(loop_prompt),
            tool_result.clone(),
            feedback.clone(),
        ]
        .join("\n");
        assert!(transcript_tail_arms_loop_reentry(&armed, file));

        // Stop-hook feedback before the wake-up is still inside the same turn.
        let after_feedback = [
            turn_start.clone(),
            feedback,
            schedule_wakeup(loop_prompt),
            tool_result.clone(),
        ]
        .join("\n");
        assert!(transcript_tail_arms_loop_reentry(&after_feedback, file));

        // A new operator prompt after the wake-up starts a new turn.
        let operator = transcript_record(
            "user",
            serde_json::json!("Also fix the Stop hook error in this session"),
        );
        let superseded = [schedule_wakeup(loop_prompt), tool_result.clone(), operator].join("\n");
        assert!(!transcript_tail_arms_loop_reentry(&superseded, file));

        // `#stoplooparmednotify`: a background-task notification wakes the
        // session without being an operator prompt, so a wake-up armed in the
        // previous turn still covers this one.
        let notification = transcript_record(
            "user",
            serde_json::json!(
                "<task-notification>\n<task-id>a1</task-id>\n<status>completed</status>\n</task-notification>"
            ),
        );
        let across_notification = [
            schedule_wakeup(loop_prompt),
            tool_result.clone(),
            notification,
        ]
        .join("\n");
        assert!(transcript_tail_arms_loop_reentry(
            &across_notification,
            file
        ));

        // A wake-up for another document, or a non-loop prompt, arms nothing.
        let other = [
            turn_start.clone(),
            schedule_wakeup("/loop agent-doc /work/tasks/other.md"),
        ]
        .join("\n");
        assert!(!transcript_tail_arms_loop_reentry(&other, file));
        let not_loop = [turn_start, schedule_wakeup("check the deploy")].join("\n");
        assert!(!transcript_tail_arms_loop_reentry(&not_loop, file));
    }

    /// `#queuetypingsteer`: src/haiven-dev/tasks/api.md, 2026-09-29. Operator
    /// typing inside an existing queue item is reported as `content_edit`. The
    /// recursive Stop must hand it back as steering to answer in-pane, not stop
    /// with "already continued once".
    #[test]
    fn a_committed_content_edit_is_handed_back_as_steering() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        let reason = "[session-check] INTERRUPTED: cycle `cycle-1` is `committed` (commit_success), repaired committed historical committed_capture snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: content_edit: Even if formal logic code is not directly understandable, agents should explain the invariants.\nThis is realtime operator steering, not a failed closeout";

        let response = committed_prompt_diff_stop_response(&doc, reason, true)
            .unwrap()
            .expect("content_edit steering after a commit must be handed back in-pane");
        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("fresh unresolved exchange work")
                    && reason.contains("Even if formal logic code is not directly understandable")),
            "{response:?}"
        );
    }

    /// `#claimedstopforward`: agent-doc-bugs.md, 2026-10-14. The mid-turn hook
    /// had already classified a newly added queue prompt as
    /// `forward_to_owner` for its live subagent claim. After the predecessor
    /// committed, Stop re-read the same diff as fresh coordinator work and
    /// emitted "Blocked by hook". The typed claim routing remains authoritative
    /// at Stop; releasing the claim makes the exact same prompt actionable again.
    #[test]
    fn committed_claimed_prompt_diff_stops_quietly_until_claim_is_released() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &[]);
        init_git_repo(dir.path(), &doc);
        complete_run(&doc);

        let prompt = "Can we guide subagents to surface prompts and permission requests?";
        let current = fs::read_to_string(&doc).unwrap().replace(
            "<!-- /agent:queue -->",
            &format!("- {prompt}\n<!-- /agent:queue -->"),
        );
        fs::write(&doc, &current).unwrap();
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            prompt,
            "subagent:subagent_prompt_permissions",
            3600,
        )
        .unwrap();

        let pending =
            agent_doc_session_check_io::midturn_steering::pending_steering_items(&doc, &current);
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].dispatch.as_str(), "forward_to_owner");
        assert_eq!(
            pending[0].owner.as_deref(),
            Some("subagent:subagent_prompt_permissions")
        );

        let reason = format!(
            "[session-check] INTERRUPTED: cycle `cycle-1` is `committed` (commit_success), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started; prompt_target: {prompt}"
        );
        assert!(is_committed_prompt_diff_interruption(&reason), "{reason}");
        assert_eq!(
            committed_prompt_diff_stop_response(&doc, &reason, false).unwrap(),
            Some(StopResponse::AllowExit),
            "claimed worker steering must not be re-admitted as coordinator work"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_committed_prompt_diff_forwarded_to_claim_owners"),
            "{ops}"
        );

        agent_doc_queue_io::queue_claim::release(&doc, prompt).unwrap();
        let response = committed_prompt_diff_stop_response(&doc, &reason, false)
            .unwrap()
            .expect("released prompt must re-enter");
        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("fresh unresolved exchange work")
                    && reason.contains(prompt)),
            "{response:?}"
        );
    }

    /// `#reportedrelease`: src/haiven-dev/tasks/docs.md, 2026-10-15. The
    /// coordinator released `subagent:page-status-enum` when it reported and
    /// stopped with the answer only in chat. Stop bounced it to "answer" the
    /// line as fresh work, without the strike contract: the agent persisted a
    /// `### Re:` answer, the line stayed queued, and steering re-offered it to
    /// a NEW subagent. A reported release now carries a receipt: Stop names
    /// the worker's unpersisted result and the exact echo that strikes the
    /// line, and steering routes it `persist_reported`, never `subagent`.
    #[test]
    fn committed_reported_release_names_the_unpersisted_worker_result_and_echo() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &[]);
        init_git_repo(dir.path(), &doc);
        complete_run(&doc);

        let prompt = "Does the page status enum have an archived value? What are the values?";
        let current = fs::read_to_string(&doc).unwrap().replace(
            "<!-- /agent:queue -->",
            &format!("- {prompt}\n<!-- /agent:queue -->"),
        );
        fs::write(&doc, &current).unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, prompt, "subagent:page-status-enum", 3600)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            prompt,
            "subagent:page-status-enum",
            "worker:page-status-enum",
            3600,
            1,
        )
        .unwrap();
        agent_doc_queue_io::claim_release_receipt::release(
            &doc,
            prompt,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
        )
        .unwrap()
        .expect("the worker's claim is released");

        let pending =
            agent_doc_session_check_io::midturn_steering::pending_steering_items(&doc, &current);
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].dispatch.as_str(), "persist_reported");
        assert_eq!(
            pending[0].owner.as_deref(),
            Some("subagent:page-status-enum")
        );
        let rendered =
            agent_doc_session_check_io::midturn_steering::render_pending_steering_items(&pending)
                .unwrap();
        assert!(!rendered.contains("DISPATCH NOW"), "{rendered}");

        let reason = format!(
            "[session-check] INTERRUPTED: cycle `cycle-1` is `committed` (commit_success), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started; prompt_target: {prompt}"
        );
        let response = committed_prompt_diff_stop_response(&doc, &reason, false)
            .unwrap()
            .expect("an unpersisted reported result must be handed back");
        let StopResponse::ContinueTurn { reason, .. } = &response else {
            panic!("expected a block: {response:?}");
        };
        assert!(
            reason.contains("subagent:page-status-enum already reported"),
            "{reason}"
        );
        assert!(
            reason.contains(&format!("`> **Queue prompt:** {prompt}`")),
            "the bounce must carry the echo that strikes the line: {reason}"
        );
        assert!(
            reason.contains("Do NOT dispatch a new subagent"),
            "{reason}"
        );
        assert!(
            !reason.contains("fresh unresolved exchange work"),
            "{reason}"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_committed_prompt_diff_reported_worker_unpersisted"),
            "{ops}"
        );

        // A requeue (abandoned work) returns the line to ordinary fresh work,
        // still with the strike contract for a queue line.
        agent_doc_queue_io::queue_claim::claim(&doc, prompt, "subagent:retry", 3600).unwrap();
        agent_doc_queue_io::claim_release_receipt::release(
            &doc,
            prompt,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
        )
        .unwrap();
        let response = committed_prompt_diff_stop_response(&doc, &reason_for(prompt), false)
            .unwrap()
            .expect("requeued line re-enters");
        let StopResponse::ContinueTurn { reason, .. } = &response else {
            panic!("expected a block: {response:?}");
        };
        assert!(
            reason.contains("fresh unresolved exchange work"),
            "{reason}"
        );
        assert!(
            reason.contains(&format!("`> **Queue prompt:** {prompt}`")),
            "{reason}"
        );
    }

    fn reason_for(prompt: &str) -> String {
        format!(
            "[session-check] INTERRUPTED: cycle `cycle-1` is `committed` (commit_success), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started; prompt_target: {prompt}"
        )
    }

    /// A claimed item may share one post-commit diff with genuine fresh work.
    /// Only the externally owned item is excluded; Stop must still hand the
    /// unclaimed prompt back to the coordinator and name that prompt.
    #[test]
    fn committed_mixed_prompt_diff_reenters_for_unclaimed_work() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &[]);
        init_git_repo(dir.path(), &doc);
        complete_run(&doc);

        let claimed = "Worker-owned permission prompt";
        let unclaimed = "Operator follow-up still needs the coordinator";
        let current = fs::read_to_string(&doc).unwrap().replace(
            "<!-- /agent:queue -->",
            &format!("- {claimed}\n- {unclaimed}\n<!-- /agent:queue -->"),
        );
        fs::write(&doc, &current).unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, claimed, "subagent:permissions", 3600)
            .unwrap();

        let pending =
            agent_doc_session_check_io::midturn_steering::pending_steering_items(&doc, &current);
        assert_eq!(pending.len(), 2, "{pending:?}");
        assert_eq!(pending[0].dispatch.as_str(), "forward_to_owner");
        assert_ne!(pending[1].dispatch.as_str(), "forward_to_owner");

        let reason = format!(
            "[session-check] INTERRUPTED: cycle `cycle-1` is `committed` (commit_success), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started; prompt_target: {claimed}"
        );
        let response = committed_prompt_diff_stop_response(&doc, &reason, false)
            .unwrap()
            .expect("unclaimed work must re-enter");
        assert!(
            matches!(&response, StopResponse::ContinueTurn { reason, .. }
                if reason.contains(unclaimed) && !reason.contains(claimed)),
            "{response:?}"
        );
    }

    #[test]
    fn recursive_stop_reopens_and_closes_committed_cycle_fresh_prompt() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        let seeded = fs::read_to_string(&doc).unwrap().replace(
            "❯ Hello\n<!-- /agent:exchange -->",
            "❯ Hello\n\n### Re: Hello — gpt-5\n\nInitial response.\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &seeded).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &seeded,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let prompt_edit = original.replace(
            "<!-- /agent:exchange -->",
            "\n❯ • Hook stopped\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, prompt_edit).unwrap();
        track_doc(&dir, &doc, "turn-1");

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            // `#steerinterruptexit`: `steering pending` (no Codex thread in the
            // environment) or the active-session drift `INTERRUPTED` (a Codex
            // thread bound); the Stop hook takes the same path for both.
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message)
            | agent_doc_session_check_io::SessionCheckStatus::SteeringPending(message) => {
                assert!(is_committed_prompt_diff_interruption(&message), "{message}");
            }
            other => panic!("expected committed prompt-diff session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Recovered and closed the interrupted hook work.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        assert_eq!(response, StopResponse::AllowExit);
        let closed = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(closed.phase.as_str(), "committed");
        assert!(
            fs::read_to_string(&doc)
                .unwrap()
                .contains("Recovered and closed the interrupted hook work."),
            "recursive Stop must reopen the terminal predecessor and persist the response instead of prescribing finalize against a committed cycle"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_post_commit_prompt_cycle_reopened")
                && ops.contains("codex_stop_post_commit_prompt_auto_closed"),
            "recursive Stop must cross the same binary-owned reopen/close path as the first Stop attempt:\n{ops}"
        );
    }

    /// `#stopsteeringafterabandon`: agent-doc-bugs.md, 2026-10-14. The first
    /// Stop failed while an operator edit temporarily fragmented the exchange;
    /// owner release then abandoned the empty preflight. The recursive Stop
    /// still carried settled steering, but the generic recursion bound ended
    /// the turn with "already continued once". It must instead mint one
    /// successor cycle and let that same recursive Stop chain close it.
    #[test]
    fn recursive_stop_opens_and_closes_successor_after_empty_cycle_abandoned_with_steering() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let committed = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Earlier prompt\n\n",
            "### Re: Earlier prompt — gpt-5\n\n",
            "Earlier response.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let abandoned =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed))
                .unwrap();
        agent_doc_cycle_state_io::mark_abandoned(
            &doc,
            "cancel_preflight_cycle_abandoned",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        let with_steering = committed.replace(
            "<!-- /agent:exchange -->",
            "\n❯ Diagnose the recursive Stop stall.\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &with_steering).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let continued = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "The prior Stop reported its recovery boundary.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();
        assert!(
            matches!(&continued, StopResponse::ContinueTurn { reason, .. }
                if reason.contains("opened successor cycle")
                    && reason.contains("Diagnose the recursive Stop stall")),
            "{continued:?}"
        );
        let successor = agent_doc_cycle_state_io::load(&doc)
            .unwrap()
            .expect("Stop successor cycle");
        assert_ne!(successor.cycle_id, abandoned.cycle_id);
        assert_eq!(successor.phase.as_str(), "preflight_started");
        assert_eq!(
            successor.last_event,
            agent_doc_cycle_state_io::CODEX_STOP_STEERING_CONTINUATION_EVENT
        );
        assert!(successor.capture_id.is_none());
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("prior Stop reported"),
            "opening the successor must not capture the predecessor's status text"
        );

        let closed = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: recursive Stop stall — gpt-5\n\n",
                "The successor-cycle handoff now continues automatically.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: true,
        })
        .unwrap();
        assert_eq!(closed, StopResponse::AllowExit);
        let terminal = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(terminal.cycle_id, successor.cycle_id);
        assert_eq!(terminal.phase.as_str(), "committed");
        assert!(
            fs::read_to_string(&doc)
                .unwrap()
                .contains("successor-cycle handoff now continues automatically")
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("codex_stop_abandoned_steering_successor_opened")
                && ops.contains("codex_stop_steering_continuation_auto_closed"),
            "{ops}"
        );
    }
}

#[cfg(test)]
mod stop_input_tests {
    use super::*;

    #[test]
    fn stop_input_accepts_null_last_assistant_message() {
        // `#stopnullmessage`: the shape Codex sends when a turn ends on a tool call.
        let input: StopInput = serde_json::from_str(
            r#"{"session_id":"s","turn_id":"t","cwd":"/w","hook_event_name":"Stop","last_assistant_message":null,"stop_hook_active":null}"#,
        )
        .expect("a null message is a tool-only stop, not a malformed payload");
        assert_eq!(input.last_assistant_message, "");
        assert!(!input.stop_hook_active);
    }

    #[test]
    fn stop_input_keeps_present_and_absent_fields() {
        let input: StopInput = serde_json::from_str(
            r#"{"session_id":"s","turn_id":"t","cwd":"/w","last_assistant_message":"done","stop_hook_active":true}"#,
        )
        .unwrap();
        assert_eq!(input.last_assistant_message, "done");
        assert!(input.stop_hook_active);
        let input: StopInput =
            serde_json::from_str(r#"{"session_id":"s","turn_id":"t","cwd":"/w"}"#).unwrap();
        assert_eq!(input.last_assistant_message, "");
        assert!(!input.stop_hook_active);
    }
}
