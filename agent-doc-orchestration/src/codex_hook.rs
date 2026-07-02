//! # Module: codex_hook
//!
//! ## Spec
//! - Implements the repo-local Codex hook bridge used by `agent-doc` installs.
//! - `handle_user_prompt_submit()` reads the Codex `UserPromptSubmit` JSON payload
//!   from stdin, finds the effective `agent-doc <FILE>`-style invocation even
//!   when the prompt body includes injected instruction preambles, and records
//!   the active document for the Codex session under
//!   `.agent-doc/codex-hooks/`.
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
//! - `user_prompt_submit_tracks_agent_doc_file`
//! - `stop_auto_closes_open_cycle_from_last_assistant_message`
//! - `stop_blocks_transcript_shaped_last_assistant_message`
//! - `stop_passes_through_committed_cycle`
//! - `stop_blocks_open_cycle_without_recoverable_response`
//! - `stop_fails_closed_after_one_auto_continue`

use agent_doc_document::queue_projection::strip_in_progress_marker;
use agent_doc_model_tier::context_transcript_io::{
    latest_codex_transcript, transcript_context_pct,
};
use agent_doc_model_tier::context_usage::{Harness, clear_decision};
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

#[derive(Debug, Deserialize)]
struct UserPromptSubmitInput {
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionState {
    session_id: String,
    doc_path: String,
    last_turn_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    last_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_auto_queue_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_context_clear_at: Option<u64>,
    updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionState {
    pub session_id: String,
    pub doc_path: String,
    pub last_turn_id: String,
    pub last_prompt: String,
    pub updated_at: u64,
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

#[derive(Debug, Serialize)]
struct BlockedStopPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    kind: &'a str,
    reason: &'a str,
    payload_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assistant_message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_prompt: Option<&'a str>,
}

pub fn handle_user_prompt_submit() -> Result<()> {
    let payload = match read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("[codex-hook] user-prompt-submit payload read failed: {err}");
            return Ok(());
        }
    };
    let input: UserPromptSubmitInput = match serde_json::from_str(&payload) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("[codex-hook] user-prompt-submit JSON parse failed: {err}");
            return Ok(());
        }
    };
    apply_user_prompt_submit(&input)
}

pub fn handle_stop() -> Result<()> {
    let response = match read_stdin_payload()
        .and_then(|payload| serde_json::from_str::<StopInput>(&payload).context("parse stop JSON"))
        .and_then(|input| apply_stop(&input))
    {
        Ok(response) => response,
        Err(err) => StopResponse::Stop {
            continue_: false,
            stop_reason: format!("agent-doc Stop hook failed closed: {err}"),
        },
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

fn apply_user_prompt_submit(input: &UserPromptSubmitInput) -> Result<()> {
    let cwd = PathBuf::from(&input.cwd);
    let roots_for_cwd = project_roots_for(&cwd);
    let previous_state = load_state_any(&roots_for_cwd, &input.session_id)?.map(|(_, state)| state);
    let doc_path = resolve_agent_doc_path(&input.prompt, &cwd).or_else(|| {
        previous_state
            .as_ref()
            .map(|state| PathBuf::from(&state.doc_path))
    });
    let Some(doc_path) = doc_path else {
        return Ok(());
    };
    let roots = tracking_roots(&cwd, Some(&doc_path));
    if roots.is_empty() {
        return Ok(());
    }

    let now = now_secs();
    let last_context_clear_at = if is_context_clear_prompt(&input.prompt) {
        Some(now)
    } else {
        previous_state
            .as_ref()
            .and_then(|state| state.last_context_clear_at)
    };
    let state = SessionState {
        session_id: input.session_id.clone(),
        doc_path: doc_path.display().to_string(),
        last_turn_id: input.turn_id.clone(),
        last_prompt: input.prompt.clone(),
        last_auto_queue_head: None,
        last_context_clear_at,
        updated_at: now,
    };
    for root in roots {
        save_state(&root, &state)?;
    }
    Ok(())
}

fn apply_stop(input: &StopInput) -> Result<StopResponse> {
    let cwd = PathBuf::from(&input.cwd);
    let roots = project_roots_for(&cwd);
    if roots.is_empty() {
        return Ok(StopResponse::Continue { continue_: true });
    }
    let Some((loaded_root, state)) = load_state_any(&roots, &input.session_id)? else {
        // No tracked in-memory session state — but a cleanly-committed document
        // can still owe an `agent:queue auto` continuation. Consult the durable
        // marker before letting Codex send its final answer; this is the live
        // failure mode the marker exists to close.
        // (#codex-auto-queue-stalled-final-gate)
        if let Some(response) = marker_fallback_continuation_response(&roots, input)? {
            return Ok(response);
        }
        return Ok(StopResponse::Continue { continue_: true });
    };

    let file = PathBuf::from(&state.doc_path);
    let cleanup_roots = tracking_roots(&cwd, Some(&file));
    if !file.exists() {
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        return Ok(StopResponse::Continue { continue_: true });
    }

    match crate::session_check::inspect(&file)? {
        crate::session_check::SessionCheckStatus::Ok(_) => {
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
        crate::session_check::SessionCheckStatus::Interrupted(reason) => {
            if is_editor_convergence_required_interruption(&reason) {
                crate::ops_log::log_op(&file, "codex_stop_editor_convergence_required_blocked");
                let display = file.display();
                let message = format!(
                    "agent-doc Stop hook found an editor-convergence blocked closeout for {display}. {reason} Do not send the final answer yet. Retry through the editor/CRDT path after the editor frontend has the required capability or the live editor state is otherwise proven. Do not run `--force-disk` unless the operator explicitly chooses that recovery."
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
                        crate::ops_log::log_op(
                            &file,
                            &format!("codex_stop_auto_close_failed err={err}"),
                        );
                        return Ok(StopResponse::Block {
                            decision: "block",
                            reason: format!(
                                "agent-doc Stop hook intercepted an unfinished document cycle for {}. The hook wrote or recovered the response but could not finish the required commit boundary: {err}. Do not send the final answer yet. Finish the commit boundary for this turn and end with `agent-doc session-check {}`.",
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

fn is_editor_convergence_required_interruption(reason: &str) -> bool {
    reason.contains("closeout blocked by `editor_convergence_required`")
        || (reason.contains("editor_convergence_required")
            && reason.contains("operator_text_authority_v1"))
}

fn committed_prompt_diff_stop_response(file: &Path, reason: &str) -> Result<Option<StopResponse>> {
    if !is_committed_prompt_diff_interruption(reason) {
        return Ok(None);
    }
    let prompt = crate::session_check::unresolved_exchange_prompt(file)?
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
    if let Some(prompt) = crate::session_check::unresolved_exchange_prompt(file)? {
        return Ok(Some(prompt));
    }
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    Ok(first_active_queue_prompt_in_content(&content))
}

fn first_active_queue_prompt_in_content(content: &str) -> Option<String> {
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
    if is_context_clear_prompt(&prompt)
        || agent_doc_queue::queue_command::slash_command_text(&prompt).is_some()
    {
        return None;
    }
    Some(prompt)
}

fn agent_doc_mcp_configured_for(file: &Path) -> bool {
    project_roots_for(file).iter().any(|root| {
        let config_path = root.join(".codex/config.toml");
        let Ok(content) = std::fs::read_to_string(&config_path) else {
            return false;
        };
        let Ok(config) = toml::from_str::<toml::Value>(&content) else {
            return false;
        };
        config
            .get("mcp_servers")
            .and_then(toml::Value::as_table)
            .and_then(|servers| servers.get("agent-doc"))
            .and_then(toml::Value::as_table)
            .map(|server| {
                server.get("command").and_then(toml::Value::as_str) == Some("agent-doc")
                    || server.get("url").and_then(toml::Value::as_str).is_some()
            })
            .unwrap_or(false)
    })
}

fn is_context_clear_prompt(prompt: &str) -> bool {
    agent_doc_queue::queue_command::is_context_clear_command(prompt)
}

/// `#clearcodex`: resolve the Codex Stop-hook continuation context-reset reason
/// AND emit the structured proof lines an operator greps for in ops.log.
///
/// The supervisor idle-queue watch (`start.rs`, `#s760c`) already emits the
/// canonical `[s760] clear-decision …` line before a pre-emptive `/clear`, but
/// the Codex Stop-hook continuation decided its `/clear` instruction with no
/// observable marker — so an operator driving a queue drain could never confirm
/// or deny that a queue-turn boundary actually requested a reset. This helper
/// restores parity: when the project is opted into `agent_doc_queue_context_reset`,
/// it logs the canonical `[s760] clear-decision` line from the Codex session
/// JSONL `token_count` event when available, plus a `[clearcodex]
/// codex-continuation` companion line carrying the effective reset reason.
/// Returns the effective reason so the caller wires it into the continuation
/// instruction.
fn codex_live_context_pct(file: &Path) -> Option<f64> {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    let project_dir = project_roots_for(file)
        .into_iter()
        .next()
        .or_else(|| std::env::current_dir().ok())?;
    let transcript = latest_codex_transcript(Path::new(&home), &project_dir)?;
    transcript_context_pct(Harness::Codex, &transcript, "codex")
}

pub(crate) fn codex_queue_context_reset_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Result<Option<String>> {
    let mut reason = crate::session_accretion::queue_context_reset_reason_if_opted_in(
        file,
        last_context_clear_at,
    )?;
    if crate::session_accretion::queue_context_reset_opted_in(file) {
        let threshold = crate::session_accretion::clear_threshold_for_doc(file);
        let pct = codex_live_context_pct(file);
        let decision = clear_decision(true, pct, threshold);
        if reason.is_none() && decision.clear {
            reason = Some(format!(
                "transcript context {:.1}% >= clear threshold {}% (#clearcodex)",
                pct.unwrap_or_default(),
                threshold
            ));
        }
    }
    Ok(reason)
}

fn codex_continuation_clear_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Option<String> {
    let reason = match codex_queue_context_reset_reason(file, last_context_clear_at) {
        Ok(reason) => reason,
        Err(err) => {
            eprintln!(
                "[agent-doc] codex stop hook: failed to resolve queue context-reset reason for {}: {err:#}",
                file.display()
            );
            None
        }
    };
    if crate::session_accretion::queue_context_reset_opted_in(file) {
        let threshold = crate::session_accretion::clear_threshold_for_doc(file);
        let pct = codex_live_context_pct(file);
        let decision = clear_decision(true, pct, threshold);
        crate::ops_log::log_op(file, &decision.diagnostic);
        crate::ops_log::log_op(
            file,
            &format!(
                "[clearcodex] codex-continuation optIn=true reason={:?} clear_instructed=false background_clear_suppressed={}",
                reason.as_deref().unwrap_or(""),
                reason.is_some()
            ),
        );
    }
    reason
}

fn log_codex_stop_queue_continuation(file: &Path, prompt: &str, source: &str) {
    crate::ops_log::log_op(
        file,
        &format!(
            "codex_stop_queue_continuation file={} source={} mcp_configured={} prompt_bytes={} prompt_sha256={}",
            file.display(),
            source,
            agent_doc_mcp_configured_for(file),
            prompt.len(),
            agent_doc_hash::content_hash(prompt),
        ),
    );
}

fn log_codex_background_context_clear_suppressed(
    file: &Path,
    prompt: &str,
    source: &str,
    reason: &str,
) {
    crate::ops_log::log_op(
        file,
        &format!(
            "codex_background_context_clear_suppressed file={} source={} result=in_pane_continuation prompt_bytes={} prompt_sha256={} reason={:?}",
            file.display(),
            source,
            prompt.len(),
            agent_doc_hash::content_hash(prompt),
            reason,
        ),
    );
}

fn background_context_clear_suppression_response(
    file: &Path,
    prompt: &str,
    source: &str,
    context_reset_reason: Option<&str>,
) -> Option<StopResponse> {
    let reason = context_reset_reason?;
    log_codex_background_context_clear_suppressed(file, prompt, source, reason);
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
    queue_completion_ids: &[String],
) -> Result<Option<crate::write::QueueConsumptionOutcome>> {
    let force_disk_without_listener = file
        .canonicalize()
        .ok()
        .map(|canonical| {
            let project_root = crate::write::resolve_ipc_project_root_pub(&canonical);
            !crate::ipc_socket::is_listener_active(&project_root)
        })
        .unwrap_or(false);
    crate::write::consume_queue_prompts_with_outcome(
        file,
        queue_completion_ids,
        force_disk_without_listener,
    )
}

fn repeated_queue_response_for_write(
    file: &Path,
    prompt: &str,
    response: &str,
) -> Result<std::result::Result<String, String>> {
    if crate::write::response_explicitly_targets_active_queue_head(file, response)? {
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
    let content_before_repair = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
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

    crate::repair::save_pending(file, &response_to_write)?;
    crate::ops_log::log_op(file, "codex_stop_repeated_queue_response_saved");
    let mut note = format!(
        " The hook replayed the last assistant response into `agent:exchange` for repeated queue head {:?}.",
        prompt
    );

    let repair_outcome = crate::repair::run_with_queue_completion_ids(file, &queue_completion_ids)?;
    if repair_outcome.replayed_response() {
        note.push_str(" The response was written through the normal repair/write path.");
    } else if repair_outcome == crate::repair::RepairOutcome::AlreadyApplied {
        note.push_str(" The response was already present and was adopted by repair.");
    } else {
        return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
            note: format!(
                "{note} The repair path did not replay or adopt the response (outcome: {repair_outcome:?})."
            ),
        });
    }

    if active_auto_queue_prompt(file)?.as_deref() == Some(prompt) {
        match consume_recovered_queue_head(file, &queue_completion_ids) {
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

    if !crate::git::is_in_git_repo(file) {
        return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
            note: format!(
                "{note} The document is not in a git repository, so the hook could not finish the required commit boundary automatically."
            ),
        });
    }

    match crate::write::complete_required_closeout(file) {
        Ok(true) => {
            note.push_str(" The hook finished the commit boundary automatically.");
        }
        Ok(false) => {}
        Err(err) => {
            crate::ops_log::log_op(
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

    crate::ops_log::log_op(file, "codex_stop_repeated_queue_recovery_success");
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
                agent_doc_mcp_configured_for(file),
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
                    agent_doc_mcp_configured_for(file),
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
    let context_reset_reason = codex_continuation_clear_reason(file, state.last_context_clear_at);
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
                        agent_doc_mcp_configured_for(file),
                        context_reset_reason.as_deref(),
                    )
                }
            },
        ),
    })
}

fn marker_repeated_queue_recovery_response(
    file: &Path,
    previous_prompt: &str,
    note: String,
) -> Result<StopResponse> {
    let Some(next_prompt) = active_auto_queue_prompt(file)? else {
        return Ok(StopResponse::Continue { continue_: true });
    };
    if next_prompt == previous_prompt {
        return Ok(StopResponse::Block {
            decision: "block",
            reason: format!(
                "agent-doc Stop hook replayed a response for {} from the durable continuation marker, but the queue head still did not advance: {:?}.{} {}",
                file.display(),
                previous_prompt,
                note,
                render_prompt_continuation_instruction(
                    &file.display().to_string(),
                    agent_doc_mcp_configured_for(file),
                    None,
                )
            ),
        });
    }
    agent_doc_queue_io::continuation_marker::record_continuation_requested_head(
        file,
        &next_prompt,
    )?;
    let context_reset_reason = codex_continuation_clear_reason(file, None);
    if let Some(response) = background_context_clear_suppression_response(
        file,
        &next_prompt,
        "durable_marker_after_recovery",
        context_reset_reason.as_deref(),
    ) {
        return Ok(response);
    }
    Ok(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook recovered the previous queue response for {disp} from the durable continuation marker.{note} The next queue prompt is {prompt:?}. {instruction}",
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
                        agent_doc_mcp_configured_for(file),
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
    if is_context_clear_prompt(&prompt) {
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
    // Keep the durable marker's requested-head in sync so a later stop with
    // missing session state still applies the non-advancing-head guard.
    let _ =
        agent_doc_queue_io::continuation_marker::record_continuation_requested_head(file, &prompt);
    let context_reset_reason = codex_continuation_clear_reason(file, state.last_context_clear_at);
    log_codex_stop_queue_continuation(file, &prompt, "tracked_state");
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
                        agent_doc_mcp_configured_for(file),
                        context_reset_reason.as_deref(),
                    )
                }
            },
        ),
    }))
}

/// Codex Stop-hook fallback when no tracked in-memory session state exists: a
/// durable continuation marker (written at the last clean closeout) may still
/// prove the document owes an `agent:queue auto` continuation. The marker is
/// re-confirmed against the live document inside
/// [`crate::queue_continuation::pending_marker_continuation_for_roots`], so a
/// stale marker never forces a spurious block. (#codex-auto-queue-stalled-final-gate)
fn marker_fallback_continuation_response(
    roots: &[PathBuf],
    input: &StopInput,
) -> Result<Option<StopResponse>> {
    // `#codex-stop-cross-doc-queue-continuation`: this fallback has no tracked
    // in-memory session state, so it must not blindly drive the first durable
    // marker — pass the current Codex pane (inherited via TMUX_PANE) so a marker
    // owned by another live actor's pane is skipped instead of forcing this pane
    // to run a foreign-owned document.
    let current_pane = std::env::var("TMUX_PANE").ok();
    let Some((file, continuation, marker)) =
        crate::queue_continuation::pending_marker_continuation_for_roots(
            roots,
            current_pane.as_deref(),
        )?
    else {
        return Ok(None);
    };

    if is_context_clear_prompt(&continuation.head_prompt) {
        return Ok(None);
    }

    // Non-advancing-head guard: a repeated stop whose marker already requested
    // this exact head must either recover the in-pane response or fail closed
    // without letting Codex send an unpersisted final answer.
    if input.stop_hook_active
        && marker.last_requested_head.as_deref() == Some(continuation.head_prompt.as_str())
    {
        return Ok(Some(
            match try_recover_repeated_queue_head_response(
                &file,
                &continuation.head_prompt,
                input,
                None,
            )? {
                RepeatedQueueHeadRecovery::Recovered { note } => {
                    marker_repeated_queue_recovery_response(&file, &continuation.head_prompt, note)?
                }
                RepeatedQueueHeadRecovery::NotRecoverable { note } => {
                    repeated_queue_recovery_unavailable_response(
                        &file,
                        &continuation.head_prompt,
                        &note,
                    )
                }
            },
        ));
    }

    agent_doc_queue_io::continuation_marker::record_continuation_requested_head(
        &file,
        &continuation.head_prompt,
    )?;
    let context_reset_reason = codex_continuation_clear_reason(&file, None);
    log_codex_stop_queue_continuation(&file, &continuation.head_prompt, "durable_marker");
    if let Some(response) = background_context_clear_suppression_response(
        &file,
        &continuation.head_prompt,
        "durable_marker",
        context_reset_reason.as_deref(),
    ) {
        return Ok(Some(response));
    }
    // #codex-self-reinvoke-prevent (Option B): in-pane continuation, not a CLI
    // re-run (see auto_queue_continuation_response).
    Ok(Some(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook found a durable `agent:queue auto` continuation for {disp} with no tracked session state. The next queue prompt is {prompt:?}. {instruction}",
            disp = file.display(),
            prompt = continuation.head_prompt.as_str(),
            instruction = {
                let display_path = file.display().to_string();
                if let Some(command) =
                    agent_doc_queue::queue_command::slash_command_text(&continuation.head_prompt)
                {
                    render_slash_command_continuation_instruction(&display_path, &command)
                } else {
                    render_prompt_continuation_instruction(
                        &display_path,
                        agent_doc_mcp_configured_for(&file),
                        context_reset_reason.as_deref(),
                    )
                }
            },
        ),
    }))
}

fn attempt_stop_closeout(
    file: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<StopCloseAttempt> {
    let payload =
        agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let has_response = matches!(
        payload,
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(_)
    );
    let has_bypassed_patchback =
        crate::session_check::detect_bypassed_response_write(file)?.is_some();
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

    let queue_synthetic_cycle =
        active_auto_queue_prompt(file)?.is_some() && open_cycle_started_from_unchanged_file(file)?;
    let captured_response_targets_queue_head = if queue_synthetic_cycle {
        match &payload {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                crate::write::response_explicitly_targets_active_queue_head(
                    file,
                    response.as_ref(),
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
                let content_before_repair = std::fs::read_to_string(file)
                    .with_context(|| format!("failed to read {}", file.display()))?;
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

    let mut note = String::new();
    match payload {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            crate::repair::save_pending(file, response.as_ref())?;
            crate::ops_log::log_op(file, "codex_stop_capture_saved");
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

    let repair_outcome = crate::repair::run_with_queue_completion_ids(file, &queue_completion_ids)?;
    if repair_outcome.replayed_response() {
        note.push_str(" The hook replayed the response through the normal write path.");
    } else if repair_outcome.repaired() {
        note.push_str(" The hook repaired the pending closeout state before auto-close.");
    }
    let queue_repair_explicitly_closes_head = queue_synthetic_cycle
        && repair_outcome.replayed_response()
        && captured_response_targets_queue_head;
    if queue_repair_explicitly_closes_head {
        match consume_recovered_queue_head(file, &queue_completion_ids) {
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

    if !crate::git::is_in_git_repo(file) {
        note.push_str(
            " The document is not in a git repository, so the hook could not finish the required commit boundary automatically.",
        );
        return Ok(StopCloseAttempt::StillOpen { note });
    }

    match crate::write::complete_required_closeout(file) {
        Ok(true) => {
            note.push_str(" The hook finished the commit boundary automatically.");
        }
        Ok(false) => {}
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!("codex_stop_auto_close_closeout_failed err={err}"),
            );
            note.push_str(&format!(
                " The hook wrote or recovered the response but could not finish the required commit boundary: {err}."
            ));
            return Ok(StopCloseAttempt::StillOpen { note });
        }
    }
    crate::ops_log::log_op(file, "codex_stop_auto_close_success");
    Ok(StopCloseAttempt::Closed)
}

fn capture_assistant_text(file: &Path, state: &SessionState, input: &StopInput) -> String {
    match agent_doc_template::replay_guard::classify_replay_payload(&input.last_assistant_message) {
        agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
            capture_missing_stop_response(file, Some(state.last_prompt.as_str()))
        }
        agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            match crate::repair::save_pending(file, response.as_ref()) {
                Ok(()) => {
                    crate::ops_log::log_op(file, "codex_stop_capture_saved");
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
    match save_blocked_stop_payload(
        file,
        "",
        reason,
        "missing_last_assistant_message",
        last_prompt,
    ) {
        Ok(path) => {
            crate::ops_log::log_op(
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
    match save_blocked_stop_payload(file, payload, reason, "blocked_replay_payload", last_prompt) {
        Ok(path) => {
            crate::ops_log::log_op(
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

fn save_blocked_stop_payload(
    file: &Path,
    payload: &str,
    reason: &str,
    kind: &str,
    last_prompt: Option<&str>,
) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked stop payload")?;
    let dir = root.join(".agent-doc/codex-hooks/blocked-stop");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked-stop dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedStopPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        kind,
        reason,
        payload_sha256: agent_doc_hash::content_hash(payload),
        last_assistant_message: (!payload.trim().is_empty()).then_some(payload),
        last_prompt: last_prompt.filter(|prompt| !prompt.trim().is_empty()),
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked stop payload {}", path.display()))?;
    Ok(path)
}

fn read_stdin_payload() -> Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("read hook payload from stdin")?;
    Ok(payload)
}

fn resolve_agent_doc_path(prompt: &str, cwd: &Path) -> Option<PathBuf> {
    let file =
        agent_doc_prompt_contract::harness_prompt::agent_doc_invocation_file_from_text(prompt)?;
    let path = PathBuf::from(file);
    let joined = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    Some(joined.canonicalize().unwrap_or(joined))
}

#[cfg(test)]
fn project_root_for(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    agent_doc_fs::find_project_root(&canonical)
}

fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(nearest_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Vec::new();
    };
    let mut roots = vec![nearest_root.clone()];
    let Some(git_root) = find_git_root(&canonical) else {
        return roots;
    };
    if !nearest_root.starts_with(&git_root) {
        return roots;
    }
    if git_root.join(".agent-doc").is_dir() {
        push_unique_root(&mut roots, git_root);
    }
    roots
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn tracking_roots(cwd: &Path, doc_path: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = project_roots_for(cwd);
    if let Some(doc_path) = doc_path {
        for root in project_roots_for(doc_path) {
            push_unique_root(&mut roots, root);
        }
    }
    roots
}

fn push_unique_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn load_state_any(roots: &[PathBuf], session_id: &str) -> Result<Option<(PathBuf, SessionState)>> {
    for root in roots {
        if let Some(state) = load_state(root, session_id)? {
            return Ok(Some((root.clone(), state)));
        }
    }
    Ok(None)
}

fn clear_state_across_roots(roots: &[PathBuf], loaded_root: &Path, session_id: &str) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        clear_state(&root, session_id)?;
    }
    Ok(())
}

fn save_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        save_state(&root, state)?;
    }
    Ok(())
}

fn load_state(root: &Path, session_id: &str) -> Result<Option<SessionState>> {
    let path = state_path(root, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let state =
        serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(state))
}

fn active_auto_queue_prompt(file: &Path) -> Result<Option<String>> {
    // Single source of truth: the shared queue-continuation detector
    // (#codex-auto-queue-stalled-final-gate). Keeps the Stop-hook continuation
    // decision identical to the durable marker and `session-check` gate.
    Ok(crate::queue_continuation::detect(file)?.map(|continuation| continuation.head_prompt))
}

fn open_cycle_started_from_unchanged_file(file: &Path) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
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

pub fn load_prompt_for_current_session(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_for_file(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_latest_state_for_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    load_latest_state_for_file(file)
}

pub fn prompt_requests_clear(prompt: &str) -> bool {
    is_context_clear_prompt(prompt)
}

pub fn record_external_prompt_for_file(file: &Path, session_id: &str, prompt: &str) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = SessionState {
        session_id: session_id.to_string(),
        doc_path: canonical.display().to_string(),
        last_turn_id: String::new(),
        last_prompt: prompt.to_string(),
        last_auto_queue_head: None,
        last_context_clear_at: is_context_clear_prompt(prompt).then(now_secs),
        updated_at: now_secs(),
    };
    for root in project_roots_for(&canonical) {
        save_state(&root, &state)?;
    }
    Ok(())
}

pub fn load_active_session_for_current_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let Some(session_id) = current_session_id() else {
        return Ok(None);
    };
    let roots = project_roots_for(file);
    let Some((_, state)) = load_state_any(&roots, &session_id)? else {
        return Ok(None);
    };
    let state_file = PathBuf::from(&state.doc_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if state_file != current_file {
        return Ok(None);
    }
    Ok(Some(ActiveSessionState {
        session_id: state.session_id,
        doc_path: state.doc_path,
        last_turn_id: state.last_turn_id,
        last_prompt: state.last_prompt,
        updated_at: state.updated_at,
    }))
}

fn load_latest_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut latest: Option<SessionState> = None;

    for root in project_roots_for(file) {
        let dir = root.join(".agent-doc/codex-hooks/sessions");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<SessionState>(&content) else {
                continue;
            };
            let state_file = PathBuf::from(&state.doc_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
            if state_file != current_file {
                continue;
            }
            let is_newer = latest
                .as_ref()
                .is_none_or(|best| state.updated_at >= best.updated_at);
            if is_newer {
                latest = Some(state);
            }
        }
    }

    Ok(latest.map(|state| ActiveSessionState {
        session_id: state.session_id,
        doc_path: state.doc_path,
        last_turn_id: state.last_turn_id,
        last_prompt: state.last_prompt,
        updated_at: state.updated_at,
    }))
}

fn save_state(root: &Path, state: &SessionState) -> Result<()> {
    let path = state_path(root, &state.session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn clear_state(root: &Path, session_id: &str) -> Result<()> {
    let path = state_path(root, session_id);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("remove {}", path.display())),
    }
    Ok(())
}

fn state_path(root: &Path, session_id: &str) -> PathBuf {
    let hash = agent_doc_hash::content_hash(session_id);
    root.join(".agent-doc/codex-hooks/sessions")
        .join(format!("{hash}.json"))
}

fn current_session_id() -> Option<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("CODEX_SESSION_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as ProcessCommand;

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
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn write_codex_mcp_config(root: &Path) {
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            format!(
                "[mcp_servers.agent-doc]\ncommand = \"agent-doc\"\ndefault_tools_approval_mode = \"approve\"\nargs = [\"mcp\", \"serve\", \"--project-root\", \"{}\"]\n",
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
        crate::snapshot::save(&doc, content).unwrap();
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
        crate::snapshot::save(&doc, content).unwrap();
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
        crate::snapshot::save(&doc, &content).unwrap();
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
        crate::snapshot::save(&doc, &content).unwrap();
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
        crate::snapshot::save(&doc, content).unwrap();
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
    fn user_prompt_submit_tracks_agent_doc_file() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        track_doc(&dir, &doc, "turn-1");

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(state.doc_path), doc);
        assert_eq!(state.last_turn_id, "turn-1");
        assert_eq!(state.last_prompt, format!("agent-doc {}", doc.display()));
    }

    #[test]
    fn user_prompt_submit_does_not_track_ambient_ancestor_root() {
        let ambient = tempfile::tempdir().unwrap();
        fs::create_dir_all(ambient.path().join(".agent-doc")).unwrap();
        let project = ambient.path().join("project");
        fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();
        let doc = project.join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: project.display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();

        let project_state = load_state(&project, "codex-session").unwrap();
        let ambient_state = load_state(ambient.path(), "codex-session").unwrap();
        assert!(
            project_state.is_some(),
            "nearest project root should receive Codex hook state"
        );
        assert!(
            ambient_state.is_none(),
            "ambient ancestor .agent-doc roots must not receive shared hook state"
        );
    }

    #[test]
    fn user_prompt_submit_tracks_same_line_agent_doc_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {} #code-review", doc.display()),
        })
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(&state.doc_path), doc);
        assert_eq!(
            state.last_prompt,
            format!("agent-doc {} #code-review", doc.display())
        );

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = agent_doc_harness::prompt_source::prompt_body_for_file(
            &doc,
            load_prompt_for_current_session,
        )
        .unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some("#code-review".to_string()));
    }

    #[test]
    fn resolve_agent_doc_path_prefers_real_invocation_after_instruction_preamble() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!(
            "# AGENTS.md instructions for {}\n\
\n\
```\n\
agent-doc <FILE>\n\
agent-doc compact <FILE>\n\
```\n\
\n\
Use the harness-native entrypoint below.\n\
\n\
agent-doc {}\n",
            dir.path().display(),
            doc.display()
        );

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn resolve_agent_doc_path_accepts_session_invocation_with_trailing_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!("agent-doc {} #agent-doc-bug", doc.display());

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn load_prompt_for_current_session_uses_codex_thread_id() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        track_doc(&dir, &doc, "turn-1");

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = load_prompt_for_current_session(&doc).unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some(format!("agent-doc {}", doc.display())));
    }

    #[test]
    fn load_latest_prompt_for_file_picks_most_recent_matching_state() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-old".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                updated_at: 10,
            },
        )
        .unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-new".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-2".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn load_latest_prompt_for_file_skips_malformed_state_entries() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();
        let state_dir = root.join(".agent-doc/codex-hooks/sessions");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("bad.json"), "{").unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-good".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn prompt_requests_clear_matches_only_exact_builtin() {
        assert!(prompt_requests_clear("/clear"));
        assert!(prompt_requests_clear("  /clear  "));
        assert!(prompt_requests_clear("/new"));
        assert!(!prompt_requests_clear("agent-doc tasks/foo.md"));
        assert!(!prompt_requests_clear("/clear please"));
    }

    #[test]
    fn stop_auto_closes_open_cycle_from_last_assistant_message() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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

        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(
            !pending.exists(),
            "pending capture should be cleared after recovery"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Final assistant response."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_abandoned(
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
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(
            fs::read_to_string(&pending)
                .unwrap()
                .contains("counting characters"),
            "the hook should capture the replayable answer for recovery"
        );
    }

    #[test]
    fn stop_blocks_consecutive_direct_chat_manual_queue_answers() {
        let dir = setup_project();
        let doc = write_manual_queue_doc(&dir, &["Remove the max character count cap"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_abandoned(
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
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        let pending_body = fs::read_to_string(&pending).unwrap();
        assert!(pending_body.contains("Second direct-chat answer."));
        assert!(!pending_body.contains("First direct-chat answer."));
    }

    #[test]
    fn stop_blocks_when_parent_submodule_pointer_closeout_fails() {
        let parent_dir = tempfile::tempdir().unwrap();
        let sub_src_dir = tempfile::tempdir().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

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
        crate::snapshot::save(&doc, original).unwrap();
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
                crate::git::submodule_pointer_drift(&doc).unwrap().is_some(),
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
        crate::snapshot::save(&doc, original).unwrap();
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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
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
        assert!(content.contains("[#bpcontract] Write the contract first."));
        assert!(
            !content.contains("Reviewing the current plan and repo conventions"),
            "leading commentary should be stripped from the replayed closeout"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
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
        assert!(content.contains("[#bpcontract] Write the contract first."));
        assert!(content.contains("### 2. Later"));
        let capture = crate::capture::latest_committed(&doc)
            .unwrap()
            .expect("committed capture should exist");
        assert!(
            !capture.response_body.contains("<!-- patch:backlog -->"),
            "captured response should be stripped of backlog patches after normalization"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_active_session_post_commit_drift() {
        let dir = setup_project();
        let doc = write_template_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let drifted = format!("{original}\nPost-closeout active-session drift.\n");
        fs::write(&doc, &drifted).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();

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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(
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

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
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
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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

        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(
            !pending.exists(),
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some(&original), Some(&original))
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
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(!pending.exists());
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
        crate::snapshot::save(&doc, content).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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
    fn stop_blocks_from_durable_marker_when_session_state_missing() {
        // #codex-auto-queue-stalled-final-gate live regression (sampleorders
        // shape): the completed head was consumed, `#seopdp` remains, queue_active
        // is true with `agent:queue auto`, and the document is clean — but the
        // Stop hook has NO tracked in-memory session state (the live failure). The
        // durable continuation marker (written at the prior clean closeout) must
        // still force continuation instead of letting Codex send a final answer.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        // Prior clean closeout wrote the durable marker.
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        // Untracked session id → load_state_any returns None.
        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("durable"), "{reason}");
                assert!(
                    reason.contains("do [#seopdp] deploy product page"),
                    "{reason}"
                );
                assert!(reason.contains("send the final answer"), "{reason}");
                assert!(!reason.contains("agent_doc_finalize"), "{reason}");
            }
            other => panic!("expected durable-marker continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation")
                && ops_log.contains("source=durable_marker")
                && ops_log.contains("mcp_configured=false")
                && ops_log.contains(&agent_doc_hash::content_hash(
                    "do [#seopdp] deploy product page"
                )),
            "Stop hook should log durable-marker queue-continuation proof:\n{ops_log}"
        );
    }

    #[test]
    fn stop_passes_through_context_clear_from_durable_marker_when_session_state_missing() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/clear"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

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
    fn stop_marker_fallback_suppresses_background_clear_after_exchange_compaction() {
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
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
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
                && ops_log.contains("source=durable_marker")
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
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();
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
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
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
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
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
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
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
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();
        let compaction_ts = crate::session_accretion::recent_exchange_compaction_timestamp(&doc)
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
    fn stop_marker_fallback_continuation_prefers_configured_mcp_tools() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("durable"), "{reason}");
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
            other => panic!("expected durable-marker continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_marker_fallback_replays_plain_final_answer_when_head_does_not_advance() {
        // #codex-auto-queue-stalled-final-gate: a repeated stop (stop_hook_active)
        // whose durable marker already requested this exact head must persist a
        // plain Codex final answer into agent:exchange instead of allowing it to
        // escape as chat-only text.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").unwrap();
        // The first continuation request recorded this head into the marker.
        agent_doc_queue_io::continuation_marker::record_continuation_requested_head(
            &doc,
            "do [#seopdp] deploy",
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.\n\nVerification: Codex stop-hook simulation."
                .to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: do [#seopdp] deploy — gpt-5"));
        assert!(content.contains("Verification: Codex stop-hook simulation."));
        assert!(!content.contains("- do [#seopdp] deploy"));
        assert!(
            crate::queue_continuation::detect(&doc).unwrap().is_none(),
            "replayed response should drain the only active head"
        );
    }

    #[test]
    fn stop_repair_preserves_auto_queue_when_response_targets_other_prompt() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some(&original), Some(&original))
            .unwrap();
        fs::write(
            &doc,
            format!(
                "{original}\n❯ do #repair-false-closeouts. #spec-test-build-install-commit-push\n"
            ),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
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
