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
    SessionState, clear_state_across_roots, load_state_any, project_roots_for,
    save_state_across_roots, tracking_roots,
};
#[cfg(test)]
use agent_doc_codex_hook_io::{
    UserPromptSubmitInput, apply_user_prompt_submit, load_state, save_state,
};
use agent_doc_document::queue_projection::strip_in_progress_marker;
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
    #[allow(dead_code)]
    turn_id: String,
    cwd: String,
    #[serde(default)]
    last_assistant_message: String,
    #[serde(default)]
    stop_hook_active: bool,
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
    StillOpen { note: String },
    NotPossible,
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
    std::thread::Builder::new()
        .name("agent-doc-codex-stop".to_string())
        .spawn(move || {
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
                    "agent-doc Stop hook exceeded its {}s internal budget and failed closed before the harness timeout. The route-owned supervisor retains any captured closeout and continues recovery; do not rerun finalize or recapture the response.",
                    budget.as_secs(),
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
    if !file.exists() {
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        return Ok(StopResponse::Continue { continue_: true });
    }

    match agent_doc_session_check_io::inspect(
        &file,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )? {
        agent_doc_session_check_io::SessionCheckStatus::Ok(_) => {
            if let Some(response) = auto_queue_continuation_response(
                &file,
                &cleanup_roots,
                &loaded_root,
                &state,
                input,
            )? {
                return Ok(response);
            }
            if let Some(response) = active_session_prompt_requires_writeback(&file, &state, input)?
            {
                return Ok(response);
            }
            clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
            Ok(StopResponse::Continue { continue_: true })
        }
        agent_doc_session_check_io::SessionCheckStatus::Interrupted(reason) => {
            // `#binaryownedfinalize`: once the response is durably captured, the
            // Stop hook is a status gate, not a request for another agent-authored
            // finalize attempt. Give the binary's keyed repair/commit operation a
            // bounded opportunity to finish through editor/CRDT authority. The
            // route-owned supervisor continues the same operation after this hook
            // returns if convergence takes longer.
            let editor_convergence_blocked = is_editor_convergence_required_interruption(&reason);
            // The hook executes the freshly-installed binary even when the
            // route-owned supervisor still has an older inode. Resume the
            // existing keyed capture here as the version-independent liveness
            // boundary; strict repair preserves editor authority and never
            // recaptures or elects force-disk.
            if try_resume_captured_finalize_in_hook(&file) {
                return apply_stop(input);
            }
            if editor_convergence_blocked {
                agent_doc_ops_log_io::log_op(
                    &file,
                    "codex_stop_editor_convergence_required_blocked",
                );
                let display = file.display();
                let message = format!(
                    "agent-doc Stop hook found an editor-convergence blocked closeout for {display}. {reason} The captured response is retained and the agent-doc binary/supervisor owns the keyed editor/CRDT retry. Do not recapture the response, rerun finalize, kill the controller, or use `--force-disk`; only re-check session status after the binary reports recovery, unless it explicitly reports `needs_operator`. Do not send the final answer yet."
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
            if !input.stop_hook_active {
                let stop_closeout = match attempt_stop_closeout(&file, &state, input) {
                    Ok(stop_closeout) => stop_closeout,
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            &file,
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
                        if let Some(response) = auto_queue_continuation_response(
                            &file,
                            &cleanup_roots,
                            &loaded_root,
                            &state,
                            input,
                        )? {
                            return Ok(response);
                        }
                        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
                        return Ok(StopResponse::Continue { continue_: true });
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
                capture_assistant_text(&file, &state, input)
            };
            let display = file.display();
            if input.stop_hook_active {
                if let Some(response) = committed_prompt_diff_stop_response(&file, &reason)? {
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

fn is_editor_convergence_required_interruption(reason: &str) -> bool {
    reason.contains("closeout blocked by `editor_convergence_required`")
        || (reason.contains("editor_convergence_required")
            && reason.contains("operator_text_authority_v1"))
}

fn committed_prompt_diff_stop_response(file: &Path, reason: &str) -> Result<Option<StopResponse>> {
    if !is_committed_prompt_diff_interruption(reason) {
        return Ok(None);
    }
    let prompt = agent_doc_session_check_io::unresolved_exchange_prompt(file)?
        .or_else(|| prompt_target_from_interruption_reason(reason))
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
    state: &SessionState,
    input: &StopInput,
) -> Result<Option<StopResponse>> {
    let Some(prompt) = active_session_prompt_or_queue_head(file)? else {
        return Ok(None);
    };
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

fn active_session_prompt_or_queue_head(file: &Path) -> Result<Option<String>> {
    if let Some(prompt) = agent_doc_session_check_io::unresolved_exchange_prompt(file)? {
        return Ok(Some(prompt));
    }
    let content = current_document_content(file, "codex_stop_active_session_queue_head")?;
    Ok(first_active_queue_prompt_in_content(&content))
}

fn first_active_queue_prompt_in_content(content: &str) -> Option<String> {
    if agent_doc_queue::queue_heads::queue_is_explicitly_stopped(content) {
        return None;
    }
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    let entries = agent_doc_queue::document_queue::parse(queue.content(content)).ok()?;
    let prompt = agent_doc_queue::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| strip_in_progress_marker(&prompt.text))
        .map(|prompt| prompt.trim().to_string())
        .find(|prompt| !prompt.is_empty())?;
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
    input: &StopInput,
    prompt: &str,
    note: String,
) -> Result<StopResponse> {
    let Some(next_prompt) = active_auto_queue_prompt(file)? else {
        clear_state_across_roots(cleanup_roots, loaded_root, &input.session_id)?;
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
    let _ = agent_doc_queue_io::continuation_marker::record_continuation_requested_head(
        file,
        &next_prompt,
    );
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
    // Keep the durable marker's requested-head in sync for document-level
    // diagnostics. Ambient hooks still require this exact tracked session.
    let _ =
        agent_doc_queue_io::continuation_marker::record_continuation_requested_head(file, &prompt);
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

fn attempt_stop_closeout(
    file: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<StopCloseAttempt> {
    let mut phase_started = std::time::Instant::now();
    let payload =
        agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let has_response = matches!(
        payload,
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
    );
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

    let active_queue_prompt = active_auto_queue_prompt(file)?;
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
            agent_doc_repair_io::pending::save_pending(file, response.as_ref())?;
            agent_doc_ops_log_io::log_op(file, "codex_stop_capture_saved");
            note.push_str(
                " The latest assistant text was captured into the pending/capture ledger before auto-close.",
            );
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

    let repair_outcome = agent_doc_repair_io::run_with_queue_completion_ids(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        &queue_completion_ids,
    )?;
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

    match agent_doc_closeout_runtime_io::complete_required_closeout(file, false) {
        Ok(true) => {
            note.push_str(" The hook finished the commit boundary automatically.");
        }
        Ok(false) => {}
        Err(err) => {
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

fn capture_assistant_text(file: &Path, state: &SessionState, input: &StopInput) -> String {
    match agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message) {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
            capture_missing_stop_response(file, Some(state.last_prompt.as_str()))
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
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

fn current_document_content(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(file, source).with_context(
        || {
            format!(
                "{source}: failed to resolve current document {}",
                file.display()
            )
        },
    )
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
        assert!(load_state(&root, "codex-session").unwrap().is_none());
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
        assert!(load_state(&outer_root, "codex-session").unwrap().is_none());
        assert!(load_state(&nested_root, "codex-session").unwrap().is_none());
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
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = "---\nsession: sid\nqueue: stop\n---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: #advance-review — gpt-5\n\n\
Reviewed the gated items.\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue priority go -->\n\
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
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(compaction_ts),
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
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
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
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
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
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
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
    fn stop_hook_active_blocks_committed_cycle_fresh_prompt_instead_of_stopping() {
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
        fs::write(
            &doc,
            format!(
                "{original}\n❯ do #repair-false-closeouts. #spec-test-build-install-commit-push\n"
            ),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("is `committed`"), "{message}");
                assert!(
                    message.contains("no new agent-doc cycle started")
                        || message.contains("without reopening the binary-owned write/commit path"),
                    "{message}"
                );
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Still working.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("fresh unresolved exchange work"),
                    "{reason}"
                );
                assert!(
                    reason.contains("previous cycle was already committed"),
                    "{reason}"
                );
                assert!(reason.contains("do #repair-false-closeouts"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(!reason.contains("already continued once"), "{reason}");
            }
            other => panic!("expected block response, got {other:?}"),
        }
    }
}
