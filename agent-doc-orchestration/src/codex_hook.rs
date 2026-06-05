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
    let doc_path = resolve_agent_doc_path(&input.prompt, &cwd).or_else(|| {
        load_state_any(&project_roots_for(&cwd), &input.session_id)
            .ok()
            .flatten()
            .map(|(_, s)| PathBuf::from(s.doc_path))
    });
    let Some(doc_path) = doc_path else {
        return Ok(());
    };
    let roots = tracking_roots(&cwd, Some(&doc_path));
    if roots.is_empty() {
        return Ok(());
    }

    let state = SessionState {
        session_id: input.session_id.clone(),
        doc_path: doc_path.display().to_string(),
        last_turn_id: input.turn_id.clone(),
        last_prompt: input.prompt.clone(),
        last_auto_queue_head: None,
        updated_at: now_secs(),
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
    if input.stop_hook_active && state.last_auto_queue_head.as_deref() == Some(&prompt) {
        return Ok(Some(StopResponse::Stop {
            continue_: false,
            stop_reason: format!(
                "agent-doc Stop hook requested auto-queue continuation for {}, but the queue head did not advance after the previous continuation request: {:?}. Run `agent-doc {}` manually or remove `auto` from the queue before ending the turn.",
                file.display(),
                prompt,
                file.display()
            ),
        }));
    }
    let mut next_state = state.clone();
    next_state.last_auto_queue_head = Some(prompt.clone());
    next_state.updated_at = now_secs();
    save_state_across_roots(cleanup_roots, loaded_root, &next_state)?;
    // Keep the durable marker's requested-head in sync so a later stop with
    // missing session state still applies the non-advancing-head guard.
    let _ = crate::queue_continuation::record_requested_head(file, &prompt);
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
            "agent-doc Stop hook kept an active `agent:queue auto` moving for {disp}. The next queue prompt is {prompt:?}. Continue THIS turn in-pane: answer that prompt in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`). Do NOT run `agent-doc {disp}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet.",
            disp = file.display(),
            prompt = prompt,
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

    // Non-advancing-head guard: a repeated stop whose marker already requested
    // this exact head must fail closed instead of looping forever.
    if input.stop_hook_active
        && marker.last_requested_head.as_deref() == Some(continuation.head_prompt.as_str())
    {
        return Ok(Some(StopResponse::Stop {
            continue_: false,
            stop_reason: format!(
                "agent-doc Stop hook requested auto-queue continuation for {} from the durable continuation marker, but the queue head did not advance: {:?}. Run `agent-doc {}` manually or remove `auto` from the queue before ending the turn.",
                file.display(),
                continuation.head_prompt,
                file.display()
            ),
        }));
    }

    crate::queue_continuation::record_requested_head(&file, &continuation.head_prompt)?;
    // #codex-self-reinvoke-prevent (Option B): in-pane continuation, not a CLI
    // re-run (see auto_queue_continuation_response).
    Ok(Some(StopResponse::Block {
        decision: "block",
        reason: format!(
            "agent-doc Stop hook found a durable `agent:queue auto` continuation for {disp} with no tracked session state. The next queue prompt is {prompt:?}. Continue THIS turn in-pane: answer that prompt in {disp} and persist with `agent-doc finalize {disp}` (or `agent-doc write --commit {disp}`). Do NOT run `agent-doc {disp}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet.",
            disp = file.display(),
            prompt = continuation.head_prompt,
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
    prompt.trim() == "/clear"
}

pub fn record_external_prompt_for_file(file: &Path, session_id: &str, prompt: &str) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = SessionState {
        session_id: session_id.to_string(),
        doc_path: canonical.display().to_string(),
        last_turn_id: String::new(),
        last_prompt: prompt.to_string(),
        last_auto_queue_head: None,
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
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as ProcessCommand;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
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
<!-- agent:queue auto -->\n\
{queue}\
<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        doc
    }

    fn write_nested_doc(dir: &tempfile::TempDir) -> PathBuf {
        let nested = dir.path().join("nested");
        fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
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

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = crate::harness_prompt::prompt_body_for_file(&doc).unwrap();
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

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
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
        assert!(!prompt_requests_clear("agent-doc tasks/foo.md"));
        assert!(!prompt_requests_clear("/clear please"));
    }

    #[test]
    fn stop_auto_closes_open_cycle_from_last_assistant_message() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final assistant response.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });

        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
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
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let current = format!("{original}\n❯ Why was startup missed?\n");
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
            "Added prioritized follow-up items.\n",
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
            "Added prioritized follow-up items.\n",
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
        let doc = write_doc(&dir);
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

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
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
            last_assistant_message: "Recovered post-closeout drift.".to_string(),
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
        let doc = write_nested_doc(&dir);
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

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: doc.parent().unwrap().display().to_string(),
            last_assistant_message: "Recovered from nested root drift.".to_string(),
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
        let doc = write_doc(&dir);
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

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
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
            last_assistant_message: "Recovered after preamble prompt tracking.".to_string(),
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

        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
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
        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
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
                assert!(reason.contains("do not send the final answer"), "{reason}");
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
        assert!(content.contains("- ~do #fix1~"));
        assert!(content.contains("- do #fix2"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_blocks_from_durable_marker_when_session_state_missing() {
        // #codex-auto-queue-stalled-final-gate live regression (monsterrodholders
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
                assert!(reason.contains("do not send the final answer"), "{reason}");
            }
            other => panic!("expected durable-marker continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_marker_fallback_fails_closed_when_head_does_not_advance() {
        // #codex-auto-queue-stalled-final-gate: a repeated stop (stop_hook_active)
        // whose durable marker already requested this exact head must fail closed
        // instead of looping forever.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").unwrap();
        // The first continuation request recorded this head into the marker.
        crate::queue_continuation::record_requested_head(&doc, "do [#seopdp] deploy").unwrap();

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Stop {
                continue_,
                stop_reason,
            } => {
                assert!(!continue_);
                assert!(stop_reason.contains("did not advance"), "{stop_reason}");
            }
            other => panic!("expected fail-closed Stop, got {other:?}"),
        }
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
        assert!(content.contains("<!-- agent:queue auto -->"));
        assert!(content.contains("queue_active: true"));
        assert!(content.contains("- do #fix1"));
        assert!(!content.contains("- ~do #fix1~"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_fails_closed_when_auto_queue_continuation_makes_no_progress() {
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
            StopResponse::Stop {
                continue_: false,
                stop_reason,
            } => {
                assert!(
                    stop_reason.contains("queue head did not advance"),
                    "{stop_reason}"
                );
                assert!(stop_reason.contains("do #fix1"), "{stop_reason}");
            }
            other => panic!("expected fail-closed no-progress stop, got {other:?}"),
        }
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
}
