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
//! - If the cycle still cannot be closed automatically, the hook falls back to
//!   blocking the turn with instructions to finish recovery/persistence.
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
    SessionState, clear_state_across_roots, load_state_any, parked_session_state,
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

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum StopResponse {
    Continue {
        #[serde(rename = "continue")]
        continue_: bool,
    },
    Block {
        decision: &'static str,
        reason: String,
    },
    Stop {
        #[serde(rename = "continue")]
        continue_: bool,
        #[serde(rename = "stopReason")]
        stop_reason: String,
    },
}

enum StopCloseAttempt {
    Closed,
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
    NotPossible,
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
const STOP_HOOK_BUDGET_ENV: &str = "AGENT_DOC_CODEX_STOP_HOOK_BUDGET_SECS";
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

fn stop_hook_budget() -> std::time::Duration {
    resolve_stop_hook_budget(std::env::var(STOP_HOOK_BUDGET_ENV).ok().as_deref())
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
        .and_then(|input| apply_stop_within_budget(input, stop_hook_budget()))
    {
        Ok(run) => run,
        Err(err) => StopHookRun {
            response: StopResponse::Stop {
                continue_: false,
                stop_reason: format!("agent-doc Stop hook failed closed: {err}"),
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
            agent_doc_controller_io::project_controller::undrained_supervisor_drain_handoff_age(
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
    if cycle.as_ref().is_some_and(|cycle| cycle.response_sha256.is_none()) {
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
        log_claimed_heads_waiting(&file);
        return Ok(None);
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
        run_id.as_deref(),
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
    agent_doc_queue_io::continuation_request::record_continuation_request(
        &file,
        run_id.as_deref(),
        &prompt,
    )
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
            run_id.as_deref().unwrap_or("none"),
        ),
    );
    Ok(Some(ClaudeStopContinuation {
        reason: claude_stop_continuation_reason(&file.display().to_string(), &prompt),
    }))
}

/// `#queueclaim`: when no drainable head remains but live heads are claimed by
/// workers outside the in-session loop (dispatched subagents), the turn ends
/// quietly and the document is waiting on that in-flight work. Record the
/// waiting state so the quiet stop is explainable from ops.log.
fn log_claimed_heads_waiting(file: &Path) {
    let content = match current_document_content(file, "claude_stop_claimed_heads_check") {
        Ok(content) => content,
        Err(err) => {
            eprintln!(
                "[agent-doc] Claude Stop hook could not read {} to report claimed queue heads: {err:#}",
                file.display()
            );
            return;
        }
    };
    let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(file, &content);
    let claimed_heads =
        agent_doc_queue::queue_continuation::claimed_head_count(&content, &claimed);
    if claimed_heads > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "claude_stop_queue_continuation_skipped reason=heads_claimed_in_flight \
                 claimed_heads={claimed_heads} action=allow_final_answer state=waiting_on_claims"
            ),
        );
    }
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

fn apply_stop_within_budget(input: StopInput, budget: std::time::Duration) -> Result<StopHookRun> {
    #[cfg(test)]
    if let Some(delay_ms) = std::env::var(STOP_HOOK_TEST_DELAY_MS_ENV)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
    {
        return run_stop_hook_task_within_budget(budget, move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            Ok(StopResponse::Continue { continue_: true })
        });
    }
    run_stop_hook_task_within_budget(budget, move || apply_stop(&input))
}

fn run_stop_hook_task_within_budget<F>(budget: std::time::Duration, task: F) -> Result<StopHookRun>
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
    match receiver.recv_timeout(budget) {
        Ok(response) => response.map(|response| StopHookRun {
            response,
            timed_out: false,
        }),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(StopHookRun {
            response: StopResponse::Stop {
                continue_: false,
                stop_reason: format!(
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
    load_state_any(&roots, session_id)
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
        return Ok(StopResponse::Continue { continue_: true });
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
        return Ok(StopResponse::Continue { continue_: true });
    }
    if !file.exists() {
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        return Ok(StopResponse::Continue { continue_: true });
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
            apply_bound_stop(input, &loaded_root, &state, &file, &cleanup_roots)
        });
    }

    apply_bound_stop(input, &loaded_root, &state, &file, &cleanup_roots)
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
            stop_phase("settle_session_binding");
            settle_session_binding(file, cleanup_roots, loaded_root, state)?;
            Ok(StopResponse::Continue { continue_: true })
        }
        agent_doc_session_check_io::SessionCheckStatus::Interrupted(reason) => {
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
            if try_resume_captured_finalize_in_hook(file) {
                return apply_stop(input);
            }
            if binary_owned_closeout_pending {
                agent_doc_ops_log_io::log_op(file, "codex_stop_binary_owned_closeout_pending");
                let display = file.display();
                let message = format!(
                    "agent-doc Stop hook found a binary-owned closeout still converging for {display}. {reason} The captured response is retained and the agent-doc binary/supervisor owns the keyed editor/CRDT retry and terminal commit. Do not recapture the response, rerun finalize, kill the controller, or use `--force-disk`; only re-check session status after the binary reports recovery, unless it explicitly reports `needs_operator`. Do not send the final answer yet."
                );
                if input.stop_hook_active {
                    return Ok(StopResponse::Stop {
                        continue_: false,
                        stop_reason: message,
                    });
                }
                return Ok(StopResponse::Block {
                    decision: "block",
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
                && let Some(response) = committed_prompt_diff_stop_response(file, &reason)?
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
            if !input.stop_hook_active || recursive_post_commit_prompt {
                let stop_closeout = match attempt_stop_closeout(file, state, input) {
                    Ok(stop_closeout) => stop_closeout,
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!("codex_stop_auto_close_failed err={err}"),
                        );
                        return Ok(StopResponse::Block {
                            decision: "block",
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
                        return Ok(StopResponse::Continue { continue_: true });
                    }
                    StopCloseAttempt::DeferredToDurableOwner => {
                        return Ok(StopResponse::Continue { continue_: true });
                    }
                    StopCloseAttempt::RepairDeferredToDurableOwner { note } => {
                        return Ok(durable_owner_repair_deferral_response(
                            file,
                            &note,
                            input.stop_hook_active,
                        ));
                    }
                    StopCloseAttempt::StillOpen { note } => {
                        return Ok(StopResponse::Block {
                            decision: "block",
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

            let capture_note = if input.stop_hook_active {
                String::new()
            } else {
                capture_assistant_text(file, state, input)
            };
            let display = file.display();
            if input.stop_hook_active {
                if let Some(response) = committed_prompt_diff_stop_response(file, &reason)? {
                    return Ok(response);
                }
                return Ok(StopResponse::Stop {
                    continue_: false,
                    stop_reason: format!(
                        "agent-doc Stop hook already continued once for {display}, but the cycle is still open. {reason}{capture_note}"
                    ),
                });
            }
            Ok(StopResponse::Block {
                decision: "block",
                reason: format!(
                    "agent-doc Stop hook intercepted an unfinished document cycle for {display}. {reason}{capture_note} Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair {display}` first. Then finish the commit boundary for this turn and end with `agent-doc session-check {display}`."
                ),
            })
        }
    }
}

fn try_resume_captured_finalize_in_hook(file: &Path) -> bool {
    invalidate_stop_document_cache();
    let Some(key) = agent_doc_repair_command_io::captured_finalize_resume_key(file)
        .ok()
        .flatten()
    else {
        return false;
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
                return true;
            }
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded => {
                return agent_doc_session_check_io::inspect(
                    file,
                    &agent_doc_closeout_runtime_io::session_check_effects(),
                )
                .is_ok_and(|status| {
                    matches!(
                        status,
                        agent_doc_session_check_io::SessionCheckStatus::Ok(_)
                    )
                });
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
                return false;
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
                return false;
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
                return false;
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
                return false;
            }
        }
    }
    false
}

fn is_binary_owned_closeout_interruption(reason: &str) -> bool {
    reason.contains("closeout blocked by `editor_convergence_required`")
        || (reason.contains("editor_convergence_required")
            && reason.contains("operator_text_authority_v1"))
        || (reason.contains("binary-owned response delivery `")
            && reason.contains(" is retained for `")
            && reason.contains("Same-capture recovery remains pending"))
        || (reason.contains("cycle `")
            && reason.contains(" is still `write_applied`")
            && reason.contains("response write landed but no terminal commit followed"))
}

fn committed_prompt_diff_stop_response(file: &Path, reason: &str) -> Result<Option<StopResponse>> {
    if !is_committed_prompt_diff_interruption(reason) {
        return Ok(None);
    }
    // `#queuetypingsteer`: a `content_edit` is the operator's own edit inside an
    // existing queue item or prompt; name that edit, not some other prompt the
    // exchange happens to hold.
    let steering_is_content_edit = reason.find("content_edit:").is_some_and(|at| {
        reason
            .find("prompt_target:")
            .is_none_or(|target| at < target)
    });
    let prompt = if steering_is_content_edit {
        prompt_target_from_interruption_reason(reason)
    } else {
        agent_doc_session_check_io::unresolved_exchange_prompt(file)?
            .or_else(|| prompt_target_from_interruption_reason(reason))
    }
    .unwrap_or_else(|| "the unresolved exchange prompt".to_string());
    Ok(Some(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook found fresh unresolved exchange work for {disp} after the previous cycle was already committed. Continue THIS turn in-pane: answer {prompt:?} in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`). Do NOT send the final answer yet.",
            disp = file.display(),
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

    if agent_doc_flow_io::closeout::cycle_already_committed(file).is_some()
        && matches!(
            agent_doc_template::replay_guard::classify_replay_payload(
                &input.last_assistant_message
            ),
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
        )
    {
        match attempt_stop_closeout(file, state, input)? {
            StopCloseAttempt::Closed => {
                settle_session_binding(file, roots, loaded_root, state)?;
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_post_commit_prompt_auto_closed source=exact_thread_prompt_debt",
                );
                return Ok(Some(StopResponse::Continue { continue_: true }));
            }
            StopCloseAttempt::DeferredToDurableOwner => {
                agent_doc_ops_log_io::log_op(
                    file,
                    "codex_stop_post_commit_prompt_closeout_deferred owner=durable_retained_capture",
                );
                return Ok(Some(StopResponse::Continue { continue_: true }));
            }
            StopCloseAttempt::RepairDeferredToDurableOwner { note } => {
                return Ok(Some(durable_owner_repair_deferral_response(
                    file,
                    &note,
                    input.stop_hook_active,
                )));
            }
            StopCloseAttempt::StillOpen { note } => {
                return Ok(Some(StopResponse::Block {
                    decision: "block",
                    reason: format!(
                        "agent-doc Stop hook opened a fresh cycle for post-commit prompt work in {disp}, then retained the response under that cycle but could not finish closeout.{note} Do not recapture the response or rerun finalize/write. Run `agent-doc session-check {disp}` and let the binary-owned keyed retry finish.",
                        disp = file.display(),
                    ),
                }));
            }
            StopCloseAttempt::NotPossible => {}
        }
    }
    let capture_note = if input.stop_hook_active {
        String::new()
    } else {
        capture_assistant_text(file, state, input)
    };
    Ok(Some(StopResponse::Block {
        decision: "block",
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
    if let Some(prompt) = agent_doc_session_check_io::unresolved_exchange_prompt(file)? {
        return Ok(Some(prompt));
    }
    let content = current_document_content(file, "codex_stop_active_session_queue_head")?;
    Ok(first_active_queue_prompt_in_content(&content))
}

fn first_active_queue_prompt_in_content(content: &str) -> Option<String> {
    let prompt = agent_doc_queue::queue_continuation::pending_head_prompt_text(
        content,
        agent_doc_queue::queue_continuation::DrainScope::InSessionLoop,
    )?;
    if agent_doc_codex_hook_io::is_context_clear_prompt(&prompt)
        || agent_doc_queue::queue_command::slash_command_text(&prompt).is_some()
    {
        return None;
    }
    Some(prompt)
}

fn background_context_clear_suppression_response(
    file: &Path,
    prompt: &str,
    source: &str,
    context_reset_reason: Option<&str>,
) -> Option<StopResponse> {
    let reason = context_reset_reason?;
    agent_doc_codex_hook_io::log_codex_background_context_clear_suppressed(
        file, prompt, source, reason,
    );
    None
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

    agent_doc_repair_io::pending::save_pending(file, &response_to_write)?;
    agent_doc_ops_log_io::log_op(file, "codex_stop_repeated_queue_response_saved");
    let mut note = format!(
        " The hook replayed the last assistant response into `agent:exchange` for repeated queue head {:?}.",
        prompt
    );

    let repair_outcome = agent_doc_repair_io::run_with_queue_completion_ids(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        &queue_completion_ids,
    )?;
    if repair_outcome.replayed_response() {
        note.push_str(" The response was written through the normal repair/write path.");
    } else if repair_outcome == agent_doc_turn::repair::RepairOutcome::AlreadyApplied {
        note.push_str(" The response was already present and was adopted by repair.");
    } else {
        return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
            note: format!(
                "{note} The repair path did not replay or adopt the response (outcome: {repair_outcome:?})."
            ),
        });
    }

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

    if repair_outcome.replayed_response()
        && agent_doc_flow_io::closeout::replay_closeout_still_proven(
            file,
            &current_document_content(file, "codex_stop_queue_replay_terminal_receipt")?,
        )?
    {
        agent_doc_ops_log_io::log_op(
            file,
            "codex_stop_repeated_queue_recovery_success source=strict_replay_receipt",
        );
        return Ok(RepeatedQueueHeadRecovery::Recovered { note });
    }
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
    StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook requested auto-queue continuation for {}, but the queue head did not advance after the previous continuation request: {:?}. The hook could not safely replay the last assistant response.{} {}",
            file.display(),
            prompt,
            note,
            render_prompt_continuation_instruction(
                &file.display().to_string(),
                agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
                None,
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
    let Some(next_prompt) = active_auto_queue_prompt(file)? else {
        park_state_across_roots(cleanup_roots, loaded_root, state)?;
        return Ok(StopResponse::Continue { continue_: true });
    };
    if next_prompt == prompt {
        return Ok(StopResponse::Block {
            decision: "block",
            reason: format!(
                "agent-doc Stop hook replayed a response for {}, but the queue head still did not advance: {:?}.{} {}",
                file.display(),
                prompt,
                note,
                render_prompt_continuation_instruction(
                    &file.display().to_string(),
                    agent_doc_codex_hook_io::agent_doc_mcp_configured_for(file),
                    None,
                )
            ),
        });
    }

    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(next_prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    let context_reset_reason =
        agent_doc_codex_hook_io::codex_continuation_clear_reason(file, state.last_context_clear_at);
    if let Some(response) = background_context_clear_suppression_response(
        file,
        &next_prompt,
        "tracked_state_after_recovery",
        context_reset_reason.as_deref(),
    ) {
        return Ok(response);
    }
    Ok(StopResponse::Block {
        decision: "block",
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
                        context_reset_reason.as_deref(),
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
    if agent_doc_document_realtime_io::retained_write_blocks_session_closeout(
        file,
        "codex_stop_auto_queue_continuation_gate",
    ) {
        let pending = agent_doc_document_realtime_io::pending_document_write(file);
        let retry_key = pending
            .as_ref()
            .map(|intent| intent.intent_id.clone())
            .unwrap_or_else(|| "unobserved-retained-intent".to_string());
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
                "codex_stop_auto_queue_continuation_deferred intent_id={} action=await_retained_write_terminal queue_advanced=false capture_created=false",
                retry_key,
            ),
        );
        return Ok(Some(retained_write_queue_deferral_response(
            file,
            &retry_key,
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
    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    let context_reset_reason =
        agent_doc_codex_hook_io::codex_continuation_clear_reason(file, state.last_context_clear_at);
    agent_doc_codex_hook_io::log_codex_stop_queue_continuation(file, &prompt, "tracked_state");
    if let Some(response) = background_context_clear_suppression_response(
        file,
        &prompt,
        "tracked_state",
        context_reset_reason.as_deref(),
    ) {
        return Ok(Some(response));
    }
    // #codex-self-reinvoke-prevent (Option B): redirect the auto-queue
    // continuation to an IN-PANE answer + persist instead of instructing Codex to
    // run `agent-doc <FILE>` again. Re-running the entrypoint from the owner pane
    // re-enters the pane it runs in and trips the recursive-direct-invocation
    // deadlock guard; answering the next prompt in this same turn and persisting
    // with `agent-doc finalize <FILE>` (a non-dispatch command) continues the
    // queue without any nested self-invocation. This matches the run-path guard's
    // own OwnedPaneSelfInvocation guidance so both sources agree.
    Ok(Some(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook kept an active `agent:queue auto` moving for {disp}. The next queue prompt is {prompt:?}. {instruction}",
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
                        context_reset_reason.as_deref(),
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
    let Some(last_head) = last_auto_queue_head.map(str::trim).filter(|head| !head.is_empty()) else {
        return false;
    };
    phase == agent_doc_turn::CyclePhase::PreflightStarted
        && !has_capture
        && !has_response_hash
        && predecessor != current_cycle_id
        && cycle_prompts.any(|prompt| prompt.trim() == last_head)
}

fn retained_write_queue_deferral_response(
    file: &Path,
    retry_key: &str,
    stop_hook_active: bool,
) -> StopResponse {
    let message = format!(
        "agent-doc Stop hook kept `agent:queue auto` paused for {} because the prior binary-owned editor write is still converging (intent_id={retry_key}). The same owner pane continues only after that retained delivery and its terminal closeout settle. The hook did not select a new prompt or create another capture. Do not resend, recapture, rerun finalize/write/repair, force disk, or recycle; wait for the existing controller state edge unless it explicitly reports `needs_operator`. Do not send the final answer yet.",
        file.display(),
    );
    if stop_hook_active {
        StopResponse::Stop {
            continue_: false,
            stop_reason: message,
        }
    } else {
        StopResponse::Block {
            decision: "block",
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
    format!(
        " The hook could not finish the required commit boundary: {}. {}.",
        format!("{err:#}").replace('\n', " "),
        agent_doc_turn::write_ownership::retained_write_remedy(
            ownership,
            &file.display().to_string()
        ),
    )
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
        StopResponse::Stop {
            continue_: false,
            stop_reason: message,
        }
    } else {
        StopResponse::Block {
            decision: "block",
            reason: message,
        }
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
    reopen_terminal_cycle_before_stop_capture(file, &payload)?;
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
            &current_document_content(file, "codex_stop_replay_terminal_receipt")?,
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
            if closeout_failure_is_binary_owned_deferral(&rendered, ownership.verdict()) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "codex_stop_auto_close_closeout_deferred verdict={} reason=captured_response_not_materialized owner=durable_retained_capture err={rendered}",
                        ownership.verdict().as_str(),
                    ),
                );
                return Ok(StopCloseAttempt::DeferredToDurableOwner);
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

/// Whether a failed commit boundary is a deferral the binary already owns.
///
/// Only the missing-captured-response refusal qualifies, and only while the
/// shared ownership predicate says something durable holds the intent
/// (`Deferred`): the retained capture's keyed worker projects the body and
/// commits on the next delivery edge. Every other verdict, including a durable
/// but unowned capture, stays fail-closed so the agent is told to act.
fn closeout_failure_is_binary_owned_deferral(
    rendered_error: &str,
    verdict: agent_doc_turn::write_ownership::RetainedWriteVerdict,
) -> bool {
    agent_doc_git_io::capture_materialization_guard::is_missing_captured_response_refusal(
        rendered_error,
    ) && verdict == agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred
}

/// Whether repair has already handed the exact write to the controller-owned
/// retained-intent retry. Validation, parse, and evidence failures are excluded:
/// those need a corrected owner response and cannot converge from a state edge.
fn repair_failure_is_binary_owned_deferral(
    rendered_error: &str,
    verdict: agent_doc_turn::write_ownership::RetainedWriteVerdict,
) -> bool {
    verdict == agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred
        && agent_doc_turn::write_ownership::is_retained_write_refusal(rendered_error)
}

fn reopen_terminal_cycle_before_stop_capture(
    file: &Path,
    payload: &agent_doc_template::replay_guard::ReplayPayloadClassification<'_>,
) -> Result<()> {
    let agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) =
        payload
    else {
        return Ok(());
    };
    let Some(previous_cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file) else {
        return Ok(());
    };
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        anyhow::bail!(
            "post-commit Stop closeout cannot mint a fresh capture cycle without a HEAD baseline"
        );
    };
    if agent_doc_turn::response_replay::response_materialized_in_content(response.as_ref(), &head) {
        anyhow::bail!(
            "post-commit Stop payload is already materialized in HEAD; refusing to capture it under the terminal cycle"
        );
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

    let reopened = agent_doc_cycle_state_io::start_preflight(file, Some(&head), Some(&current))?;
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
    Ok(())
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
    Ok(agent_doc_queue_io::queue_continuation::detect(file)?
        .map(|continuation| continuation.head_prompt))
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
        assert!(wake < skill, "the working re-entry must come first: {reason}");
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

    /// The Codex Stop contract is untouched by the Claude feedback form.
    #[test]
    fn codex_stop_output_shapes_are_unchanged() {
        assert_eq!(
            serde_json::to_value(StopResponse::Block {
                decision: "block",
                reason: "r".to_string(),
            })
            .unwrap(),
            serde_json::json!({"decision": "block", "reason": "r"})
        );
        assert_eq!(
            serde_json::to_value(StopResponse::Continue { continue_: true }).unwrap(),
            serde_json::json!({"continue": true})
        );
        assert_eq!(
            serde_json::to_value(StopResponse::Stop {
                continue_: false,
                stop_reason: "s".to_string(),
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
        assert!(is_stop_hook_feedback("<system-reminder>\nStop hook feedback"));
        assert!(!is_stop_hook_feedback("please fix the parser"));
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
        assert!(matches!(result, StopResponse::Continue { continue_: true }));
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
    fn stop_hook_budget_returns_a_fail_closed_response_before_outer_timeout() {
        let run = run_stop_hook_task_within_budget(std::time::Duration::from_millis(1), || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            Ok(StopResponse::Continue { continue_: true })
        })
        .unwrap();

        assert!(run.timed_out);
        assert!(matches!(
            run.response,
            StopResponse::Stop {
                continue_: false,
                ..
            }
        ));
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
            .env(STOP_HOOK_TEST_DELAY_MS_ENV, "5000")
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

        let started = std::time::Instant::now();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "child failed: {output:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "timed-out hook waited for the detached worker: {:?}",
            started.elapsed()
        );
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

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let old = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.old.as_ref() {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
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

        assert_eq!(response, StopResponse::Continue { continue_: true });

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
        assert!(closeout_failure_is_binary_owned_deferral(
            &refusal,
            owned.verdict()
        ));

        // Fail closed whenever nothing durable owns the intent.
        for verdict in [
            RetainedWriteVerdict::Stranded,
            RetainedWriteVerdict::CaptureResumeUnowned,
            RetainedWriteVerdict::AwaitingTerminalCommit,
            RetainedWriteVerdict::UnansweredEditPending,
        ] {
            assert!(
                !closeout_failure_is_binary_owned_deferral(&refusal, verdict),
                "{verdict:?} must stay fail-closed"
            );
        }
        // Any other commit failure stays fail-closed even with a durable owner.
        assert!(!closeout_failure_is_binary_owned_deferral(
            "git commit failed: index.lock exists",
            RetainedWriteVerdict::Deferred
        ));
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
        let deferral = commit
            .find("closeout_failure_is_binary_owned_deferral(")
            .expect("commit failure must consult the durable-owner deferral");
        let still_open = commit
            .find("StopCloseAttempt::StillOpen")
            .expect("unowned failures stay still-open");
        assert!(deferral < still_open);
        assert!(commit.contains("StopCloseAttempt::DeferredToDurableOwner"));
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
        let first =
            durable_owner_repair_deferral_response(Path::new("/p/fpe.md"), &note, false);
        assert!(matches!(first, StopResponse::Block { .. }));
        let recursive =
            durable_owner_repair_deferral_response(Path::new("/p/fpe.md"), &note, true);
        assert!(matches!(recursive, StopResponse::Stop { .. }));
        let rendered = serde_json::to_string(&recursive).unwrap();
        assert!(rendered.contains(retry_key), "stable retry key preserved: {rendered}");
        assert!(rendered.contains("Do not reopen or recapture the cycle"));
        assert!(rendered.contains("Wait for the existing controller state edge"));
        assert!(rendered.contains("needs_operator"));
        assert!(!rendered.contains("run `agent-doc repair"));
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
            .find("retained_write_blocks_session_closeout(")
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

        let prompts = ["I want to demo chatting and triggering the FPE then it showing up in the dashboard."];
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
            assert!(!unsafe_shape, "only the proven empty auto-queue child is disposable");
        }

        let retry_key = "1790972821780862047-1-b19f206141461c9678dcc6f01db3826c43654fdd137ef7b7fcb00ecaa416e667";
        for stop_hook_active in [false, true] {
            let response = retained_write_queue_deferral_response(
                Path::new("/p/fpe.md"),
                retry_key,
                stop_hook_active,
            );
            assert_eq!(
                matches!(&response, StopResponse::Stop { .. }),
                stop_hook_active,
            );
            let rendered = serde_json::to_string(&response).unwrap();
            assert!(rendered.contains(retry_key));
            assert!(rendered.contains("did not select a new prompt or create another capture"));
            assert!(rendered.contains("same owner pane continues only after"));
            assert!(rendered.contains("needs_operator"));
        }
    }

    #[test]
    fn retained_predecessor_quarantines_already_created_empty_queue_child_metadata_only() {
        let dir = setup_project();
        let prompt = "I want to demo chatting and triggering the FPE then it showing up in the dashboard.";
        let doc = write_auto_queue_doc(&dir, &[prompt]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();

        let parent = agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(&original),
            Some(&original),
        )
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

        let child = agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(&original),
            Some(&original),
        )
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
        assert_eq!(first_stop, StopResponse::Continue { continue_: true });
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
            StopResponse::Continue { continue_: true },
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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
    fn stop_blocks_direct_chat_manual_queue_response_after_recursive_guard() {
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

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("active session-document work"), "{reason}");
                assert!(reason.contains("2300 words"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(reason.contains("agent-doc write --commit"), "{reason}");
                assert!(reason.contains("agent-doc session-check"), "{reason}");
                assert!(reason.contains("pending/capture ledger"), "{reason}");
            }
            other => panic!("expected direct-chat writeback block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("counting characters"),
            "chat-only answer must not be treated as document closeout"
        );
        let pending = agent_doc_repair_io::pending::load_active_pending_response(&doc)
            .unwrap()
            .unwrap();
        assert!(
            pending.contains("counting characters"),
            "the hook should capture the replayable answer for recovery"
        );
    }

    #[test]
    fn stop_blocks_consecutive_direct_chat_manual_queue_answers() {
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
        assert!(
            matches!(first, StopResponse::Block { .. }),
            "first direct-chat closeout must be blocked"
        );

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

        match second {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("Remove the max character count cap"),
                    "{reason}"
                );
                assert!(reason.contains("agent-doc write --commit"), "{reason}");
                assert!(reason.contains("agent-doc session-check"), "{reason}");
            }
            other => panic!("expected second direct-chat writeback block, got {other:?}"),
        }
        let pending_body = agent_doc_repair_io::pending::load_active_pending_response(&doc)
            .unwrap()
            .unwrap();
        assert!(pending_body.contains("Second direct-chat answer."));
        assert!(!pending_body.contains("First direct-chat answer."));
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
            StopResponse::Block { reason, .. } => {
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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
        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK.lock();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

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

        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK.lock();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

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

        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(response, StopResponse::Continue { continue_: true });
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
    }

    #[test]
    fn stop_passes_through_committed_cycle_with_stopped_queue_head() {
        // A halt writes `queue: stop` AND strips the marker's `go`. A leftover
        // `go` beside `stop` is an explicit marker control that wins on every
        // activation reader (`#qbindingone`, GH #79), so it is not a stopped queue.
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = "---\nsession: sid\nqueue: stop\n---\n\n\
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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
                first_active_queue_prompt_in_content(&document(queue)).as_deref(),
                expected,
                "queue: {queue}"
            );
        }
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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

    /// `#queueclaim`: a head dispatched to a subagent is claimed. When every
    /// remaining head is claimed, the Stop hook must let the turn end quietly
    /// (no continuation) instead of re-entering for work already in flight;
    /// releasing the claim restores the continuation.
    #[test]
    fn claude_stop_does_not_reenter_for_claimed_heads() {
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
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(ops.contains("state=waiting_on_claims"), "{ops}");

        // Releasing a claim restores drainability.
        complete_run(&doc);
        agent_doc_queue_io::queue_claim::release(
            &doc,
            "#gh-fix https://github.com/o/r/issues/109",
        )
        .unwrap();
        let resumed = apply_claude_stop_with_drain_readiness(&input, no_supervisor)
            .unwrap()
            .expect("a released head is drainable again");
        assert!(resumed.reason.contains("issues/109"), "{}", resumed.reason);
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
        agent_doc_cycle_state_io::mark_response_captured(
            doc,
            "response_captured",
            Some(&content),
            Some(&content),
            "response-sha",
            None,
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

        assert_eq!(response, StopResponse::Continue { continue_: true });

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

        assert_eq!(response, StopResponse::Continue { continue_: true });

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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
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

        assert_eq!(response, StopResponse::Continue { continue_: true });
    }

    #[test]
    fn stop_tracked_session_suppresses_background_clear_after_exchange_compaction() {
        let dir = setup_project();
        // Background context clears are disabled even when the document opted into
        // queue context reset. The Stop hook should keep queue continuation in-pane.
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
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert!(
                    reason.contains("automatic context clearing is disabled"),
                    "{reason}"
                );
                assert!(!reason.contains("Run `/clear`"), "{reason}");
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
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert!(
                    reason.contains("automatic context clearing is disabled"),
                    "{reason}"
                );
                assert!(!reason.contains("Run `/clear`"), "{reason}");
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
        let _home_guard = EnvGuard::set("HOME", home.path());
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
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert!(
                    reason.contains("automatic context clearing is disabled"),
                    "{reason}"
                );
                assert!(!reason.contains("Run `/clear`"), "{reason}");
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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
                assert!(reason.contains("agent-doc write --commit"), "{reason}");
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Block { reason, .. } => {
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
            StopResponse::Stop {
                continue_: false,
                stop_reason,
            } => {
                assert!(stop_reason.contains("already continued once"));
                assert!(stop_reason.contains("cycle is still open"));
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

        assert!(
            matches!(response, StopResponse::Continue { continue_: true }),
            "{response:?}"
        );
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(state.last_prompt.is_empty());
        assert!(state.last_turn_id.is_empty());
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
        assert!(
            matches!(response, StopResponse::Continue { continue_: true }),
            "{response:?}"
        );
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert!(state.last_prompt.is_empty());
        assert!(state.last_turn_id.is_empty());
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

        reopen_terminal_cycle_before_stop_capture(&doc, &payload).unwrap();

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
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                assert!(is_committed_prompt_diff_interruption(&message), "{message}");
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
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
            matches!(&response, StopResponse::Block { reason, .. }
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

        assert!(
            matches!(response, StopResponse::Continue { continue_: true }),
            "{response:?}"
        );
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
        let across_notification =
            [schedule_wakeup(loop_prompt), tool_result.clone(), notification].join("\n");
        assert!(transcript_tail_arms_loop_reentry(&across_notification, file));

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

        let response = committed_prompt_diff_stop_response(&doc, reason)
            .unwrap()
            .expect("content_edit steering after a commit must be handed back in-pane");
        assert!(
            matches!(&response, StopResponse::Block { reason, .. }
                if reason.contains("fresh unresolved exchange work")
                    && reason.contains("Even if formal logic code is not directly understandable")),
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
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                assert!(is_committed_prompt_diff_interruption(&message), "{message}");
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Recovered and closed the interrupted hook work.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
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
