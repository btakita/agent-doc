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
            clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
            Ok(StopResponse::Continue { continue_: true })
        }
        crate::session_check::SessionCheckStatus::Interrupted(reason) => {
            if !input.stop_hook_active {
                match attempt_stop_closeout(&file, &state, input)? {
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

fn is_committed_prompt_diff_interruption(reason: &str) -> bool {
    reason.contains("is `committed`")
        && reason.contains("prompt_target:")
        && (reason.contains("unresolved prompt-bearing user changes")
            || reason.contains(
                "active harness session changed this document after the last committed closeout",
            ))
        && (reason.contains("no new agent-doc cycle started")
            || reason.contains("without reopening the binary-owned write/commit path"))
}

fn prompt_target_from_interruption_reason(reason: &str) -> Option<String> {
    let marker = "prompt_target:";
    let tail = reason.split_once(marker)?.1.trim();
    (!tail.is_empty()).then(|| tail.to_string())
}

fn first_nonempty_prompt_line(prompt: &str) -> String {
    prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(prompt)
        .trim()
        .to_string()
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
    crate::queue_command::is_context_clear_command(prompt)
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
/// it logs the canonical `[s760] clear-decision` line (Codex transcript token
/// reading is unsupported — `read_used_tokens` → None for Codex — so the ctx%
/// gate is always `pct=none clear=false` and never drives the clear by itself)
/// plus a `[clearcodex] codex-continuation` companion line carrying the
/// accretion/compaction reason that actually instructs the `/clear`. Returns the
/// effective reason unchanged so the caller still wires it into the continuation
/// instruction.
fn codex_continuation_clear_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Option<String> {
    let reason = match crate::session_accretion::queue_context_reset_reason_if_opted_in(
        file,
        last_context_clear_at,
    ) {
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
        // Codex transcript token reading is unsupported, so ctx% is unknown and
        // the s760 gate never clears on its own; the accretion/compaction
        // `reason` is the live signal that instructs the `/clear`.
        let decision = crate::context_pct::clear_decision(true, None, threshold);
        crate::ops_log::log_op(file, &decision.diagnostic);
        crate::ops_log::log_op(
            file,
            &format!(
                "[clearcodex] codex-continuation optIn=true reason={:?} clear_instructed={}",
                reason.as_deref().unwrap_or(""),
                reason.is_some()
            ),
        );
    }
    reason
}

fn continuation_closeout_instruction(file: &Path, context_reset_reason: Option<&str>) -> String {
    if let Some(reason) = context_reset_reason {
        return format!(
            "Do NOT answer the next queue prompt in this already-loaded Codex context. Fresh context is required because {reason}. Run `/clear` before continuing, then re-invoke `agent-doc {disp}` from the fresh context so the next queue head runs after the reset. Do not send the final answer yet.",
            disp = file.display()
        );
    }

    if agent_doc_mcp_configured_for(file) {
        format!(
            "Continue THIS turn in-pane via the configured `agent-doc` MCP server: call `agent_doc_preflight` for {disp}, use `agent_doc_plan` / `agent_doc_read` as needed, answer that prompt in the response passed to `agent_doc_finalize`, and verify the result with `agent_doc_session_check`. If the MCP tools are unavailable in this Codex run, answer that prompt in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`). Do NOT run `agent-doc {disp}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet.",
            disp = file.display()
        )
    } else {
        format!(
            "Continue THIS turn in-pane: answer that prompt in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`). Do NOT run `agent-doc {disp}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet.",
            disp = file.display()
        )
    }
}

fn slash_command_continuation_instruction(file: &Path, command: &str) -> String {
    format!(
        "Do NOT answer the queued slash command {command:?} as an agent-doc prompt. Let the current turn close so the managed owner-pane supervisor can submit {command:?} at the next idle prompt, mark that queue head complete, and continue the remaining queue. Do not send the final answer yet. If no managed supervisor is available, submit {command:?} in the owner pane, then run `agent-doc queue consume {disp}` and `agent-doc commit {disp}` before continuing.",
        command = command,
        disp = file.display()
    )
}

fn continuation_closeout_instruction_for_head(
    file: &Path,
    head: &str,
    context_reset_reason: Option<&str>,
) -> String {
    if let Some(command) = crate::queue_command::slash_command_text(head) {
        slash_command_continuation_instruction(file, &command)
    } else {
        continuation_closeout_instruction(file, context_reset_reason)
    }
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
            crate::ops_log::content_hash(prompt),
        ),
    );
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
    patch.push_str(response.trim());
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str("<!-- /patch:exchange -->\n");
    patch
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
    let payload = crate::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let response = match payload {
        crate::replay_guard::ReplayPayloadClassification::Empty => {
            return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                note: capture_missing_stop_response(file, last_prompt),
            });
        }
        crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            return Ok(RepeatedQueueHeadRecovery::NotRecoverable {
                note: capture_blocked_stop_payload(
                    file,
                    &input.last_assistant_message,
                    &reason,
                    last_prompt,
                ),
            });
        }
        crate::replay_guard::ReplayPayloadClassification::Replayable(response) => response,
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

    crate::repair::save_pending(file, &response_to_write)?;
    crate::ops_log::log_op(file, "codex_stop_repeated_queue_response_saved");
    let mut note = format!(
        " The hook replayed the last assistant response into `agent:exchange` for repeated queue head {:?}.",
        prompt
    );

    let repair_outcome = crate::repair::run(file)?;
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
        match crate::write::consume_queue_prompt_with_outcome(file) {
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
            continuation_closeout_instruction(file, None)
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
                continuation_closeout_instruction(file, None)
            ),
        });
    }

    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(next_prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    let _ = crate::queue_continuation::record_requested_head(file, &next_prompt);
    let context_reset_reason = codex_continuation_clear_reason(file, state.last_context_clear_at);
    Ok(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook recovered the previous queue response for {disp}.{note} The next queue prompt is {prompt:?}. {instruction}",
            disp = file.display(),
            note = note,
            prompt = next_prompt,
            instruction = continuation_closeout_instruction_for_head(
                file,
                &next_prompt,
                context_reset_reason.as_deref()
            ),
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
                continuation_closeout_instruction(file, None)
            ),
        });
    }
    crate::queue_continuation::record_requested_head(file, &next_prompt)?;
    let context_reset_reason = codex_continuation_clear_reason(file, None);
    Ok(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook recovered the previous queue response for {disp} from the durable continuation marker.{note} The next queue prompt is {prompt:?}. {instruction}",
            disp = file.display(),
            note = note,
            prompt = next_prompt,
            instruction = continuation_closeout_instruction_for_head(
                file,
                &next_prompt,
                context_reset_reason.as_deref()
            ),
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
    let _ = crate::queue_continuation::record_requested_head(file, &prompt);
    let context_reset_reason = codex_continuation_clear_reason(file, state.last_context_clear_at);
    log_codex_stop_queue_continuation(file, &prompt, "tracked_state");
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
            instruction = continuation_closeout_instruction_for_head(
                file,
                &prompt,
                context_reset_reason.as_deref()
            ),
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

    crate::queue_continuation::record_requested_head(&file, &continuation.head_prompt)?;
    let context_reset_reason = codex_continuation_clear_reason(&file, None);
    log_codex_stop_queue_continuation(&file, &continuation.head_prompt, "durable_marker");
    // #codex-self-reinvoke-prevent (Option B): in-pane continuation, not a CLI
    // re-run (see auto_queue_continuation_response).
    Ok(Some(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook found a durable `agent:queue auto` continuation for {disp} with no tracked session state. The next queue prompt is {prompt:?}. {instruction}",
            disp = file.display(),
            prompt = continuation.head_prompt.as_str(),
            instruction = continuation_closeout_instruction_for_head(
                &file,
                &continuation.head_prompt,
                context_reset_reason.as_deref()
            ),
        ),
    }))
}

fn attempt_stop_closeout(
    file: &Path,
    state: &SessionState,
    input: &StopInput,
) -> Result<StopCloseAttempt> {
    let payload = crate::replay_guard::classify_replay_payload(&input.last_assistant_message);
    let has_response = matches!(
        payload,
        crate::replay_guard::ReplayPayloadClassification::Replayable(_)
    );
    let has_bypassed_patchback =
        crate::session_check::detect_bypassed_response_write(file)?.is_some();
    if !has_response && !has_bypassed_patchback {
        return Ok(match payload {
            crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
                StopCloseAttempt::StillOpen {
                    note: capture_blocked_stop_payload(
                        file,
                        &input.last_assistant_message,
                        &reason,
                        Some(state.last_prompt.as_str()),
                    ),
                }
            }
            crate::replay_guard::ReplayPayloadClassification::Empty => {
                StopCloseAttempt::StillOpen {
                    note: capture_missing_stop_response(file, Some(state.last_prompt.as_str())),
                }
            }
            crate::replay_guard::ReplayPayloadClassification::Replayable(_) => {
                StopCloseAttempt::NotPossible
            }
        });
    }

    let queue_synthetic_cycle =
        active_auto_queue_prompt(file)?.is_some() && open_cycle_started_from_unchanged_file(file)?;
    let captured_response_targets_queue_head = if queue_synthetic_cycle {
        match &payload {
            crate::replay_guard::ReplayPayloadClassification::Replayable(response) => {
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

    let mut note = String::new();
    match payload {
        crate::replay_guard::ReplayPayloadClassification::Replayable(response) => {
            crate::repair::save_pending(file, response.as_ref())?;
            crate::ops_log::log_op(file, "codex_stop_capture_saved");
            note.push_str(
                " The latest assistant text was captured into the pending/capture ledger before auto-close.",
            );
        }
        crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            note.push_str(&capture_blocked_stop_payload(
                file,
                &input.last_assistant_message,
                &reason,
                Some(state.last_prompt.as_str()),
            ));
        }
        crate::replay_guard::ReplayPayloadClassification::Empty => {}
    }

    let repair_outcome = crate::repair::run(file)?;
    if repair_outcome.replayed_response() {
        note.push_str(" The hook replayed the response through the normal write path.");
    } else if repair_outcome.repaired() {
        note.push_str(" The hook repaired the pending closeout state before auto-close.");
    }
    let queue_repair_explicitly_closes_head = queue_synthetic_cycle
        && repair_outcome.replayed_response()
        && captured_response_targets_queue_head;
    if queue_repair_explicitly_closes_head {
        match crate::write::consume_queue_prompt_with_outcome(file) {
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
    match crate::replay_guard::classify_replay_payload(&input.last_assistant_message) {
        crate::replay_guard::ReplayPayloadClassification::Empty => {
            capture_missing_stop_response(file, Some(state.last_prompt.as_str()))
        }
        crate::replay_guard::ReplayPayloadClassification::Replayable(response) => {
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
        crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
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
    let root = crate::snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked stop payload")?;
    let dir = root.join(".agent-doc/codex-hooks/blocked-stop");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked-stop dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        crate::ops_log::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedStopPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        kind,
        reason,
        payload_sha256: crate::ops_log::content_hash(payload),
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
    let mut inside_code_fence = false;
    for raw_line in prompt.lines().rev() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            inside_code_fence = !inside_code_fence;
            continue;
        }
        if inside_code_fence || line.is_empty() {
            continue;
        }
        let Some(file) = parse_agent_doc_invocation_line(line) else {
            continue;
        };
        if file.starts_with('<') && file.ends_with('>') {
            continue;
        }
        let path = PathBuf::from(file);
        let joined = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        return Some(joined.canonicalize().unwrap_or(joined));
    }
    None
}

fn parse_agent_doc_invocation_line(line: &str) -> Option<&str> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.as_slice() {
        ["agent-doc", "claim", file, ..] | ["/agent-doc", "claim", file, ..] => Some(*file),
        ["agent-doc", "compact", "exchange", file, ..]
        | ["/agent-doc", "compact", "exchange", file, ..] => Some(*file),
        ["agent-doc", "compact", file, ..] | ["/agent-doc", "compact", file, ..] => Some(*file),
        ["agent-doc", file, ..] | ["/agent-doc", file, ..] => Some(*file),
        _ => None,
    }
}

#[cfg(test)]
fn project_root_for(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    crate::snapshot::find_project_root(&canonical)
}

fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(nearest_root) = crate::snapshot::find_project_root(&canonical) else {
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
    let hash = crate::ops_log::content_hash(session_id);
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
mod tests;
