//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` inspects the persisted per-document cycle state in
//!   `.agent-doc/state/cycles/<hash>.json` and exits nonzero when the most
//!   recent cycle is still open (`preflight_started`, `response_captured`, or
//!   `write_applied`).
//! - Falls back to the last `ops.log` event only when no cycle-state file
//!   exists yet, preserving compatibility for older repos.
//! - Distinguishes "cycle started but no write/commit followed" from
//!   "response write landed but no commit followed" in both cycle-state and
//!   ops-log fallback paths.
//! - When an open `preflight_started` cycle already has a visible response
//!   patchback in the working tree, reports that manual-repair / commit-boundary
//!   state explicitly instead of collapsing it into the generic open-cycle
//!   message.
//! - Also fails closed when the current document diverges from its snapshot in
//!   a way that looks like a direct assistant patchback (`### Re:` or
//!   `## Assistant`) without a corresponding `agent-doc` cycle.
//! - Also fails closed when the current document already has unresolved
//!   prompt-bearing user edits (`prompt_target`) relative to the snapshot, but
//!   no new `agent-doc` cycle ever started for them. Plain exchange-only
//!   content edits without a fresh prompt target do not reopen a committed
//!   cycle. `agent:queue` prompt edits are excluded from this guard because
//!   queue activation and consumption are owned by the next preflight cycle.
//! - Also fails closed when a closed cycle leaves the live `agent:exchange` tail
//!   ending in a prompt-looking block with no later assistant response. This
//!   catches direct-harness turns where the prompt was already committed into
//!   the snapshot/baseline but the final response patchback never happened.
//! - When that bypassed patchback also leaves prompt-target lines in the same
//!   diff without the binary-owned `❯ ` transcript prefix, `session-check`
//!   reports the bare prompt target in the failure marker so the write path can
//!   be repaired instead of silently accepted.
//! - Narrow self-heal: when that drift is already committed in `HEAD` and the
//!   current working tree matches `HEAD` modulo transient boundary / `(HEAD)`
//!   markers, `session-check` repairs the stale snapshot instead of reporting
//!   a fresh interruption forever.
//! - Exit 0 when the current cycle state is committed, when state/log files
//!   are missing, or when the fallback `ops.log` event is terminal and no
//!   likely bypassed patchback is present.
//! - Exit 1 when the current cycle state is still open, when the fallback last
//!   `ops.log` event is `preflight_diff_start`, when a likely direct
//!   assistant patchback bypassed `agent-doc write` / `finalize`, or when
//!   the cycle state says `committed` but the snapshot does not match HEAD
//!   in the owning git root (response patchback visible but never committed).
//! - Exit 2 on unexpected I/O errors.
//!
//! ## Agentic Contracts
//! - May also clear a persisted startup-miss marker when the marker is proven
//!   stale because a later registered session start has already superseded it.
//! - Otherwise mutates only the snapshot in the narrow committed-historical-drift
//!   repair case above.
//! - Called by supervisors / watchdogs (and directly from skill) to
//!   detect the "started but never wrote" invariant violation flagged
//!   as bug #a011.
//!
//! ## Evals
//! - `session_check_empty_log_exits_zero`
//! - `session_check_open_cycle_state_exits_one`
//! - `session_check_committed_cycle_state_exits_zero`
//! - `detect_bypassed_response_write_flags_template_heading`
//! - `detect_bypassed_response_write_flags_inline_assistant_heading`
//! - `detect_bypassed_response_write_ignores_plain_user_prompt`
//! - `session_check_repairs_committed_historical_snapshot_drift`
//! - `session_check_missing_log_exits_zero`
//! - `session_check_snapshot_committed_guard_fails_when_snapshot_differs`
//! - `session_check_snapshot_committed_guard_passes_when_committed`

use anyhow::{Context, Result};
use std::path::Path;

use crate::component::{is_backlog_component, is_tracked_work_component};

/// Event name prefix emitted by `preflight::run` that indicates a cycle
/// started but may have been abandoned. If this is the final entry in
/// ops.log, the previous cycle did not complete.
pub const PREFLIGHT_START_EVENT: &str = "preflight_diff_start";
pub const IPC_WRITE_CONSUMED_EVENT: &str = "ipc_write_consumed";
pub const SNAPSHOT_SAVED_FILE_IPC_EVENT: &str = "snapshot_saved_file_ipc";
pub const IPC_PROOF_INSUFFICIENT_EVENT: &str = "ipc_proof_insufficient";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

pub struct SessionCheckReport {
    pub status: SessionCheckStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
enum GuardResult {
    None,
    Warn(Vec<String>),
    Error(String),
}

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path) -> Result<()> {
    run_with_options(file, false)
}

/// `session-check` with the optional Codex final-gate.
///
/// Default (`codex_final_gate = false`): keeps exit 0 for a clean document and
/// prints `queue_continuation_required=...` as an informational typed detail.
/// Strict (`codex_final_gate = true`): exits `2` when a clean document still
/// owes an active `agent:queue auto` continuation, so Codex direct-exec closeout
/// paths cannot send a final answer past a stalled queue.
/// (#codex-auto-queue-stalled-final-gate)
pub fn run_with_options(file: &Path, codex_final_gate: bool) -> Result<()> {
    self_heal_late_ipc_overapplication(file)?;
    let report = inspect_with_warnings(file)?;
    for warning in &report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(message) => {
            println!("{}", message);
            if let Some(continuation) = crate::queue_continuation::detect(file)? {
                // #prompt-preempts-auto-queue: a live unresolved exchange prompt
                // must run before queue continuation, even when it was already
                // baselined into the snapshot. Defer the queue (do not force the
                // Codex final-gate) while such a prompt exists so the next cycle
                // answers it instead of skipping to the queue head.
                if let Some(unresolved) = unresolved_exchange_prompt(file)? {
                    println!(
                        "queue_continuation_required=false queue_deferred_for_unresolved_exchange_prompt={:?} next_queue_prompt={:?}",
                        unresolved, continuation.head_prompt
                    );
                    eprintln!(
                        "[session-check] queue continuation deferred for {}: unresolved exchange prompt {:?} must run before the queue head {:?} (#prompt-preempts-auto-queue).",
                        file.display(),
                        unresolved,
                        continuation.head_prompt
                    );
                    return Ok(());
                }
                if let Some(command) =
                    crate::queue_command::slash_command_text(&continuation.head_prompt)
                {
                    println!(
                        "queue_continuation_required=true next_queue_command={:?}",
                        command
                    );
                } else {
                    println!(
                        "queue_continuation_required=true next_queue_prompt={:?}",
                        continuation.head_prompt
                    );
                }
                if codex_final_gate {
                    if let Some(command) =
                        crate::queue_command::slash_command_text(&continuation.head_prompt)
                    {
                        eprintln!(
                            "[session-check] codex-final-gate: active `agent:queue auto` slash command required for {} — submit {} after the current turn reaches an idle prompt before sending any final answer.",
                            file.display(),
                            command
                        );
                    } else {
                        eprintln!(
                            "[session-check] codex-final-gate: active `agent:queue auto` continuation required for {} — continue with `agent-doc {}` before sending any final answer.",
                            file.display(),
                            file.display()
                        );
                    }
                    std::process::exit(2);
                }
            } else {
                println!("queue_continuation_required=false");
            }
            // #finalize-owned-pane-response-patchback: proactive final-gate
            // block. When a Codex same-pane recursive invocation was refused
            // (abandoned cycle with last_event starting
            // "recursive_direct_invocation_blocked") but no response body was
            // captured, the agent may still produce a final chat answer that
            // bypasses `agent-doc write` / `finalize`. Block the final answer
            // so the operator must pipe the response through binary-owned
            // closeout.
            //
            // Recovery adoption: if the response was already patched into
            // agent:exchange (no unresolved prompt), the abandoned cycle is
            // recoverable — adopt the visible response idempotently instead of
            // blocking.
            if codex_final_gate
                && let Some(cycle) = crate::cycle_state::load(file).ok().flatten()
                && matches!(cycle.phase, crate::cycle_state::CyclePhase::Abandoned)
                && cycle
                    .last_event
                    .starts_with("recursive_direct_invocation_blocked")
                && cycle.capture_id.is_none()
                && cycle.response_sha256.is_none()
            {
                let has_visible_response = unresolved_exchange_prompt(file)?.is_none()
                    && exchange_tail_has_response_heading(file);
                if has_visible_response {
                    eprintln!(
                        "[session-check] codex-final-gate: recursive direct invocation was blocked for {} but the response is already visible in agent:exchange — adopting the manual patchback idempotently.",
                        file.display()
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "codex_final_gate_manual_patchback_adopted file={} cycle_id={} last_event={}",
                            file.display(),
                            cycle.cycle_id,
                            cycle.last_event
                        ),
                    );
                } else {
                    eprintln!(
                        "[session-check] codex-final-gate: recursive direct invocation was blocked for {} with no captured response body — pipe the response through `agent-doc write --commit {}` before sending any final answer.",
                        file.display(),
                        file.display()
                    );
                    std::process::exit(2);
                }
            }
            Ok(())
        }
        SessionCheckStatus::Interrupted(message) => {
            println!("{}", message);
            std::process::exit(1);
        }
    }
}

pub fn inspect(file: &Path) -> Result<SessionCheckStatus> {
    Ok(inspect_with_warnings(file)?.status)
}

pub fn inspect_with_warnings(file: &Path) -> Result<SessionCheckReport> {
    let mut report = SessionCheckReport {
        status: inspect_core(file)?,
        warnings: Vec::new(),
    };
    if matches!(report.status, SessionCheckStatus::Ok(_)) {
        // Phase 6 (#lr-content-6): build one RunContext for the whole guard
        // sweep. `set_doc_content` populates `DocContentCell` once; every guard
        // that needs the document, its frontmatter, or its parsed components
        // reads from the cached `FrontmatterSlot` / `ComponentsSlot` instead of
        // independently re-reading + re-parsing the file (previously ~20 reads
        // and ~10 parses per `inspect` call).
        let rc = crate::graph::RunContext::new(file.to_path_buf());
        rc.set_doc_content(std::fs::read_to_string(file)?);
        match check_dropped_exchange_prompt_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_dropped_queue_prompt_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_queue_response_contamination_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        if let Some(message) = check_completed_pending_reap_guard(file, &rc)? {
            report.status = SessionCheckStatus::Interrupted(message);
            return Ok(report);
        }
        match check_shadow_backlog_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_malformed_tracked_item_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_backlog_replay_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_snapshot_committed_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_parent_submodule_pointer_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_committed_without_response_body_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_no_response_active_queue_head(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_reaped_queue_head_without_response(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_prompt_only_exchange_tail_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        for guard in [
            check_pending_capture_guard(file, &rc)?,
            check_pending_done_guard(file, &rc)?,
            check_expect_done_or_gate_guard(file, &rc)?,
            check_partial_closeout_state_guard(file)?,
            check_partial_staging_closeout_guard(file)?,
            check_blocked_closeout_followup_guard(file, &rc)?,
            check_gated_phase_split_guard(file, &rc)?,
            check_queue_audit_partial_completion_guard(file)?,
            check_queue_head_removal_guard(file, &rc)?,
            check_free_text_queue_head_provenance(file, &rc)?,
        ] {
            match guard {
                GuardResult::None => {}
                GuardResult::Warn(lines) => report.warnings.extend(lines),
                GuardResult::Error(message) => {
                    report.status = SessionCheckStatus::Interrupted(message);
                    break;
                }
            }
        }
        if let Ok(Some(miss)) = crate::startup_miss::load(file) {
            if let Some(supersession) =
                crate::startup_miss::superseded_by_newer_registered_start(file, &miss)?
            {
                crate::startup_miss::clear(file)?;
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "session_check_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} latest_start_timestamp={}",
                        file.display(),
                        miss.pane_id,
                        supersession.registered_pane,
                        supersession.latest_start_timestamp
                    ),
                );
            } else {
                let detail = crate::startup_miss::session_log_diagnostic(file, &miss.session_id)
                    .ok()
                    .flatten()
                    .map(|detail| format!("; {detail}"))
                    .unwrap_or_default();
                report.warnings.push(format!(
                    "[session-check] WARNING: startup-miss marker exists for pane {} ({:?}) — the last {} start never acknowledged a document cycle{}",
                    miss.pane_id, miss.origin, miss.harness, detail
                ));
            }
        }
    }
    Ok(report)
}

fn normalized_prompt_for_match(line: &str) -> String {
    line.trim().trim_start_matches('❯').trim().to_string()
}

/// True when `doc`'s `agent:exchange` component contains a line matching the
/// given prompt (normalized: leading `❯` and whitespace stripped). Used to
/// decide whether a recorded dropped prompt has been resolved (reached the
/// committed document) so the guard can clear and stop firing.
fn exchange_contains_prompt_line(doc: &str, prompt: &str) -> bool {
    let needle = normalized_prompt_for_match(prompt);
    if needle.is_empty() {
        return true;
    }
    let Ok(components) = crate::component::parse(doc) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    exchange
        .content(doc)
        .lines()
        .any(|line| normalized_prompt_for_match(line) == needle)
}

/// True when the committed `doc`'s `agent:queue` still contains the given queue
/// prompt line (normalized). Used to decide whether a dropped user queue edit
/// reached HEAD (preserved) so the guard can clear.
fn normalized_queue_line_for_match(line: &str) -> String {
    let trimmed = line.trim();
    let trimmed = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed);
    normalized_prompt_for_match(trimmed)
}

fn queue_contains_prompt_line(doc: &str, prompt: &str) -> bool {
    let needle = normalized_queue_line_for_match(prompt);
    if needle.is_empty() {
        return true;
    }
    let Ok(components) = crate::component::parse(doc) else {
        return false;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return false;
    };
    queue
        .content(doc)
        .lines()
        .any(|line| normalized_queue_line_for_match(line) == needle)
}

/// `#queue-user-edit-overwrite`: fail closed when this cycle recorded a
/// user-authored `agent:queue` edit dropped during a `content_ours` IPC adoption
/// and that queue line is still absent from the committed `HEAD` — unless the
/// current response legitimately consumed it (its `do [#id]` id reached a
/// lifecycle outcome this cycle). A preserved queue line (reached HEAD's queue
/// or exchange) or a consumed head clears the marker; a silently-deleted user
/// queue edit fails closed.
fn check_dropped_queue_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_queue_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    // Unlike the exchange guard, a user queue edit is SUPPOSED to stay out of
    // HEAD: `content_ours` adoption preserves it on disk so it re-surfaces as a
    // next-cycle diff. The loss case is the edit vanishing from the visible
    // document, so check the current file (and HEAD as a committed fallback).
    // Phase 6 (#lr-content-6): cached document content via `DocContentCell`.
    let visible = rc.doc_content();
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;
    let still_missing: Vec<String> = state
        .dropped_queue_prompts
        .iter()
        .filter(|prompt| {
            // Preserved in the visible/HEAD queue, or answered in the
            // visible/HEAD exchange → kept, not lost.
            if queue_contains_prompt_line(&visible, prompt)
                || queue_contains_prompt_line(head, prompt)
                || exchange_contains_prompt_line(&visible, prompt)
                || exchange_contains_prompt_line(head, prompt)
            {
                return false;
            }
            // Legitimately consumed this cycle: the queued `do [#id]` id reached
            // a done/gate/reap outcome, so deleting the queue line is correct.
            let consumed = do_directive_target_ids(std::slice::from_ref(prompt))
                .into_iter()
                .any(|id| resolved_ids.contains(&id));
            !consumed
        })
        .cloned()
        .collect();
    if still_missing.is_empty() {
        crate::cycle_state::clear_dropped_queue_prompts(file)?;
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "dropped_queue_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored agent:queue edit(s) were dropped during an IPC content_ours merge and are missing from the visible document without being consumed: {}. Convergence overwrote a newer visible queue; re-add them to `agent:queue` and re-run `agent-doc finalize {}` / `agent-doc write --commit {}` so the queued work is preserved (see #queue-user-edit-overwrite).",
        still_missing.join("; "),
        file.display(),
        file.display()
    )))
}

/// Collect the assistant `### Re:` response prose from an exchange component
/// body (heading + boundary + comment + blockquote lines excluded). Used to
/// detect queue lines that were copied out of a response body.
///
/// Blockquote lines (`> …`) are EXCLUDED: every `### Re:` response echoes the
/// prompt it is answering as a `> **Queue prompt:** … > <verbatim head text>`
/// quote. Including those echoes made the contamination guard
/// (`check_queue_response_contamination_guard`) false-positive on any
/// still-live free-text queue head whose text a response quoted — the response
/// legitimately quotes the head it answered, and an earlier response that
/// quoted a near-identical prompt would flag an unrelated live queue item too.
/// The guard targets assistant ANSWER prose copied into the queue, not the
/// prompt-echo, so blockquoted quotes are not response prose for this purpose.
/// (#jb-run-agent-doc-response-queue-contamination — blockquote-echo false positive)
fn assistant_response_text(exchange_body: &str) -> String {
    let mut in_response = false;
    let mut out = String::new();
    for line in exchange_body.lines() {
        let trimmed = line.trim();
        if is_exchange_response_heading(trimmed) {
            in_response = true;
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary") {
            continue;
        }
        if in_response
            && !trimmed.is_empty()
            && !trimmed.starts_with("<!--")
            && !trimmed.starts_with('>')
        {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

/// True when a queue prompt's text is a recognized directive / id-bearing prompt
/// (a legitimate queue entry shape) rather than free-text prose.
fn is_queue_directive_prompt(text: &str) -> bool {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    lower.starts_with("do ")
        || lower.starts_with("preset ")
        || lower.starts_with("dispatch ")
        || lower.starts_with("run ")
        || t.starts_with('#')
        || t.contains("[#")
}

/// True when a queue prompt references a slash command (e.g. `/agent-doc`,
/// `/clear`, `/compact`, `/loop`) at a token boundary.
///
/// `#queue-contamination-guard-false-positive`: such a prompt is a
/// user-authored instruction, not assistant answer prose copied into the queue.
/// The contamination guard targets declarative answer prose ("Yes. I drove
/// ..."), which never leads with slash-command instructions, so a legit user
/// prompt that mentions `/agent-doc`/`/clear` must not be flagged just because
/// it shares a verbatim 40-char run with a response that discussed the same
/// commands. (A leading `/` is also a unix-path shape, which is likewise user
/// content, not copied answer prose — so this skip stays conservative.)
fn mentions_slash_command(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        // The slash must begin a token (start of string or after a non-word char)
        // so `src/agent-doc` (a path segment after a word char) is not matched.
        let at_token_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !at_token_start {
            continue;
        }
        // Require at least two command-name chars after the slash so a bare
        // separator `/` is not treated as a command reference.
        let cmd_len = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .count();
        if cmd_len >= 2 && text[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            return true;
        }
    }
    false
}

/// `#jb-run-agent-doc-response-queue-contamination`: `Run Agent Doc` / queue
/// synthesis must never enqueue assistant response prose. The live repro added
/// `- Yes. I drove the already-authenticated Google Ads browser session ...`
/// (copied from a `### Re:` body) to `agent:queue auto`. Detect a free-text
/// queue prompt whose text appears inside an assistant response body and fail
/// closed naming the contaminating candidate.
fn check_queue_response_contamination_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Ok(GuardResult::None);
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(GuardResult::None);
    };

    let queue_body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = crate::queue::parse(queue_body) else {
        return Ok(GuardResult::None);
    };
    let response_text = assistant_response_text(exchange.content(&content));
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }

    let mut contaminated: Vec<String> = Vec::new();
    for prompt in crate::queue::prompts(&entries) {
        let text = prompt.text.trim();
        if text.is_empty() || is_queue_directive_prompt(text) {
            continue;
        }
        // #queue-contamination-guard-false-positive: a queue prompt that
        // references a slash command (/agent-doc, /clear, /compact, ...) is a
        // user instruction, not copied answer prose — skip it.
        if mentions_slash_command(text) {
            continue;
        }
        // Only treat substantial prose as a contamination candidate; short
        // free-text prompts are legitimate (`#free-text-queue-head-consume`).
        let normalized = normalized_prompt_for_match(text);
        if normalized.chars().count() < 20 {
            continue;
        }
        let needle: String = normalized.chars().take(40).collect();
        if response_text.contains(&needle) {
            contaminated.push(text.chars().take(80).collect::<String>());
        }
    }

    if contaminated.is_empty() {
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_response_contamination_guard_failed file={} count={}",
            file.display(),
            contaminated.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: agent:queue contains assistant response prose copied from a `### Re:` body, not a user prompt or `do [#id]` directive: {}. Remove the contaminating line(s) from `agent:queue` (only user prompts, `do [#id]`, `preset`/`dispatch`, or backlog-derived entries are valid queue sources) and re-run finalize (see #jb-run-agent-doc-response-queue-contamination).",
        contaminated
            .iter()
            .map(|t| format!("{:?}", t))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// `#exchange-prompt-dropped-on-merge`: fail closed when this cycle recorded a
/// user-authored exchange prompt dropped during a `content_ours` IPC adoption
/// and that prompt is still absent from the committed `HEAD`. The evidence is
/// persisted at adoption time, so this guard catches the silent-loss class even
/// when the editor overwrote the disk prompt via IPC buffer convergence before
/// the post-commit disk diff could observe it.
fn check_dropped_exchange_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_exchange_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let still_missing: Vec<String> = state
        .dropped_exchange_prompts
        .iter()
        .filter(|prompt| !exchange_contains_prompt_line(head, prompt))
        .cloned()
        .collect();
    if still_missing.is_empty() {
        // The dropped prompt reached the committed document — resolved.
        crate::cycle_state::clear_dropped_exchange_prompts(file)?;
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "dropped_exchange_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored exchange prompt(s) were dropped during an IPC content_ours merge and are missing from the committed document: {}. The cycle committed `content_ours` without these prompt-bearing line(s); re-add them to `agent:exchange` and re-run `agent-doc finalize {}` / `agent-doc write --commit {}` so they are answered (see #exchange-prompt-dropped-on-merge).",
        still_missing.join("; "),
        file.display(),
        file.display()
    )))
}

fn check_completed_pending_reap_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<String>> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let completed: Vec<crate::pending::PendingItem> = components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| completed_pending_items(component.content(&content)))
        .collect();
    if completed.is_empty() {
        return Ok(None);
    }

    let refs = completed
        .into_iter()
        .map(|item| {
            if item.id.is_empty() {
                format!("<missing-id> {}", item.text)
            } else {
                format!("#{}", item.id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if refs.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "[session-check] INTERRUPTED: document still contains completed tracked item(s) after closeout: {}. Re-run preflight/repair so the reap is persisted through the snapshot + commit boundary",
        refs
    )))
}

fn completed_pending_items(body: &str) -> Vec<crate::pending::PendingItem> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(crate::pending::PendingItem::is_done)
        .collect()
}

fn check_snapshot_committed_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    use crate::git::SnapshotCommitStatus;
    match rc.snapshot_commit_status() {
        SnapshotCommitStatus::Committed
        | SnapshotCommitStatus::NoSnapshot
        | SnapshotCommitStatus::NoHead
        | SnapshotCommitStatus::NotInGitRepo => Ok(GuardResult::None),
        SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            // Phase 3 (#jbccc3): silently treat the auto-recoverable cancel
            // pattern as a non-error here. Standalone `session-check` is then
            // free to surface OK while preflight runs the binary-owned commit
            // through `enforce_no_uncommitted_closeout_drift`. Without this
            // skip, the guard would still bail with the misleading "cycle
            // state is committed but the snapshot does not match HEAD"
            // message that masks the JB cache-conflict cancel root cause.
            if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
                return Ok(GuardResult::None);
            }
            let side_effects = tracked_side_effect_note(file)?;
            let msg = format!(
                "[session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (snapshot_len={}, head_len={}). The response patchback is visible but was never committed{} {}",
                snapshot_len,
                head_len,
                side_effects,
                closeout_recovery_hint(file)
            );
            eprintln!("{}", msg);
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_committed_guard_failed file={} snapshot_len={} head_len={}",
                    file.display(),
                    snapshot_len,
                    head_len
                ),
            );
            Ok(GuardResult::Error(msg))
        }
    }
}

fn closeout_recovery_hint(file: &Path) -> String {
    // `#closeout-repair-churn`: render one typed recovery instruction for the
    // classified state instead of a single static "try write --commit" line.
    let state = crate::flow::closeout::classify_closeout_recovery_state(file);
    match state.recovery_command(file) {
        Some(command) => format!("Recovery [{}]: {}.", state.as_str(), command),
        None => format!(
            "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
            file.display(),
            file.display()
        ),
    }
}

/// `#codex-final-response-not-written`: a completed turn that committed real
/// binary-owned work this cycle but never captured an assistant response body.
///
/// Symptom: an agent (notably a Codex/direct-exec run, or any cycle whose
/// `finalize` landed pending mutations + the commit but lost the response — e.g.
/// a malformed/empty patchback) reaches `Committed` with side effects applied,
/// yet `agent:exchange` has no new `### Re:` close-out. The cycle-state proves
/// it: a real binary write turn sets `had_pending_mutations`, and a captured
/// response always sets `capture_id`/`response_sha256` (see
/// `capture::record` → `cycle_state::mark_response_captured`). So
/// `Committed` + `had_pending_mutations` + no `capture_id` means the write path
/// processed this turn's mutations and committed without ever persisting a
/// response — the missing close-out.
///
/// This is precise rather than broad: a no-op sweep close
/// (`closing cycle as already committed`) never sets `had_pending_mutations`,
/// and any normal response cycle sets `capture_id`, so neither false-fires.
/// Recovery is non-destructive — land the visible response through
/// `agent-doc write --commit`, which sets `capture_id` and clears the guard.
/// True when the committed `agent:exchange` contains at least one assistant
/// `### Re:` response heading (`#codex-queue-drain-no-response-body`). Used to
/// verify a queue-drain turn actually landed a response body in the document
/// rather than only mutating status/queue/backlog. A doc with no exchange
/// component, or an exchange holding only a compacted `### Session Summary`,
/// returns false.
fn committed_exchange_has_response_body(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    let components = crate::component::parse(&content)?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(false);
    };
    let body = &content[exchange.open_end..exchange.close_start];
    Ok(body
        .lines()
        .any(|line| line.trim_start().starts_with("### Re:")))
}

fn check_committed_without_response_body_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    let committed_exchange_has_body = committed_exchange_has_response_body(file)?;
    if committed_exchange_has_body {
        return Ok(GuardResult::None);
    }
    // A captured response (capture_id/response_sha256) normally means the
    // close-out body landed through the binary write path — not the
    // missing-response shape. EXCEPTION (#codex-queue-drain-no-response-body):
    // the systematic Codex queue-drain bug sets the capture record yet commits
    // only status/queue/backlog, leaving `agent:exchange` with zero `### Re:`
    // blocks. So for a queue-drain turn, capture metadata alone is not proof —
    // require the committed exchange to actually contain a response body before
    // trusting it. Non-queue turns keep trusting the capture record (no behavior
    // change); only a queue turn whose committed exchange has no `### Re:` body
    // falls through to fire.
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        let is_queue_turn = state.queue_task_id.is_some() || !state.active_queue_heads.is_empty();
        if !is_queue_turn {
            return Ok(GuardResult::None);
        }
    }
    // Only fire when this cycle ran a response-write turn. `had_pending_mutations`
    // is set exclusively by the `write`/`finalize` response path
    // (`write.rs` → `cycle_state::mark_pending_mutations`), so it proves a real
    // response cycle processed mutations this turn. Bookkeeping-only commits stay
    // OK: a bare sweep re-commit never touches it, and `repair`'s completed-backlog
    // reap records `reaped_pending_ids` via `record_reaped_pending_ids` WITHOUT
    // `mark_pending_mutations` — a legitimate no-response commit that must not fire.
    if !state.had_pending_mutations {
        return Ok(GuardResult::None);
    }
    // A no-op commit (`commit_already_current`) committed NO new binary-owned work
    // this turn: the snapshot already equalled `HEAD`, so the pending mutation (a
    // reap / `--done` of an item already reflected in `HEAD`) left the document
    // byte-identical and there is nothing a paired response would accompany. Firing
    // here deadlocks the cycle — the recommended `write --commit` recovery is itself
    // a no-op (no response body to write, nothing to commit), so a re-running
    // closeout poller re-interrupts every pass forever. A real
    // `#codex-final-response-not-written` miss commits actual side-effect content
    // (`last_event` `commit_success` / `commit`), so it still fires.
    if crate::cycle_state::is_noop_commit_event(&state.last_event) {
        crate::ops_log::log_op(
            file,
            &format!(
                "committed_without_response_body_guard_skipped_noop_commit file={} cycle_id={} last_event={} pending_done={} reaped={}",
                file.display(),
                state.cycle_id,
                state.last_event,
                state.pending_done_ids.len(),
                state.reaped_pending_ids.len(),
            ),
        );
        return Ok(GuardResult::None);
    }
    let side_effects = tracked_side_effect_note(file)?;
    let msg = format!(
        "[session-check] INTERRUPTED: cycle committed binary-owned work this turn but no assistant `### Re:` response body is present in `agent:exchange` (cycle `{}`, last_event `{}`). The close-out response was never written into `agent:exchange`{} (#codex-queue-drain-no-response-body). {}",
        state.cycle_id,
        state.last_event,
        side_effects,
        closeout_recovery_hint(file)
    );
    eprintln!("{}", msg);
    crate::ops_log::log_op(
        file,
        &format!(
            "committed_without_response_body_guard_failed file={} cycle_id={} last_event={} had_pending_mutations={} pending_done={} reaped={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            state.had_pending_mutations,
            state.pending_done_ids.len(),
            state.reaped_pending_ids.len(),
        ),
    );
    Ok(GuardResult::Error(msg))
}

/// `#nochange-after-stall-breadth`: a no-response repair/reap-only closeout
/// must not make an active queue head look complete. The missing-response guard
/// intentionally skips no-op bookkeeping commits to avoid deadlocking ordinary
/// `--done` repairs, but when the same cycle recorded a runnable `agent:queue`
/// head and that head is still both queued and open in `agent:backlog`, the
/// turn made no durable progress on executable work. Fail closed so the next
/// actor runs the head instead of reporting a plain no-change/clean closeout.
fn check_no_response_active_queue_head(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        return Ok(GuardResult::None);
    }
    let bookkeeping_evidence = state.had_pending_mutations
        || !state.pending_done_ids.is_empty()
        || !state.pending_kept_open_ids.is_empty()
        || !state.reaped_pending_ids.is_empty()
        || !state.pending_gated_ids.is_empty()
        || state.pending_added_this_cycle;
    if !bookkeeping_evidence {
        return Ok(GuardResult::None);
    }
    let recorded_ids = do_directive_target_ids(&state.active_queue_heads);
    if recorded_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let content = rc.doc_content();
    let current_head_ids: std::collections::HashSet<String> =
        committed_current_queue_head_ids(&content)
            .into_iter()
            .map(|id| crate::pending::normalize_pending_id(&id))
            .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();
    let mut resolved_or_deferred = crate::cycle_state::resolved_pending_ids(file)?;
    resolved_or_deferred.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| crate::pending::normalize_pending_id(id)),
    );

    let mut live: Vec<String> = Vec::new();
    for id in recorded_ids {
        let norm = crate::pending::normalize_pending_id(&id);
        if norm.is_empty() {
            continue;
        }
        if !current_head_ids.contains(&norm) || !open_backlog.contains(&norm) {
            continue;
        }
        if resolved_or_deferred.contains(&norm) {
            continue;
        }
        if !live.iter().any(|existing| existing == &norm) {
            live.push(norm);
        }
    }

    if live.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = live
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "no_response_active_queue_head_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            live.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: cycle `{}` committed without an assistant response body while runnable agent:queue head(s) {} remained queued and open in agent:backlog; this was a no-response repair/reap-only closeout, not a completed queue turn",
        state.cycle_id, ids
    );
    let repair = format!(
        "run `agent-doc {}` from the owning session so the queued head is answered, or resolve each id through `agent-doc write --commit {}` with `--done`, `--pending-gate`, or `--pending-edit` proof before closing",
        file.display(),
        file.display()
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #nochange-after-stall-breadth)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #nochange-after-stall-breadth)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `#compact-reap-no-response-record`: a reap-only / no-response-body closeout
/// that reaps a `do #id` queue-directive head this cycle, where that id's
/// `### Re:` response is absent from both the live exchange and any
/// HEAD-referenced compact archive, has silently lost the response record.
///
/// This is the gap left by [`check_no_response_active_queue_head`], which only
/// fires while the head is still queued *and* open in `agent:backlog`. Once a
/// maintenance / compaction reap removes the id from `agent:backlog` (and strikes
/// it from the queue), that guard's `current_head_ids ∧ open_backlog` condition is
/// false, so the silent loss goes undetected and `finalize --done` later fails
/// with "id not found in backlog".
///
/// The precondition `capture_id.is_none() && response_sha256.is_none()` scopes the
/// guard to reap-only / bookkeeping closeouts: a real response cycle records a
/// capture, so its reaps are answered (not lost) and never reach this guard. A
/// legitimate prior-cycle reap (the id was answered in an earlier cycle and only
/// reaped now) is filtered out by [`directive_response_materialized`], which finds
/// the `### Re: ... #id` heading in the live exchange or a HEAD compact archive.
fn check_reaped_queue_head_without_response(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    // A response was captured this cycle → its reaps are answered, not lost.
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        return Ok(GuardResult::None);
    }
    if state.reaped_pending_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let directive_ids: std::collections::HashSet<String> =
        do_directive_target_ids(&state.active_queue_heads)
            .into_iter()
            .map(|id| crate::pending::normalize_pending_id(&id))
            .filter(|id| !id.is_empty())
            .collect();
    if directive_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    let reaped: std::collections::HashSet<String> = state
        .reaped_pending_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    let content = rc.doc_content();
    let head = crate::git::show_head(file).ok().flatten();
    let mut lost: Vec<String> = Vec::new();
    for id in directive_ids {
        if !reaped.contains(&id) {
            continue;
        }
        if directive_response_materialized(file, &content, head.as_deref(), &id) {
            continue;
        }
        if !lost.iter().any(|existing| existing == &id) {
            lost.push(id);
        }
    }
    if lost.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = lost
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "reaped_queue_head_without_response_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            lost.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: cycle `{}` reaped `do` queue-directive head(s) {} into agent:done without an assistant response landing in agent:exchange (no response body this cycle and no `### Re:` for the id in the exchange or a HEAD compact archive); the response record was silently lost",
        state.cycle_id, ids
    );
    let repair = format!(
        "recover the lost response by re-running `agent-doc {}` so the directive id is answered, or restore the missing `### Re:` block through `agent-doc write --commit {}` before closing",
        file.display(),
        file.display()
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #compact-reap-no-response-record)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #compact-reap-no-response-record)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// True when a `### Re:` response heading targeting `id` (normalized, no `#`)
/// exists in the live exchange `content` or in any HEAD-referenced compact
/// archive. Used by [`check_reaped_queue_head_without_response`] to distinguish a
/// legitimate prior-cycle reap (response durably recorded, possibly archived) from
/// a silent loss.
fn directive_response_materialized(
    file: &Path,
    content: &str,
    head: Option<&str>,
    id: &str,
) -> bool {
    if content_has_re_heading_for_id(content, id) {
        return true;
    }
    let Some(head) = head else {
        return false;
    };
    crate::flow::closeout::compact_archive_pointers(head)
        .into_iter()
        .any(|pointer| {
            crate::flow::closeout::read_head_compact_archive(file, pointer)
                .map(|archive| content_has_re_heading_for_id(&archive, id))
                .unwrap_or(false)
        })
}

/// True when any `### Re:` heading line in `content` references `#id` / `[#id]`.
/// `do #id` responses always render under a `### Re: ... #id` heading, so a
/// heading-scoped match avoids false matches against queue-prompt echoes or
/// backlog lines that merely mention the id.
fn content_has_re_heading_for_id(content: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let needle = format!("#{}", id.to_ascii_lowercase());
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("### Re:") && !trimmed.starts_with("###Re") {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.find(&needle) {
            None => false,
            Some(pos) => {
                // Reject a longer-id prefix collision (`#ab` must not match `#abc`).
                let after = &lower[pos + needle.len()..];
                !after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
        }
    })
}

/// Pure inputs for the dormant per-id response-loss detector
/// ([`reaped_directive_ids_without_response`], `#z2jy` bkx9-pure-detector).
///
/// All ids are normalized (no leading `#`, lowercased — the caller passes them
/// through [`crate::pending::normalize_pending_id`]). The detector performs no
/// I/O: the caller resolves `content` (the live committed exchange) and
/// `archives` (the HEAD-referenced compact-archive bodies) up front, so the core
/// logic stays deterministically unit-testable.
// Dormant (#z2jy): constructed only by unit tests until the #bkx9 wiring lands.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct ReapedResponseLossInput<'a> {
    /// `do #id` directive target ids active this cycle.
    pub directive_ids: &'a [String],
    /// Pending ids reaped into `agent:done` this cycle.
    pub reaped_ids: &'a [String],
    /// Live committed exchange content.
    pub content: &'a str,
    /// HEAD-referenced compact-archive bodies (each searched like `content`).
    pub archives: &'a [String],
}

/// Pure per-id response-loss detector (`#z2jy` bkx9-pure-detector). DORMANT.
///
/// Returns the reaped `do #id` directive ids whose `### Re: ... #id` response
/// heading did NOT materialize — neither in the live exchange `content` nor in
/// any HEAD compact `archives` entry. Order follows `directive_ids`; duplicates
/// are collapsed.
///
/// Unlike the live [`check_reaped_queue_head_without_response`] guard, this core
/// does not consult per-cycle capture state, so it also surfaces the `#bkx9`
/// residual — a response body *was* captured this cycle but a specific id's
/// `### Re:` was lost in a CRDT merge (the captured-but-id-lost case).
///
/// It is intentionally NOT wired into the live guard yet: wiring it (and proving
/// it against a reproduced `#ipc-crdt-response-drift`) is gated as `#bkx9`,
/// because this guard runs at every `write --commit` closeout and a false
/// positive would wedge all closeouts. The known false-positive class is pinned
/// by the unit tests: a single `### Re:` heading that answers `do #A` + `do #B`
/// but names only `#A` flags `#B` as lost.
///
/// See `specs/07-closeout-commands.md` `#compact-reap-no-response-record`.
// Dormant (#z2jy): exercised only by unit tests until the #bkx9 wiring lands.
#[allow(dead_code)]
pub(crate) fn reaped_directive_ids_without_response(
    input: &ReapedResponseLossInput<'_>,
) -> Vec<String> {
    let reaped: std::collections::HashSet<&str> =
        input.reaped_ids.iter().map(String::as_str).collect();
    let mut lost: Vec<String> = Vec::new();
    for id in input.directive_ids {
        if id.is_empty() || !reaped.contains(id.as_str()) {
            continue;
        }
        let materialized = content_has_re_heading_for_id(input.content, id)
            || input
                .archives
                .iter()
                .any(|archive| content_has_re_heading_for_id(archive, id));
        if materialized {
            continue;
        }
        if !lost.iter().any(|existing| existing == id) {
            lost.push(id.clone());
        }
    }
    lost
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JbCacheConflictAcceptDuplicateReplay {
    pub heading: String,
    pub deduped_content: String,
}

/// Detect the late JetBrains File Cache Conflict "accept" replay shape.
///
/// The stale editor/cache payload lands after the cycle already committed, so
/// the working tree contains an extra adjacent response block while `HEAD`
/// still contains the correct single-response document. This is not a fresh
/// direct patchback; it is safe to repair by replacing the working tree and
/// snapshot with `dedupe(current)` when that result matches `HEAD` modulo
/// transient editor markers.
pub fn detect_jb_cache_conflict_accept_duplicate_replay(
    file: &Path,
) -> Result<Option<JbCacheConflictAcceptDuplicateReplay>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_accept_duplicate_replay_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_accept_duplicate_replay_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<JbCacheConflictAcceptDuplicateReplay>> {
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(heading) = crate::dedupe::first_duplicate_response_heading(&current) else {
        return Ok(None);
    };
    let deduped = crate::dedupe::dedupe_responses(&current);
    if deduped == current {
        return Ok(None);
    }
    let Some(head) = rc.head_content() else {
        return Ok(None);
    };
    if crate::git::normalize_transient_agent_doc_markers(&deduped)
        != crate::git::normalize_transient_agent_doc_markers(&head)
    {
        return Ok(None);
    }

    Ok(Some(JbCacheConflictAcceptDuplicateReplay {
        heading,
        deduped_content: head.to_string(),
    }))
}

/// A late-IPC reposition / stale-patch replay re-inserted the committed
/// response into the working tree after the cycle already reached `HEAD`.
///
/// The duplicate body matches `HEAD`'s committed response (possibly wrapped in
/// redundant `<!-- agent:boundary:* -->` markers and non-adjacent), so the
/// safe repair is to restore the committed `HEAD` content over the working tree
/// and snapshot. See `tasks/agent-doc/plan-duplicate-response-after-commit.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LateIpcResponseOverapplication {
    pub remediated_content: String,
}

/// Detect the late-IPC committed-response over-application shape.
///
/// Unlike [`detect_jb_cache_conflict_accept_duplicate_replay`], this does not
/// require the duplicate to be a *consecutive* `### Re:` block — the reposition
/// signal can leave the re-applied copy separated by boundary markers, which
/// the consecutive-only `dedupe_responses` collapse misses, letting the generic
/// `detect_bypassed_response_write` guard misclassify it as a manual patchback.
/// We instead prove that the working tree is `HEAD` plus extra duplicate copies
/// of already-committed responses (identical scaffold, same response set), in
/// which case restoring `HEAD` is provably safe.
pub fn detect_late_ipc_response_overapplication(
    file: &Path,
) -> Result<Option<LateIpcResponseOverapplication>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_late_ipc_response_overapplication_with_context(file, &rc)
}

pub fn detect_late_ipc_response_overapplication_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<LateIpcResponseOverapplication>> {
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let Some(head) = rc.head_content() else {
        return Ok(None);
    };
    // Strict path: surplus block is a byte-identical copy of a committed
    // response. Stale path (#jb-cache-conflict-stale-accept-replay): a JB File
    // Cache Conflict accepted late replayed an *earlier draft* of the same
    // response, so the surplus block shares a committed heading topic but its
    // body drifted — `cur_set != head_set`. Both restore the committed HEAD.
    if crate::dedupe::is_committed_response_overapplication(&current, &head)
        || crate::dedupe::is_committed_response_replay_including_stale(&current, &head)
    {
        return Ok(Some(LateIpcResponseOverapplication {
            remediated_content: head.to_string(),
        }));
    }
    Ok(None)
}

fn parent_pointer_recovery_hint(file: &Path) -> String {
    format!(
        "Use `agent-doc commit {}` to finish the missing parent pointer commit, then re-run `agent-doc session-check {}`.",
        file.display(),
        file.display()
    )
}

fn short_oid(value: Option<&str>) -> String {
    value
        .map(|oid| oid.chars().take(12).collect::<String>())
        .filter(|oid| !oid.is_empty())
        .unwrap_or_else(|| "<missing>".to_string())
}

fn parent_submodule_pointer_message(
    drift: &crate::git::SubmodulePointerDrift,
    file: &Path,
) -> String {
    format!(
        "parent submodule pointer is not committed for {} (parent HEAD {}, submodule HEAD {}). The response patchback crossed the submodule repo but not the parent commit boundary. {}",
        drift.relative_path,
        short_oid(drift.parent_head.as_deref()),
        short_oid(Some(&drift.submodule_head)),
        parent_pointer_recovery_hint(file)
    )
}

fn check_parent_submodule_pointer_guard(file: &Path) -> Result<GuardResult> {
    let Some(drift) = crate::git::submodule_pointer_drift(file)? else {
        return Ok(GuardResult::None);
    };
    let msg = format!(
        "[session-check] INTERRUPTED: {}",
        parent_submodule_pointer_message(&drift, file)
    );
    eprintln!("{}", msg);
    crate::ops_log::log_op(
        file,
        &format!(
            "parent_submodule_pointer_guard_failed file={} submodule={} parent_head={} submodule_head={}",
            file.display(),
            drift.relative_path,
            short_oid(drift.parent_head.as_deref()),
            short_oid(Some(&drift.submodule_head))
        ),
    );
    Ok(GuardResult::Error(msg))
}

fn check_prompt_only_exchange_tail_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    let Some(prompt) = prompt_only_exchange_tail(&content) else {
        return Ok(GuardResult::None);
    };
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: live exchange ends with unresolved prompt-only closeout tail and no assistant response: {}. Finish the turn through `agent-doc finalize {}` or recover the missing response with `agent-doc write --commit {}` before reporting closeout success.",
        prompt,
        file.display(),
        file.display()
    )))
}

fn tracked_side_effect_paths(file: &Path) -> Result<Vec<String>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let doc_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(crate::git::tracked_modified_paths(file)?
        .into_iter()
        .filter(|path| !path.starts_with(".agent-doc/"))
        .filter(|path| path != &doc_name && !path.ends_with(&format!("/{doc_name}")))
        .collect())
}

fn tracked_side_effect_note(file: &Path) -> Result<String> {
    let mut paths = tracked_side_effect_paths(file)?;
    if paths.is_empty() {
        return Ok(String::new());
    }
    let overflow = paths.len().saturating_sub(3);
    paths.truncate(3);
    let mut note = format!("; tracked side-effect edits: {}", paths.join(", "));
    if overflow > 0 {
        note.push_str(&format!(" (+{} more)", overflow));
    }
    Ok(note)
}

/// Phase 3 (#jbccc3): JB File Cache Conflict cancel auto-recovery detection.
///
/// Returns true when the document is in the recoverable post-write pre-commit
/// shape: the cycle is at `WriteApplied` (or already-marked `Committed` whose
/// commit boundary never landed in `HEAD`), the snapshot has the visible
/// response, `HEAD` does not, and the working tree matches the snapshot
/// modulo transient `(HEAD)` / boundary markers (no live exchange edits beyond
/// the response). When this returns true, `git::commit(file)` reliably closes
/// the cycle and `session_check` must avoid misclassifying the same state as
/// a `likely_direct_response_patchback`.
///
/// See `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md` Phase 3.
pub fn detect_jb_cache_conflict_cancel_recoverable(file: &Path) -> Result<bool> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_cancel_recoverable_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_cancel_recoverable_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if !matches!(
        state.phase,
        crate::cycle_state::CyclePhase::WriteApplied | crate::cycle_state::CyclePhase::Committed
    ) {
        return Ok(false);
    }
    if !matches!(
        rc.snapshot_commit_status(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(false);
    }
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(snapshot) = rc.snapshot_content() else {
        return Ok(false);
    };
    let normalized_doc = crate::git::normalize_transient_agent_doc_markers(&doc);
    let normalized_snapshot = crate::git::normalize_transient_agent_doc_markers(&snapshot);
    Ok(normalized_doc == normalized_snapshot)
}

pub fn detect_uncommitted_closeout_drift(file: &Path) -> Result<Option<String>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_uncommitted_closeout_drift_with_context(file, &rc)
}

pub fn detect_uncommitted_closeout_drift_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<String>> {
    if crate::git::repair_committed_historical_snapshot_drift(file)?.is_some() {
        return Ok(None);
    }
    if let Some(drift) = crate::git::submodule_pointer_drift(file)? {
        return Ok(Some(parent_submodule_pointer_message(&drift, file)));
    }
    // Phase 3 (#jbccc3): jb_cache_conflict_cancel is auto-recoverable through
    // `git::commit`. Skip the lower-precision `detect_bypassed_response_write`
    // and `SnapshotDiffersFromHead` paths below so neither this caller nor
    // standalone `session-check` accuses the user of a direct patchback when
    // the binary-owned write path actually applied the response but the commit
    // boundary never landed. Preflight's `enforce_no_uncommitted_closeout_drift`
    // separately runs `git::commit` to close the cycle.
    if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
        return Ok(None);
    }
    if let Some(marker) = detect_bypassed_response_write(file)? {
        return Ok(Some(format!(
            "found likely direct response patchback without agent-doc cycle: {}{} {}",
            marker,
            tracked_side_effect_note(file)?,
            closeout_recovery_hint(file)
        )));
    }
    if let Some(marker) = detect_uncommitted_exchange_drift(file)? {
        if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
            return Ok(None);
        }
        return Ok(Some(format!(
            "document has uncommitted exchange changes beyond the committed snapshot: {}{} {}",
            marker,
            tracked_side_effect_note(file)?,
            closeout_recovery_hint(file)
        )));
    }
    match rc.snapshot_commit_status() {
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
                return Ok(None);
            }
            Ok(Some(format!(
                "snapshot differs from HEAD without an open or recoverable agent-doc cycle (snapshot_len={}, head_len={}){} {}",
                snapshot_len,
                head_len,
                tracked_side_effect_note(file)?,
                closeout_recovery_hint(file)
            )))
        }
        crate::git::SnapshotCommitStatus::Committed
        | crate::git::SnapshotCommitStatus::NoSnapshot
        | crate::git::SnapshotCommitStatus::NoHead
        | crate::git::SnapshotCommitStatus::NotInGitRepo => Ok(None),
    }
}

fn check_shadow_backlog_guard(_file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    let report = crate::pending::detect_shadow_open_items(&content)?;
    if !report.shadow_only.is_empty() {
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: {}. Re-run preflight/repair after restoring them to the live backlog or marking them complete",
            format_shadow_refs(&report.shadow_only)
        )));
    }
    if !report.duplicated_in_live_backlog.is_empty() {
        return Ok(GuardResult::Warn(vec![format!(
            "[session-check] warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        )]));
    }
    Ok(GuardResult::None)
}

fn format_shadow_refs(items: &[crate::pending::ShadowPendingItem]) -> String {
    items
        .iter()
        .map(crate::pending::ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_malformed_tracked_item_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let refs = malformed_tracked_item_refs_in(&content, &components, None);
    if refs.is_empty() {
        return Ok(GuardResult::None);
    }

    Ok(GuardResult::Error(malformed_tracked_item_message(&refs)))
}

pub fn malformed_tracked_item_refs(
    file: &Path,
    completed_by_response: Option<&str>,
) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(malformed_tracked_item_refs_in(
        &content,
        &components,
        completed_by_response,
    ))
}

/// Shared malformed-item detection over already-read content + parsed
/// components. Phase 6 (#lr-content-6) lets `inspect`'s guard read these from
/// the cached graph slots while external callers still pass a freshly read
/// document.
fn malformed_tracked_item_refs_in(
    content: &str,
    components: &[crate::component::Component],
    completed_by_response: Option<&str>,
) -> Vec<String> {
    components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let name = component.name.clone();
            crate::pending::detect_malformed_item_lines(component.content(content))
                .into_iter()
                .map(move |item| (name.clone(), item))
        })
        .filter(|(_, item)| {
            completed_by_response
                .map(|response| response_clearly_completes_pending_id(response, &item.id))
                .unwrap_or(true)
        })
        .map(|(name, item)| format!("{} {}", name, item.reference()))
        .collect::<Vec<_>>()
}

pub fn malformed_tracked_item_message(refs: &[String]) -> String {
    format!(
        "[session-check] INTERRUPTED: malformed tracked checklist item(s) in live backlog/icebox: {}. Repair the checklist prefix before closeout so pending guards can prove the item state",
        refs.join("; ")
    )
}

fn check_backlog_replay_guard(file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let hash = crate::snapshot::doc_hash(&canonical).unwrap_or_default();
    let baseline_content = crate::snapshot::find_project_root(&canonical)
        .map(|root| root.join(format!(".agent-doc/baselines/{}.md", hash)))
        .and_then(|p| std::fs::read_to_string(p).ok());

    let baseline = match baseline_content {
        Some(content) => content,
        None => match rc.head_content() {
            Some(content) => content.to_string(),
            None => return Ok(GuardResult::None),
        },
    };

    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;

    let external_done_ids = crate::preflight::external_done_archive_ids(file, &current_content)?;
    let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_done_ids,
    )?;

    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(crate::pending::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done",
            refs
        )));
    }

    Ok(GuardResult::None)
}

/// #late-ipc-patch-response-uncommitted: self-heal a late-IPC committed-response
/// over-application in place so a mutating session-check path does not stall the
/// `agent:queue` auto-loop on an unrecoverable interruption.
///
/// A wedged/slow IPC listener can apply a stale queued patch minutes late —
/// after the cycle already committed — re-adding a duplicate `### Re:` block to
/// the working tree. The real response is in HEAD, so the surplus block is pure
/// drift. `detect_late_ipc_response_overapplication` only returns `Some` when
/// restoring HEAD is provably safe (scaffold matches HEAD, every committed
/// response present unchanged, no new user directive introduced), so restoring
/// the committed HEAD only drops the duplicate — the identical remediation
/// `preflight` applies. Returns `true` when it healed.
///
/// Kept out of the read-only `inspect*` path on purpose: only the mutating
/// command entrypoints (`enforce_clean_closeout` on the finalize boundary,
/// `run_with_options` for direct-exec `agent-doc session-check`) repair in place.
fn self_heal_late_ipc_overapplication(file: &Path) -> Result<bool> {
    let Some(overapplication) = detect_late_ipc_response_overapplication(file)? else {
        return Ok(false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "late_ipc_response_overapplication_self_healed file={}",
            file.display()
        ),
    );
    eprintln!(
        "[session-check] late_ipc_overapplication: self-healing — restoring committed HEAD over the re-added duplicate response for {}",
        file.display()
    );
    crate::write::atomic_write_pub(file, &overapplication.remediated_content)?;
    crate::snapshot::save(file, &overapplication.remediated_content)?;
    Ok(true)
}

pub fn enforce_clean_closeout(file: &Path) -> Result<()> {
    self_heal_late_ipc_overapplication(file)?;
    let report = inspect_with_warnings(file)?;
    for warning in report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(_) => Ok(()),
        SessionCheckStatus::Interrupted(message) => anyhow::bail!(message),
    }
}

fn inspect_core(file: &Path) -> Result<SessionCheckStatus> {
    if let Some(replay) = detect_jb_cache_conflict_accept_duplicate_replay(file)? {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found JetBrains File Cache Conflict accept replay duplicate at `{}`; `dedupe(current)` matches committed HEAD. Run `agent-doc preflight {}` to auto-repair, or run `agent-doc dedupe {}` followed by `agent-doc write --commit {}`.",
            replay.heading,
            file.display(),
            file.display(),
            file.display()
        )));
    }

    if let Some(heading) = detect_duplicate_response_patchback(file)? {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found consecutive duplicate response patchback at `{}`. Run `agent-doc dedupe {}` or rerun closeout so the write path can repair it before commit.",
            heading,
            file.display()
        )));
    }

    // Late-IPC reposition / stale-patch replay re-inserted the committed
    // response (possibly boundary-wrapped and non-adjacent) into the working
    // tree after it already reached HEAD. Recognize it as an over-application
    // before the generic `detect_bypassed_response_write` guard accuses the
    // operator of a manual patchback. The mutating entrypoints
    // (`enforce_clean_closeout`, `run_with_options`) self-heal this in place via
    // `self_heal_late_ipc_overapplication` before reaching here, and `preflight`
    // auto-repairs by restoring HEAD; this Interrupted return is the fallback for
    // read-only inspectors (#late-ipc-patch-response-uncommitted).
    if detect_late_ipc_response_overapplication(file)?.is_some() {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found late-IPC committed-response over-application at `{}`; the working tree re-adds a response already in HEAD. Run `agent-doc preflight {}` to auto-repair (restores the committed HEAD), or `agent-doc write --commit {}` to recover through the normal closeout boundary.",
            file.display(),
            file.display(),
            file.display()
        )));
    }

    if let Some(state) = crate::cycle_state::load(file)? {
        if state.is_open() {
            if let Some(reason) = crate::repair::recover_missing_commit_boundary(
                file,
                "session_check_commit_boundary_recovered",
            )? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` was `{}` ({}), recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        phase_name(state.phase),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` was `{}` ({}); recovered the missing commit boundary from {}",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason
                )));
            }
            if let Some(message) = open_cycle_manual_patchback_message(file, &state)? {
                return Ok(SessionCheckStatus::Interrupted(message));
            }
            return Ok(SessionCheckStatus::Interrupted(open_cycle_message(
                file, &state,
            )?));
        }
        // #codex-owned-pane-prompt-miss: a recursive same-pane direct invocation
        // that abandoned its empty cycle is terminal, but that abandon is NOT
        // sufficient closeout if an unresolved exchange prompt still remains with
        // no later response — the user prompt was never answered. Report a
        // missed-prompt recovery instead of accepting the abandoned cycle as OK.
        // (Defense in depth: the run-side early guard now bails before opening a
        // cycle in this shape, but older abandoned cycles or alternate paths must
        // still be caught here.)
        if matches!(state.phase, crate::cycle_state::CyclePhase::Abandoned)
            && state
                .last_event
                .starts_with("recursive_direct_invocation_blocked")
            && let Some(unresolved) = unresolved_exchange_prompt(file)?
        {
            let excerpt: String = unresolved
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(&unresolved)
                .trim()
                .chars()
                .take(200)
                .collect();
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` was abandoned by the recursive same-pane guard ({}), but an unresolved exchange prompt is still unanswered: \"{}\". Answer it in this owner pane's current turn and persist with `agent-doc finalize {}` (or `agent-doc write --commit {}`); do not re-run `agent-doc {}` from this same pane.",
                state.cycle_id,
                state.last_event,
                excerpt,
                file.display(),
                file.display(),
                file.display()
            )));
        }
        if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
            if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason,
                    prompt_marker
                )));
            }
            return Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                reason
            )));
        }
        if let Some(marker) = detect_bypassed_response_write(file)? {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        phase_name(state.phase),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason
                )));
            }
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                marker,
                tracked_side_effect_note(file)?,
                closeout_recovery_hint(file)
            )));
        }
        if let Some(marker) = detect_active_session_post_commit_drift(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                marker
            )));
        }
        if let Some(marker) = detect_uncommitted_exchange_drift(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the document has uncommitted exchange changes beyond the committed snapshot: {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle before reporting success.",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                marker,
                file.display(),
                file.display()
            )));
        }
        if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                marker
            )));
        }
        return Ok(SessionCheckStatus::Ok(format!(
            "[session-check] ok — cycle `{}` is `{}` ({})",
            state.cycle_id,
            phase_name(state.phase),
            state.last_event
        )));
    }

    match last_ops_event(file)? {
        None => {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — repaired committed historical {} snapshot drift",
                    reason
                )));
            }
            if let Some(marker) = detect_bypassed_response_write(file)? {
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — repaired committed historical {} snapshot drift",
                        reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    tracked_side_effect_note(file)?,
                    closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = detect_uncommitted_exchange_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: document has uncommitted exchange changes beyond the committed snapshot (no cycle state): {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
                    marker,
                    file.display(),
                    file.display()
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: document has unresolved prompt-bearing user changes but no agent-doc cycle ever started: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(
                "[session-check] no cycle state or ops.log — ok".to_string(),
            ))
        }
        Some(event) if event.starts_with(PREFLIGHT_START_EVENT) => {
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — cycle started but no write/commit followed",
                PREFLIGHT_START_EVENT
            )))
        }
        Some(event) if is_write_completed_commit_missing_event(&event) => {
            if let Some(reason) = crate::repair::recover_missing_commit_boundary(
                file,
                "session_check_commit_boundary_recovered",
            )? {
                let repaired_cycle = crate::cycle_state::load(file)?;
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log entry was `{}`, recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        event_name(&event),
                        reason,
                        prompt_marker
                    )));
                }
                if let Some(state) = repaired_cycle {
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; recovered the missing commit boundary from {} into cycle `{}`",
                        event, reason, state.cycle_id
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; recovered the missing commit boundary from {}",
                    event, reason
                )));
            }
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — response write landed but no commit followed",
                event_name(&event)
            )))
        }
        Some(event) => {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                    event, reason
                )));
            }
            if let Some(marker) = detect_bypassed_response_write(file)? {
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                        event, reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    tracked_side_effect_note(file)?,
                    closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = detect_uncommitted_exchange_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the document has uncommitted exchange changes beyond the committed snapshot: {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
                    marker,
                    file.display(),
                    file.display()
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — last event: {}",
                event
            )))
        }
    }
}

fn detect_duplicate_response_patchback(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    Ok(crate::dedupe::first_duplicate_response_heading(&content))
}

fn check_pending_capture_guard(file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_capture_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() || state.had_pending_mutations {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }
    let missing_targets = crate::write::unresolved_backlog_capture_targets(file, &state);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response came from a prompt that required backlog capture in {}, but those tracked-work surfaces did not change this cycle",
            missing_targets.join(", ")
        )));
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract described at least {} distinct issue(s), but the committed response only enumerated {} explicit backlog item(s) for target(s) {}",
            expected_count,
            promised_count,
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_plan_reference_shortfall(file, &state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract required at least {} explicit plan reference(s), but the committed response only cited {} existing plan path(s)",
            expected_count, promised_count,
        )));
    }
    let missing_ids =
        crate::write::unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response promised new tracked item(s) {} for explicit backlog target(s) {}, but those ids are still missing after this cycle",
            missing_ids.join(", "),
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        return Ok(GuardResult::Error(
            "[session-check] error: committed response came from a prompt that required backlog capture, but this cycle recorded no backlog mutations and did not explicitly state that there were no actionable follow-up items"
                .to_string(),
        ));
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(GuardResult::None);
    }

    let warn_line = format!(
        "[session-check] warn: response contains ~{} recommendation-like items but no --pending-add flags were used this cycle",
        signal.estimated_count
    );
    let hint_line =
        "[session-check] hint: consider adding pending items for actionable follow-up work"
            .to_string();

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => {
            GuardResult::Warn(vec![warn_line, hint_line])
        }
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub fn resolve_pending_capture_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_capture_guard {
        return Ok(mode);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .pending_capture
        .unwrap_or_default())
}

pub fn resolve_pending_capture_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): read frontmatter from the cached `FrontmatterSlot`
    // instead of re-reading + re-parsing the document. The slot already parsed
    // `DocContentCell` (set once per inspect cycle); these guard-mode fields are
    // SSH-resolution-independent so the resolved value is identical.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.pending_capture_guard {
        return Ok(mode);
    }
    Ok(rc
        .project_config()
        .guards
        .pending_capture
        .unwrap_or_default())
}

pub fn resolve_pending_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = crate::project_config::load_project_for_doc(file)
        .guards
        .pending_done
    {
        return Ok(mode);
    }
    if fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
    {
        return Ok(crate::frontmatter::PendingCaptureGuardMode::Strict);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Warn)
}

pub fn resolve_pending_done_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): frontmatter from the cached slot, project config
    // from the cached `ProjectConfigSlot`.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = rc.project_config().guards.pending_done {
        return Ok(mode);
    }
    if fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
    {
        return Ok(crate::frontmatter::PendingCaptureGuardMode::Strict);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Warn)
}

pub fn resolve_review_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.review_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = crate::project_config::load_project_for_doc(file)
        .guards
        .review_done
    {
        return Ok(mode);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Off)
}

pub fn resolve_review_done_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): cached frontmatter + project config slots.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.review_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = rc.project_config().guards.review_done {
        return Ok(mode);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Off)
}

pub fn resolve_auto_done(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(enabled) = fm.auto_done {
        return Ok(enabled);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .auto_done
        .unwrap_or(false))
}

pub fn resolve_auto_done_with_context(_file: &Path, rc: &crate::graph::RunContext) -> Result<bool> {
    // Phase 6 (#lr-content-6): cached frontmatter + project config slots.
    let fm = rc.frontmatter();
    if let Some(enabled) = fm.auto_done {
        return Ok(enabled);
    }
    Ok(rc.project_config().guards.auto_done.unwrap_or(false))
}

fn check_pending_done_guard(file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    let missing = detect_missing_pending_done_ids(
        file,
        &response_text,
        &state.pending_done_ids,
        &state.pending_kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        hint
    );
    let warn_line = format!(
        "[session-check] warn: response appears to complete existing pending {} but no matching `--done` was recorded this cycle",
        ids
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{}` or add `pending_done_guard: off` for this document when the item should stay open",
                repair
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{}` or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
            repair
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub fn detect_missing_pending_done_ids(
    file: &Path,
    response_text: &str,
    recorded_done_ids: &[String],
    kept_open_ids: &[String],
) -> Result<Vec<String>> {
    if response_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?;
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    let recorded_done: std::collections::HashSet<String> = recorded_done_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let kept_open: std::collections::HashSet<String> = kept_open_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    Ok(open_ids
        .into_iter()
        .filter(|id| !kept_open.contains(id))
        .filter(|id| response_clearly_completes_pending_id(response_text, id))
        .filter(|id| !recorded_done.contains(id))
        .collect())
}

pub fn response_text_for_guards(response: &str) -> String {
    let Ok((patches, unmatched)) = crate::template::parse_patches(response) else {
        return response.to_string();
    };

    let preferred: Vec<String> = patches
        .iter()
        .filter(|patch| matches!(patch.name.as_str(), "exchange" | "findings"))
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !preferred.is_empty() {
        return preferred.join("\n\n");
    }

    if !unmatched.trim().is_empty() {
        return unmatched.trim().to_string();
    }

    let fallback: Vec<String> = patches
        .iter()
        .filter(|patch| {
            !is_backlog_component(&patch.name)
                && !crate::component::is_review_component(&patch.name)
        })
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !fallback.is_empty() {
        return fallback.join("\n\n");
    }

    response.to_string()
}

fn open_tracked_work_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect())
}

fn response_clearly_completes_pending_id(response_text: &str, id: &str) -> bool {
    // Completion is signalled by a response HEADING whose topic RESOLVES to
    // exactly this id — never by a bare prose citation of `#id` in the body
    // (#pending-done-guard-false-positive). Mentioning a related/residual open id
    // in prose (e.g. "relates to #foo", "fixed alongside #bar") is a reference,
    // not a completion claim; the old prose-window heuristic read those as
    // completions and forced retry-with-suppression cycles. A heading match plus
    // a completion marker still distinguishes a real completion from a
    // halt/refusal response that merely names the head (#queue-strike-on-halt).
    if !response_heading_resolves_to_pending_id(response_text, id) {
        return false;
    }
    contains_completion_marker(&response_text.to_ascii_lowercase())
}

/// True when some `### Re:` response heading's topic resolves to `#id`. A
/// batch `do [#a] [#b] …` directive heading resolves to every bracketed id; a
/// titled `#id descriptive text` heading resolves only to its LEADING id (the
/// trailing words are prose). A heading that merely contains `#id` later in
/// descriptive prose — and any `#id` cited in the response BODY — never
/// resolves to it. This mirrors the exact-id queue-consume matching.
fn response_heading_resolves_to_pending_id(response_text: &str, id: &str) -> bool {
    let id_lower = id.to_ascii_lowercase();
    for raw in response_text.lines() {
        let line = raw.trim().to_ascii_lowercase();
        let Some(after) = line.strip_prefix('#') else {
            continue;
        };
        let heading = after.trim_start_matches('#').trim_start();
        let Some(topic) = heading.strip_prefix("re:") else {
            continue;
        };
        let topic = topic.split(" — ").next().unwrap_or(topic).trim();
        if let Some(do_list) = topic.strip_prefix("do ") {
            // Batch do-directive: every bracketed `[#id]` is a completion target.
            let bracket_ids = extract_bracket_ids(do_list);
            if !bracket_ids.is_empty() {
                if bracket_ids.iter().any(|b| b == &id_lower) {
                    return true;
                }
                continue;
            }
            // No brackets — a single `do #id` form; leading id only.
            if leading_hash_id(do_list).as_deref() == Some(id_lower.as_str()) {
                return true;
            }
        } else if leading_hash_id(topic).as_deref() == Some(id_lower.as_str()) {
            return true;
        }
    }
    false
}

/// The leading `#id` token of `text` (optionally `[`-wrapped), or `None`.
fn leading_hash_id(text: &str) -> Option<String> {
    let t = text.strip_prefix('[').unwrap_or(text);
    let rest = t.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    (!id.is_empty()).then_some(id)
}

/// All `[#id]` bracketed ids appearing in `text`, in order.
fn extract_bracket_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("[#") {
        let after = &rest[pos + 2..];
        let id: String = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
        let consumed = id.len();
        if !id.is_empty() {
            out.push(id);
        }
        rest = &after[consumed..];
    }
    out
}

fn contains_completion_marker(text: &str) -> bool {
    [
        "implemented",
        "fixed",
        "done.",
        "done ",
        "completed",
        "updated",
        "verification:",
        "verified",
        "pushed",
        "commit:",
        "outcome:",
        "what changed:",
        "landed",
        "shipped",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

pub fn inline_done_signal_ids(
    file: &Path,
    prompt_texts: &[String],
    auto_done: bool,
) -> Result<Vec<String>> {
    if prompt_texts.is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    let single_review_id = if auto_done {
        single_open_review_item_id(file)?
    } else {
        None
    };
    let mut ids = Vec::new();

    for prompt in prompt_texts {
        for id in explicit_done_signal_ids(prompt) {
            if open_ids.contains(&id) && !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }

        if auto_done
            && plain_done_signal(prompt)
            && let Some(id) = single_review_id.as_deref()
            && open_ids.contains(id)
            && !ids.iter().any(|existing| existing == id)
        {
            ids.push(id.to_string());
        }
    }

    Ok(ids)
}

fn explicit_done_signal_ids(text: &str) -> Vec<String> {
    let normalized = normalize_done_signal_text(text);
    if normalized.is_empty() {
        return Vec::new();
    }

    let lower = normalized.to_ascii_lowercase();
    let is_done_signal = lower.contains(" done")
        || lower.ends_with(" done")
        || lower.starts_with("done ")
        || lower.contains(" complete")
        || lower.ends_with(" complete")
        || lower.starts_with("complete ")
        || lower.contains(" completed")
        || lower.ends_with(" completed")
        || lower.starts_with("completed ")
        || lower.contains(" resolved")
        || lower.ends_with(" resolved")
        || lower.starts_with("resolved ");
    if !is_done_signal {
        return Vec::new();
    }

    extract_pending_hash_ids(&normalized)
}

fn plain_done_signal(text: &str) -> bool {
    let normalized = normalize_done_signal_text(text);
    let lower = normalized.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "done"
            | "done."
            | "complete"
            | "complete."
            | "completed"
            | "completed."
            | "resolved"
            | "resolved."
    )
}

fn normalize_done_signal_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('❯')
        .trim()
        .trim_start_matches("- ")
        .trim()
        .to_string()
}

fn extract_pending_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if ch != '#' {
            idx += 1;
            continue;
        }

        let start = byte_idx + ch.len_utf8();
        let mut end = start;
        let mut cursor = idx + 1;
        while cursor < chars.len() {
            let (next_byte, next_ch) = chars[cursor];
            if next_ch.is_ascii_alphanumeric() || next_ch == '-' || next_ch == '_' {
                end = next_byte + next_ch.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        if end > start {
            let id = crate::pending::normalize_pending_id(&text[start..end]);
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
        idx = cursor.max(idx + 1);
    }

    ids
}

/// `#do-id-closeout-open-backlog`: extract the tracked-work ids named by an
/// explicit `do [#id]` / `do #id` prompt directive. Mirrors the binary-side
/// `tsift_graph::extract_do_targets` normalization (strip leading `❯`, an
/// optional bracketed annotation prefix like `[id]`, then require a `do `
/// prefix) so preflight can record the closeout expectation for those ids.
pub fn do_directive_target_ids(prompt_texts: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for prompt in prompt_texts {
        for line in prompt.lines() {
            for id in do_directive_target_ids_in_line(line) {
                if !ids.iter().any(|existing| existing == &id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

fn do_directive_target_ids_in_line(line: &str) -> Vec<String> {
    let mut normalized = line.trim().trim_start_matches('❯').trim();
    normalized = normalized
        .strip_prefix("- ")
        .or_else(|| normalized.strip_prefix("* "))
        .or_else(|| normalized.strip_prefix("+ "))
        .unwrap_or(normalized)
        .trim();
    // Optional-`do` Stage 2: a `re [#id]` / `re #id` reference never targets a
    // tracked id — it is inert (no execute, no reap). Skip it before any id
    // extraction so the closeout guards do not expect a reference to be resolved.
    let lower_full = normalized.to_ascii_lowercase();
    if let Some(after_re) = lower_full.strip_prefix("re ") {
        let after_re = after_re.trim_start();
        if after_re.starts_with("[#") || after_re.starts_with('#') {
            return Vec::new();
        }
    }
    // Strip a leading non-id annotation prefix (`[label]`), but NOT a bare id
    // token `[#id]` — under the optional-`do` grammar that token IS the directive.
    if normalized.starts_with('[')
        && !normalized.starts_with("[#")
        && let Some(closing) = normalized.find(']')
    {
        normalized = normalized[closing + 1..].trim();
    }
    let lower = normalized.to_ascii_lowercase();
    // Explicit `do ` prefix keeps its original contract: extract every id target
    // named after the verb (e.g. `do [#a] then [#b]`).
    if let Some(rest) = lower.strip_prefix("do ") {
        return extract_pending_hash_ids(rest);
    }
    // Stage 2: the `do` verb is optional — a bare leading `[#id]` / `#id` token
    // is id-backed. A trailing `:` (`[#id]: note`) keeps the line inert prose.
    if leads_with_bare_id_token(&lower) {
        return extract_pending_hash_ids(&lower);
    }
    Vec::new()
}

/// Optional-`do` Stage 2: true when a normalized directive head leads with a
/// bare id token (`[#id]` or `#id`) that should execute / reap id-backed. A
/// trailing `:` after the token marks prose, not a directive (`[#id]: note`).
/// `lower` is expected lowercased and marker-stripped.
fn leads_with_bare_id_token(lower: &str) -> bool {
    let (rest, bracketed) = if let Some(r) = lower.strip_prefix("[#") {
        (r, true)
    } else if let Some(r) = lower.strip_prefix('#') {
        (r, false)
    } else {
        return false;
    };
    let id_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .count();
    if id_len == 0 {
        return false;
    }
    let after = &rest[id_len..];
    if bracketed {
        match after.strip_prefix(']') {
            Some(tail) => !tail.starts_with(':'),
            None => false,
        }
    } else {
        after.is_empty() || after.starts_with([' ', '\t', '.'])
    }
}

/// Open (`[ ]`/gated, not done) ids that live specifically in the live
/// `agent:backlog` component. The `expect_done_or_gate` guard keys off backlog
/// membership: `--done`, `--pending-gate`, reap, and icebox moves all remove an
/// id from `agent:backlog`, so an id still present here was never given a
/// lifecycle outcome.
fn open_backlog_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_backlog_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .filter(|id| !id.is_empty())
        .collect())
}

/// `#do-id-closeout-open-backlog`: a resolved `do [#id]` directive must end with
/// an explicit lifecycle outcome for its target id. If the cycle committed a
/// response (queue cleared, status updated) but the target id is still open in
/// `agent:backlog` and was not recorded as done / kept-open / reaped this cycle,
/// fail closed so the directive cannot silently leave its target `[ ]`.
fn check_expect_done_or_gate_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    // Only enforce once the cycle has closed with a committed response. An open
    // cycle is still mid-flight; a no-response commit never sets `capture_id`.
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let resolved: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .chain(state.pending_kept_open_ids.iter())
        .chain(state.reaped_pending_ids.iter())
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();

    let mut unresolved: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if resolved.contains(&id) {
            continue;
        }
        if !open_backlog.contains(&id) {
            continue;
        }
        if !unresolved.iter().any(|existing| existing == &id) {
            unresolved.push(id);
        }
    }

    if unresolved.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = unresolved
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let done_hint = unresolved
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        done_hint
    );
    let warn_line = format!(
        "[session-check] warn: `do #id` directive resolved this cycle but tracked target {} is still open in agent:backlog with no `--done`, `--pending-gate`, or kept-open edit recorded",
        ids
    );

    crate::ops_log::log_op(
        file,
        &format!(
            "expect_done_or_gate_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{}`, run `--pending-gate <id>` if review/external validation remains, or add `pending_done_guard: off` when the item should stay open",
                repair
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{}`, run `--pending-gate <id>` if review/external validation remains, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            repair
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `do [#id]` target ids present in a committed document's `agent:queue`
/// component. Used by `#queue-clear-unrun-items` to decide which recorded
/// preflight heads are still queued (preserved) vs removed this cycle.
fn committed_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = crate::component::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    do_directive_target_ids(&[queue.content(content).to_string()])
}

/// `do [#id]` target ids for the current live queue head only.
fn committed_current_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = crate::component::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let entries = crate::queue::parse(queue.content(content)).unwrap_or_default();
    let Some(head) = crate::queue::first_prompt(&entries) else {
        return Vec::new();
    };
    do_directive_target_ids(std::slice::from_ref(&head.text))
}

/// `#queue-clear-unrun-items`: an active `agent:queue` head is executable user
/// intent. A closeout / reset / commit may delete a runnable `do [#id]` head
/// only with durable proof that it was consumed (this cycle's directive target,
/// owned by `#do-id-closeout-open-backlog`), resolved (its `#id` left
/// `agent:backlog` via done/gate/reap), or removed by an explicit user edit.
/// When a head present in the visible queue at preflight disappears from the
/// committed queue while its `#id` is STILL OPEN in `agent:backlog` and the
/// cycle never targeted it, fail closed and name each lost id so the queue can
/// be restored. Suppress an intentional user removal with
/// `<!-- no-queue-removal-guard -->`.
fn check_queue_head_removal_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    // Only enforce on a committed closeout; an open cycle is still mid-flight and
    // may not have written the final queue yet.
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let recorded_ids = do_directive_target_ids(&state.active_queue_heads);
    if recorded_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    // Explicit user removal already reconciled — do not second-guess it.
    if content.contains("<!-- no-queue-removal-guard -->") {
        return Ok(GuardResult::None);
    }

    let still_queued: std::collections::HashSet<String> = committed_queue_head_ids(&content)
        .into_iter()
        .map(|id| crate::pending::normalize_pending_id(&id))
        .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();
    // Lifecycle proof: ids the cycle explicitly resolved (done/reaped/gated) or
    // chose to keep open via an explicit edit. A done/gate/reap also removes the
    // id from `open_backlog`, so this set is a defensive superset.
    let mut resolved: std::collections::HashSet<String> =
        crate::cycle_state::resolved_pending_ids(file)?;
    resolved.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| crate::pending::normalize_pending_id(id)),
    );
    // This cycle's `do [#id]` directive targets are owned by the
    // `expect_done_or_gate` guard, which reports the open-target class with a
    // more specific repair. Skip them here to avoid double-firing.
    let directive_targets: std::collections::HashSet<String> = state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .collect();

    let mut lost: Vec<String> = Vec::new();
    for id in recorded_ids {
        let norm = crate::pending::normalize_pending_id(&id);
        if norm.is_empty() {
            continue;
        }
        if still_queued.contains(&norm) {
            continue; // head preserved in the committed queue
        }
        if !open_backlog.contains(&norm) {
            continue; // backlog item resolved / removed → deletion proven
        }
        if resolved.contains(&norm) || directive_targets.contains(&norm) {
            continue; // explicit lifecycle proof or sibling-owned target
        }
        if !lost.iter().any(|existing| existing == &norm) {
            lost.push(norm);
        }
    }

    if lost.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = lost
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_head_removal_guard_fired file={} lost={}",
            file.display(),
            lost.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: runnable agent:queue head(s) {} were removed from the committed queue but their backlog item(s) are still open in agent:backlog, and the cycle never consumed, completed, gated, or reaped them — unrun queue work was silently dropped",
        ids
    );
    let repair = format!(
        "restore the dropped head(s) to `agent:queue` (or resolve each id with `--done`/`--pending-gate`), then re-run `agent-doc write --commit {}`; add `<!-- no-queue-removal-guard -->` to the response if the removal was an explicit user edit",
        file.display()
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #queue-clear-unrun-items)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #queue-clear-unrun-items)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `#lr-queue-patchback-miss`: require committed-response / deferral
/// proof for each free-text (non-`do [#id]`) queue head recorded at preflight.
/// Free-text heads have no backlog id, so the guard checks that: (a) the head
/// text is still present in the committed queue (deferral / not yet consumed),
/// or (b) a committed `### Re:` response exists that plausibly answers it. A
/// binary consume marker by itself is not proof: the answer must be visible in
/// committed `agent:exchange` history, normally via the queue-prompt echo.
fn check_free_text_queue_head_provenance(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_free_text_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let content = rc.doc_content();
    if content.contains("<!-- no-free-text-queue-head-guard -->") {
        return Ok(GuardResult::None);
    }
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(GuardResult::None);
    };
    let committed_queue_text: String = components
        .iter()
        .find(|c| c.name == "queue")
        .map(|c| c.content(&content).to_string())
        .unwrap_or_default();
    let mut unresolved: Vec<String> = Vec::new();
    for head in &state.active_free_text_queue_heads {
        let normalized = head.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if committed_queue_text
            .to_ascii_lowercase()
            .contains(&normalized)
        {
            continue;
        }
        if response_head_plausibly_answers(&content, head) {
            continue;
        }
        unresolved.push(head.clone());
    }
    if unresolved.is_empty() {
        return Ok(GuardResult::None);
    }
    let heads_text = unresolved
        .iter()
        .map(|h| format!("\"{}\"", h))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "free_text_queue_head_provenance_guard_fired file={} unresolved={}",
            file.display(),
            heads_text
        ),
    );
    let warn_line = format!(
        "[session-check] warn: free-text agent:queue head(s) {heads_text} were seen at preflight but have no committed response/echo or explicit deferral proof in the closeout — the prompt may have been silently lost"
    );
    let repair = format!(
        "either respond to the unresolved head(s) and run `agent-doc finalize {}`, or add `<!-- no-free-text-queue-head-guard -->` if the removal was intentional",
        file.display()
    );
    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #lr-queue-patchback-miss)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #lr-queue-patchback-miss)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

fn response_head_plausibly_answers(content: &str, head: &str) -> bool {
    let head_words: Vec<&str> = head
        .split_whitespace()
        .filter(|w| {
            w.len() > 3
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "the"
                        | "this"
                        | "that"
                        | "with"
                        | "from"
                        | "also"
                        | "does"
                        | "what"
                        | "when"
                        | "how"
                )
        })
        .collect();
    if head_words.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    let mut matched = 0;
    for word in &head_words {
        if lower.contains(&word.to_ascii_lowercase()) {
            matched += 1;
        }
    }
    matched * 2 >= head_words.len()
}

/// Tight list of "deferred live work" phrases that, combined with a shipped
/// signal, indicate a `do [#id]` turn shipped a repo phase but left live
/// deploy / sync / verification / approval work for a later phase
/// (`#do-id-partial-closeout-state`). Kept narrow to avoid false positives on
/// ordinary closeout prose.
const PARTIAL_CLOSEOUT_REMAINING_PHRASES: &[&str] = &[
    "not deployed",
    "not yet deployed",
    "deploy remains",
    "deployment remains",
    "deploy/",
    "live verification",
    "live verify",
    "live-verify",
    "external validation remains",
    "awaiting approval",
    "awaiting user",
    "user approval",
    "sync remains",
    "feed sync",
    "merchant center",
    "live ads",
    "remains: deploy",
];

fn text_has_shipped_signal(lower: &str) -> bool {
    (lower.contains("committed")
        || lower.contains("commit + push")
        || lower.contains("commit and push"))
        && lower.contains("push")
}

fn text_has_partial_remaining_signal(lower: &str) -> bool {
    PARTIAL_CLOSEOUT_REMAINING_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// `#do-id-partial-closeout-state`: a `do [#id]` turn that ships a repo phase
/// (committed + pushed) while the response also says deploy / sync / live
/// verification / approval remains, and the directed id is still `[ ]` open,
/// should narrow the visible backlog item + queue head to the next phase via
/// `--pending-edit` (or `--pending-gate`) so the next required action is visible
/// instead of leaving the original full-task text. This is WARN-only by design:
/// it must never block the closeout (the auto-queue drain depends on this path),
/// and lacks per-id edit tracking, so it advises rather than enforces. Suppress
/// with a `<!-- no-partial-closeout-guard -->` marker in the response when the
/// item was already narrowed.
fn check_partial_closeout_state_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() || state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-partial-closeout-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !(text_has_shipped_signal(&lower) && text_has_partial_remaining_signal(&lower)) {
        return Ok(GuardResult::None);
    }

    // Only directed ids that are still open in agent:backlog and not resolved
    // (done/reaped) this cycle are candidates for next-phase narrowing.
    let resolved: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .chain(state.reaped_pending_ids.iter())
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();

    let mut candidates: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if resolved.contains(&id) || !open_backlog.contains(&id) {
            continue;
        }
        if !candidates.iter().any(|existing| existing == &id) {
            candidates.push(id);
        }
    }
    if candidates.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = candidates
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let edit_hint = candidates
        .iter()
        .map(|id| format!("--pending-edit \"{}=<remaining next-phase scope>\"", id))
        .collect::<Vec<_>>()
        .join(" ");

    crate::ops_log::log_op(
        file,
        &format!(
            "partial_closeout_state_guard_fired file={} candidates={}",
            file.display(),
            candidates.join(",")
        ),
    );

    Ok(GuardResult::Warn(vec![
        format!(
            "[session-check] warn: partial `do [#id]` closeout — work shipped (committed + pushed) but the response says live deploy/sync/verification work remains, yet tracked target {} still carries its original full-task text in agent:backlog",
            ids
        ),
        format!(
            "[session-check] hint: narrow the backlog item + queue head to the next phase with `{}` (or `--pending-gate <id>` if only review/external validation remains), or add `<!-- no-partial-closeout-guard -->` when it is already narrowed",
            edit_hint
        ),
    ]))
}

#[derive(Debug, Clone)]
struct PartialStagingFinding {
    repo: std::path::PathBuf,
    committed_paths: Vec<String>,
    dirty_paths: Vec<String>,
    literals: Vec<String>,
}

/// `#partial-staging-closeout-guard`: a manual repo commit can accidentally
/// stage only the source half of a source+test change. Local verification then
/// passes against the dirty worktree while CI sees only the partial commit.
/// This guard is WARN-only and narrow: it requires a latest-commit source/test
/// path relationship plus overlapping changed string literals in tracked
/// uncommitted or staged companion changes.
fn check_partial_staging_closeout_guard(file: &Path) -> Result<GuardResult> {
    let findings = partial_staging_closeout_findings(file)?;
    if findings.is_empty() {
        return Ok(GuardResult::None);
    }

    let mut lines = Vec::new();
    for finding in findings.iter().take(3) {
        crate::ops_log::log_op(
            file,
            &format!(
                "partial_staging_closeout_guard_fired file={} repo={} committed_paths={} dirty_paths={} literals={}",
                file.display(),
                finding.repo.display(),
                finding.committed_paths.join(","),
                finding.dirty_paths.join(","),
                finding.literals.join("|")
            ),
        );
        lines.push(format!(
            "[session-check] warn: possible partial staging closeout in {} — latest commit changed {}, but tracked uncommitted companion changes remain in {} with overlapping changed string literal(s): {}.",
            finding.repo.display(),
            preview_items(&finding.committed_paths, 4),
            preview_items(&finding.dirty_paths, 4),
            preview_items(&finding.literals, 3)
        ));
    }
    if findings.len() > 3 {
        lines.push(format!(
            "[session-check] warn: {} additional partial staging candidate(s) omitted.",
            findings.len() - 3
        ));
    }
    lines.push(
        "[session-check] hint: commit the companion changes, revert them, or rerun verification against the committed tree before reporting CI-ready closeout."
            .to_string(),
    );
    Ok(GuardResult::Warn(lines))
}

fn partial_staging_closeout_findings(file: &Path) -> Result<Vec<PartialStagingFinding>> {
    let mut findings = Vec::new();
    for repo in partial_staging_candidate_repos(file)? {
        if let Some(finding) = partial_staging_finding_for_repo(&repo)? {
            findings.push(finding);
        }
    }
    Ok(findings)
}

fn partial_staging_candidate_repos(file: &Path) -> Result<Vec<std::path::PathBuf>> {
    let start = if file.is_dir() {
        file
    } else {
        file.parent().unwrap_or_else(|| Path::new("."))
    };
    let Some(root) = git_toplevel(start)? else {
        return Ok(Vec::new());
    };

    let mut repos = vec![root.clone()];
    if let Some(status) = git_stdout(
        &root,
        &["status", "--porcelain=v1", "--ignore-submodules=none"],
    )? {
        for line in status.lines() {
            let Some(rel) = parse_porcelain_path(line) else {
                continue;
            };
            let candidate = root.join(rel);
            if !candidate.is_dir() {
                continue;
            }
            if let Some(subroot) = git_toplevel(&candidate)?
                && subroot != root
            {
                repos.push(subroot);
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}

fn partial_staging_finding_for_repo(repo: &Path) -> Result<Option<PartialStagingFinding>> {
    if git_stdout(repo, &["rev-parse", "--verify", "HEAD^"])?.is_none() {
        return Ok(None);
    }

    let committed_paths = git_name_lines(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=ACMRT",
            "HEAD^",
            "HEAD",
        ],
    )?
    .into_iter()
    .filter(|path| is_partial_staging_relevant_path(path))
    .collect::<Vec<_>>();
    if committed_paths.is_empty() {
        return Ok(None);
    }

    let mut dirty_paths = git_name_lines(repo, &["diff", "--name-only", "--diff-filter=ACMRT"])?;
    dirty_paths.extend(git_name_lines(
        repo,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMRT"],
    )?);
    dirty_paths = dirty_paths
        .into_iter()
        .filter(|path| is_partial_staging_relevant_path(path))
        .collect::<Vec<_>>();
    dirty_paths.sort();
    dirty_paths.dedup();
    if dirty_paths.is_empty() || !partial_staging_paths_look_related(&committed_paths, &dirty_paths)
    {
        return Ok(None);
    }

    let committed_diff =
        git_stdout(repo, &["diff", "--unified=0", "HEAD^", "HEAD"])?.unwrap_or_default();
    let mut dirty_diff = git_stdout(repo, &["diff", "--unified=0"])?.unwrap_or_default();
    if let Some(cached) = git_stdout(repo, &["diff", "--cached", "--unified=0"])? {
        if !dirty_diff.is_empty() && !cached.is_empty() {
            dirty_diff.push('\n');
        }
        dirty_diff.push_str(&cached);
    }

    let committed_literals = extract_changed_string_literals(&committed_diff);
    let dirty_literals = extract_changed_string_literals(&dirty_diff);
    let mut overlap = committed_literals
        .intersection(&dirty_literals)
        .cloned()
        .collect::<Vec<_>>();
    overlap.sort();
    if overlap.is_empty() {
        return Ok(None);
    }

    Ok(Some(PartialStagingFinding {
        repo: repo.to_path_buf(),
        committed_paths,
        dirty_paths,
        literals: overlap,
    }))
}

fn git_toplevel(start: &Path) -> Result<Option<std::path::PathBuf>> {
    let Some(stdout) = git_stdout(start, &["rev-parse", "--show-toplevel"])? else {
        return Ok(None);
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(std::path::PathBuf::from(trimmed)))
}

fn git_name_lines(repo: &Path, args: &[&str]) -> Result<Vec<String>> {
    let Some(stdout) = git_stdout(repo, args)? else {
        return Ok(Vec::new());
    };
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?} in {}", args, repo.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

fn parse_porcelain_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let status = &line[..2];
    if status == "??" {
        return None;
    }
    let raw = line[3..].trim();
    if raw.is_empty() {
        return None;
    }
    let path = raw.rsplit(" -> ").next().unwrap_or(raw).trim();
    Some(path.trim_matches('"').to_string())
}

fn is_partial_staging_relevant_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with(".agent-doc/")
        || normalized.starts_with(".git/")
        || normalized.ends_with(".lock")
    {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    let Some(ext) = lower.rsplit('.').next() else {
        return false;
    };
    matches!(
        ext,
        "rs" | "kt"
            | "kts"
            | "java"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "go"
            | "rb"
            | "swift"
            // `#partial-staging-guard-cross-doc-noise`: markdown is NOT source/test
            // code. In a multi-session superproject the latest commit is usually a
            // session DOCUMENT (`.md`) and the dirty companions are other session
            // docs; their shared prose vocabulary (e.g. "make check", "agent-doc")
            // is incidental, not a source+test partial-staging signal, so including
            // `md` made the guard WARN on nearly every closeout. The guard targets
            // source+test code partial staging; documents are excluded.
            | "txt"
            | "snap"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
    )
}

fn partial_staging_paths_look_related(committed: &[String], dirty: &[String]) -> bool {
    if committed
        .iter()
        .any(|committed_path| dirty.iter().any(|dirty_path| dirty_path == committed_path))
    {
        return true;
    }
    let dirty_has_test = dirty.iter().any(|path| path_looks_test_like(path));
    let committed_has_source = committed.iter().any(|path| !path_looks_test_like(path));
    dirty_has_test && committed_has_source
}

fn path_looks_test_like(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.starts_with("test/")
        || lower.contains("/test/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
        || lower.ends_with(".snap")
        || lower
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains("test"))
}

fn extract_changed_string_literals(diff: &str) -> std::collections::BTreeSet<String> {
    let mut literals = std::collections::BTreeSet::new();
    for line in diff.lines() {
        if !(line.starts_with('+') || line.starts_with('-'))
            || line.starts_with("+++")
            || line.starts_with("---")
        {
            continue;
        }
        for literal in extract_string_literals_from_line(&line[1..]) {
            if interesting_changed_literal(&literal) {
                literals.insert(literal);
            }
        }
    }
    literals
}

fn extract_string_literals_from_line(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' && ch != '`' {
            continue;
        }
        let quote = ch;
        let mut escaped = false;
        let mut literal = String::new();
        for next in chars.by_ref() {
            if escaped {
                literal.push(next);
                escaped = false;
                continue;
            }
            if quote == '"' && next == '\\' {
                escaped = true;
                continue;
            }
            if next == quote {
                break;
            }
            literal.push(next);
        }
        result.push(literal);
    }
    result
}

fn interesting_changed_literal(literal: &str) -> bool {
    let trimmed = literal.trim();
    trimmed.len() >= 4 && trimmed.chars().any(|ch| ch.is_ascii_alphanumeric())
}

fn preview_items(items: &[String], limit: usize) -> String {
    let mut preview = items
        .iter()
        .take(limit)
        .map(|item| format!("`{}`", item))
        .collect::<Vec<_>>();
    if items.len() > limit {
        preview.push(format!("...(+{})", items.len() - limit));
    }
    preview.join(", ")
}

/// Tight list of "blocked / still needs future action" phrases that, combined
/// with a directed id gated this cycle, indicate a `do [#id]` closeout reported
/// the work is still incomplete and needs more agent execution — distinct from
/// a clean implementation-complete review gate. Kept narrow (no bare "blocked"
/// or generic "requires"/"until") so ordinary review/closeout prose does not
/// trip the guard.
const BLOCKED_FUTURE_ACTION_PHRASES: &[&str] = &[
    "remains blocked",
    "still blocked",
    "is blocked",
    "are blocked",
    "blocked on",
    "blocked by",
    "blocked:",
    "blocked until",
    "next step to complete",
    "next steps to complete",
    "steps to complete",
    "cannot complete until",
    "can't complete until",
    "still needs to",
    "still need to",
    "must remove",
    "must delete",
    "must expire",
    "needs to be removed",
    "no live cutover",
    "waiting on approval",
    "awaiting approval",
    "needs approval before",
    "requires approval before",
    "deliberately delete",
    "get approval",
];

/// Explicit "no follow-up needed" justifications that satisfy a blocked-shape
/// closeout for a genuinely review-only gate. Keep aligned with the repair hint.
const NO_FOLLOWUP_JUSTIFICATION_PHRASES: &[&str] = &[
    "no additional backlog follow-up",
    "no additional follow-up is needed",
    "no follow-up backlog",
    "no further backlog",
    "no actionable backlog follow-up",
    "no remaining backlog work",
];

fn text_has_blocked_future_action_signal(lower: &str) -> bool {
    BLOCKED_FUTURE_ACTION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

fn text_has_no_followup_justification(lower: &str) -> bool {
    NO_FOLLOWUP_JUSTIFICATION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// True when a blocked / still-needed-work phrase co-occurs with `#id` inside
/// the same paragraph of the response. Paragraph-scoping (blank-line separated)
/// keeps an incidental blocked phrase about unrelated work — or the `### Re:`
/// heading that always names the id — from tying the signal to the directed id.
fn blocked_signal_tied_to_id(text: &str, id: &str) -> bool {
    let needle = format!("#{}", id.to_ascii_lowercase());
    text.split("\n\n").any(|paragraph| {
        let lower = paragraph.to_ascii_lowercase();
        lower.contains(&needle) && text_has_blocked_future_action_signal(&lower)
    })
}

/// Open (`[ ]`/gated, not done) ids that currently live in a `review`/gated
/// component. Used to confirm a directed id gated this cycle is still gated
/// (not subsequently un-gated or completed) before the blocked-closeout guard
/// fires.
fn open_review_ids(file: &Path) -> Result<std::collections::HashSet<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(std::collections::HashSet::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| crate::component::is_review_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| crate::pending::normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect())
}

/// `#blocked-closeout-followup-capture`: a directed `do [#id]` cycle that moves
/// its target to the review/gated component (`--pending-gate`) while the
/// response says the work is blocked / still needs future action must capture an
/// actionable follow-up before clean closeout — otherwise the document explains
/// the blocker but the active backlog no longer drives the remaining work.
///
/// Satisfied by any of: keeping the id open in `agent:backlog`
/// (`--pending-edit <id>=...`), adding a new follow-up item (`--pending-add*`),
/// or an explicit no-follow-up justification phrase in the response. A
/// `<!-- no-blocked-followup-guard -->` marker also suppresses it.
fn check_blocked_closeout_followup_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty()
        || state.pending_gated_ids.is_empty()
        || state.is_open()
    {
        return Ok(GuardResult::None);
    }
    // A new follow-up backlog/review item was captured this cycle — satisfied.
    if state.pending_added_this_cycle {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-blocked-followup-guard -->")
        || capture
            .response_body
            .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !text_has_blocked_future_action_signal(&lower) {
        return Ok(GuardResult::None);
    }
    if text_has_no_followup_justification(&lower) {
        return Ok(GuardResult::None);
    }

    let kept_open: std::collections::HashSet<String> = state
        .pending_kept_open_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let done: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let gated: std::collections::HashSet<String> = state
        .pending_gated_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let still_gated = open_review_ids(file)?;

    let mut unresolved: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if kept_open.contains(&id) || done.contains(&id) {
            continue;
        }
        if !gated.contains(&id) || !still_gated.contains(&id) {
            continue;
        }
        // Tie the blocked signal to the directed id (same paragraph) so an
        // incidental blocked phrase about unrelated work does not fire.
        if !blocked_signal_tied_to_id(&text, &id) {
            continue;
        }
        if !unresolved.iter().any(|existing| existing == &id) {
            unresolved.push(id);
        }
    }

    if unresolved.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = unresolved
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let edit_hint = unresolved
        .iter()
        .map(|id| format!("--pending-edit \"{}=<remaining next action>\"", id))
        .collect::<Vec<_>>()
        .join(" ");
    let add_after_hint = unresolved
        .first()
        .map(|id| format!("--pending-add-after {} \"<id>=<concrete next step>\"", id))
        .unwrap_or_default();
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        edit_hint
    );
    let warn_line = format!(
        "[session-check] warn: `do #id` closeout reported blocked / still-needed work but gated tracked target {} out of agent:backlog with no kept-open edit, new follow-up item, or explicit no-follow-up justification — the remaining steps live only in prose",
        ids
    );

    crate::ops_log::log_op(
        file,
        &format!(
            "blocked_closeout_followup_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: keep the work tracked with `{}`, split a new follow-up via `{}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or add `<!-- no-blocked-followup-guard -->`",
                repair, add_after_hint
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: keep the work tracked with `{}`, split a new follow-up via `{}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            repair,
            add_after_hint
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `#gated-followup-split-enforcement`: when a directed `do [#id]` cycle keeps a
/// multi-phase item open (via `--pending-edit` / `--review-edit` /
/// `--pending-gate`) whose body enumerates several gated/remaining phases but
/// never breaks them out into discrete child backlog IDs, the deferred phases
/// stay buried in one parent's narrowed description and are not independently
/// trackable or queueable. Advise splitting each phase into its own child ID
/// (sibling of `#blocked-closeout-followup-capture` and the SKILL "one backlog
/// ID per actionable phase" rule).
///
/// Warn-first advisory only — it never blocks closeout. Suppressible via a
/// `<!-- no-gated-phase-split-guard -->` response marker.
fn check_gated_phase_split_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() || state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-gated-phase-split-guard -->")
    {
        return Ok(GuardResult::None);
    }

    // Items kept open this cycle (`--pending-edit` / `--review-edit` /
    // `--pending-gate` all feed `pending_kept_open_ids`) that were also the
    // directed targets — the parent items at risk of burying gated phases.
    let kept_open: std::collections::HashSet<String> = state
        .pending_kept_open_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    if kept_open.is_empty() {
        return Ok(GuardResult::None);
    }
    let directed: std::collections::HashSet<String> = state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let mut flagged: Vec<String> = Vec::new();
    for component in components.iter() {
        let trackable = crate::component::is_backlog_component(&component.name)
            || crate::component::is_review_component(&component.name);
        if !trackable {
            continue;
        }
        let (_, items, _) = crate::pending::parse_items(component.content(&content));
        for item in items {
            if item.is_done() {
                continue;
            }
            let id = crate::pending::normalize_pending_id(&item.id);
            if id.is_empty() || !kept_open.contains(&id) || !directed.contains(&id) {
                continue;
            }
            let body = format!("{} {}", item.text, item.continuation);
            if body_enumerates_multiple_gated_phases(&body)
                && !body_already_split_into_child_ids(&body, &id)
                && !flagged.iter().any(|existing| existing == &id)
            {
                flagged.push(id);
            }
        }
    }
    if flagged.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = flagged
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let add_after_hint = flagged
        .first()
        .map(|id| format!("--pending-add-after {id} \"<child-id>=<one phase scope>\""))
        .unwrap_or_default();

    crate::ops_log::log_op(
        file,
        &format!(
            "gated_phase_split_guard_fired file={} flagged={}",
            file.display(),
            flagged.join(",")
        ),
    );

    Ok(GuardResult::Warn(vec![
        format!(
            "[session-check] warn: kept-open tracked item {ids} enumerates multiple gated/remaining phases in its body but does not break them out into discrete child backlog IDs — the deferred phases are not independently trackable or queueable"
        ),
        format!(
            "[session-check] hint: split each gated phase into its own child id (e.g. `agent-doc write {} {} --pending-only --commit`), keeping the parent as context, or add `<!-- no-gated-phase-split-guard -->` if the phases are intentionally one unit",
            file.display(),
            add_after_hint
        ),
    ]))
}

/// True when a kept-open item body enumerates multiple gated/remaining phases:
/// the word "phase" appears, at least two short parenthesized phase markers
/// (`(1)`, `(2a)`, `(2b)`, `(3)`, ...) are present, and a gating/remaining
/// signal frames them as deferred work.
fn body_enumerates_multiple_gated_phases(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if !lower.contains("phase") {
        return false;
    }
    let gating = [
        "gated",
        "remaining",
        "live-verify",
        "live verify",
        "awaiting",
        "still needs",
        "not yet",
    ];
    if !gating.iter().any(|signal| lower.contains(signal)) {
        return false;
    }
    count_phase_markers(body) >= 2
}

/// Count distinct short parenthesized phase markers like `(1)`, `(2a)`, `(2b)`,
/// `(3)`. Requires 1-2 digits optionally followed by 1-2 ASCII lowercase letters
/// so dates and commit hashes (`(2026-05-31)`, `(submodule 407b0825)`) are not
/// mistaken for phase markers.
fn count_phase_markers(body: &str) -> usize {
    static MARKER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\((\d{1,2}[a-z]{0,2})\)").unwrap());
    let mut seen = std::collections::HashSet::new();
    for cap in MARKER.captures_iter(body) {
        seen.insert(cap[1].to_string());
    }
    seen.len()
}

/// True when the body already references at least two discrete child ids other
/// than its own (and other than the ubiquitous `#agent-doc-bug` preset tag) —
/// i.e. the phases were already broken out into independently trackable ids, so
/// the split advisory should stay quiet.
fn body_already_split_into_child_ids(body: &str, own_id: &str) -> bool {
    static ID_REF: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"#([a-z0-9][a-z0-9-]*)").unwrap());
    let mut others = std::collections::HashSet::new();
    for cap in ID_REF.captures_iter(body) {
        let id = crate::pending::normalize_pending_id(&cap[1]);
        if !id.is_empty() && id != own_id && id != "agent-doc-bug" {
            others.insert(id);
        }
    }
    others.len() >= 2
}

/// Substep-completion phrases that evidence partial progress in a queue audit.
const QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES: &[&str] = &[
    "is complete",
    "was complete",
    "are complete",
    "were complete",
    "is done",
    "was done",
    "was clean",
    "is clean",
    "is current",
    "are current",
    "passed",
    "verified clean",
    "already complete",
];

/// `#queue-audit-partial-completion`: detect a queue-completion audit response
/// that collapses meaningful partial progress into a blanket "none complete."
///
/// A queue audit ("which queue items are complete?") should classify each row as
/// complete / partially complete / not-started, naming completed substeps and the
/// exact remaining condition — not answer "none are complete" just because every
/// row still has one remaining action. This warn-first guard fires only on the
/// clearest collapse signal: the response is about the queue, makes a blanket
/// none-complete claim, shows at least two distinct substep-completion signals,
/// and never frames anything as "partial." It is WARN-only (never blocks
/// closeout) and suppressed by a `<!-- no-queue-audit-guard -->` marker.
///
/// The richer per-row state table is response guidance (a natural-language
/// judgment that lives in the skill/spec contract, per the binary-vs-skill rule),
/// so the binary only flags the unambiguous collapse rather than trying to
/// classify free-text rows itself.
fn check_queue_audit_partial_completion_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-queue-audit-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !queue_audit_collapses_partial_completion(&lower) {
        return Ok(GuardResult::None);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "queue_audit_partial_completion_guard_fired file={}",
            file.display()
        ),
    );

    Ok(GuardResult::Warn(vec![
        "[session-check] warn: this queue-completion audit reports the queue as not complete while also citing several completed substeps, but never classifies any row as partially complete — meaningful partial progress is collapsed into \"none complete\"".to_string(),
        "[session-check] hint: classify each queue row as complete / partially complete / not-started, naming the completed substeps and the exact remaining condition for partial rows; recommend splitting a row with multiple gateable phases. Add `<!-- no-queue-audit-guard -->` if the all-or-none framing is intentional.".to_string(),
    ]))
}

/// True when a queue-audit response collapses partial completion: it is about the
/// queue, makes a blanket none-complete claim, shows >=2 distinct substep
/// completions, and never frames anything as "partial."
fn queue_audit_collapses_partial_completion(lower: &str) -> bool {
    if !lower.contains("queue") {
        return false;
    }
    // Already broke it down — not a collapse.
    if lower.contains("partial") {
        return false;
    }
    if !queue_audit_has_none_complete_claim(lower) {
        return false;
    }
    let substep_completions = QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES
        .iter()
        .filter(|phrase| lower.contains(*phrase))
        .count();
    substep_completions >= 2
}

/// A blanket "none / not ... complete" claim about the queue items.
fn queue_audit_has_none_complete_claim(lower: &str) -> bool {
    static NONE_COMPLETE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        // "none of the queue items is/are (fully) complete", "no items are
        // complete", "none are fully complete", etc. — a none/no quantifier
        // within a short span before a complete/completed token.
        regex::Regex::new(r"\b(none|no)\b[^.\n]{0,60}?\bcomplet(e|ed)\b").unwrap()
    });
    NONE_COMPLETE.is_match(lower)
}

fn single_open_review_item_id(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(None);
    };
    let ids = components
        .into_iter()
        .filter(|component| crate::component::is_review_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect::<Vec<_>>();
    if ids.len() == 1 {
        Ok(ids.into_iter().next())
    } else {
        Ok(None)
    }
}

fn phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

fn detect_active_session_post_commit_drift(file: &Path) -> Result<Option<String>> {
    let Some(session) = crate::codex_hook::load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current == snapshot {
        return Ok(None);
    }
    if crate::git::normalize_committed_exchange_artifacts(&current)
        == crate::git::normalize_committed_exchange_artifacts(&snapshot)
    {
        return Ok(None);
    }

    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    if prompt_marker.is_none()
        && active_session_drift_is_only_exchange_or_backlog_metadata(&snapshot, &current)
    {
        return Ok(None);
    }
    if prompt_marker.is_none() && promptless_comment_only_drift(&snapshot, &current) {
        return Ok(None);
    }
    if prompt_marker.is_none() && exchange_only_promptless_content_drift(&snapshot, &current) {
        return Ok(None);
    }
    let prompt_preview = session
        .last_prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("agent-doc session");
    let prompt_preview = prompt_preview.trim();

    let detail = match prompt_marker {
        Some(marker) => format!(
            "{}; active_session={} turn={} prompt={}",
            marker, session.session_id, session.last_turn_id, prompt_preview
        ),
        None => format!(
            "active_session={} turn={} prompt={}",
            session.session_id, session.last_turn_id, prompt_preview
        ),
    };
    Ok(Some(detail))
}

fn detect_uncommitted_exchange_drift(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current == snapshot {
        return Ok(None);
    }
    let norm_current = crate::git::normalize_committed_exchange_artifacts(&current);
    let norm_snapshot = crate::git::normalize_committed_exchange_artifacts(&snapshot);
    if norm_current == norm_snapshot {
        return Ok(None);
    }
    if !exchange_has_new_appended_content(&norm_snapshot, &norm_current) {
        return Ok(None);
    }
    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    let detail = match prompt_marker {
        Some(marker) => format!(
            "uncommitted working tree drift beyond snapshot with exchange changes; {}",
            marker
        ),
        None => "uncommitted working tree drift beyond snapshot with exchange changes".to_string(),
    };
    Ok(Some(detail))
}

fn exchange_has_new_appended_content(snapshot: &str, current: &str) -> bool {
    let Some(snapshot_exchange) = extract_normalized_exchange_body(snapshot) else {
        return false;
    };
    let Some(current_exchange) = extract_normalized_exchange_body(current) else {
        return false;
    };
    if current_exchange == snapshot_exchange {
        return false;
    }
    let snapshot_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let current_lines: Vec<&str> = current_exchange.lines().collect();
    if current_lines.len() <= snapshot_lines.len() {
        return false;
    }
    for (i, line) in snapshot_lines.iter().enumerate() {
        if current_lines.get(i) != Some(line) {
            return false;
        }
    }
    let appended: String = current_lines[snapshot_lines.len()..].join("\n");
    if appended
        .lines()
        .map(str::trim)
        .any(is_exchange_response_heading)
    {
        return true;
    }
    if appended
        .lines()
        .any(crate::diff::text_line_looks_like_prompt_target)
    {
        return false;
    }
    true
}

fn extract_normalized_exchange_body(doc: &str) -> Option<String> {
    let (_, body) = crate::frontmatter::parse(doc).ok()?;
    let components = crate::component::parse(body).ok()?;
    for component in &components {
        if component.name == "exchange" {
            return Some(component.content(body).to_string());
        }
    }
    None
}

fn exchange_only_promptless_content_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    let Some(snapshot_masked) = mask_exchange_component_content(snapshot) else {
        return false;
    };
    let Some(current_masked) = mask_exchange_component_content(current) else {
        return false;
    };
    crate::git::normalize_transient_agent_doc_markers(&snapshot_masked)
        == crate::git::normalize_transient_agent_doc_markers(&current_masked)
}

fn active_session_drift_is_only_exchange_or_backlog_metadata(
    snapshot: &str,
    current: &str,
) -> bool {
    let Some(snapshot_masked) = mask_components_by_name(snapshot, &["exchange", "backlog"]) else {
        return false;
    };
    let Some(current_masked) = mask_components_by_name(current, &["exchange", "backlog"]) else {
        return false;
    };
    crate::git::normalize_transient_agent_doc_markers(&snapshot_masked)
        == crate::git::normalize_transient_agent_doc_markers(&current_masked)
}

fn promptless_comment_only_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    crate::git::normalize_transient_agent_doc_markers(&crate::diff::strip_comments(snapshot))
        == crate::git::normalize_transient_agent_doc_markers(&crate::diff::strip_comments(current))
}

fn mask_exchange_component_content(doc: &str) -> Option<String> {
    mask_components_by_name(doc, &["exchange"])
}

fn mask_components_by_name(doc: &str, names: &[&str]) -> Option<String> {
    let components = crate::component::parse(doc).ok()?;
    let mut masked = doc.to_string();
    let mut saw_target = false;
    for component in components.iter().rev() {
        if !names.contains(&component.name.as_str()) {
            continue;
        }
        saw_target = true;
        masked.replace_range(component.open_end..component.close_start, "\n");
    }
    saw_target.then_some(masked)
}

fn open_cycle_message(file: &Path, state: &crate::cycle_state::CycleState) -> Result<String> {
    let ipc_hint = latest_ipc_proof_diagnostic_hint(file)?
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    if state.last_event.starts_with("direct_invocation_timeout")
        || state
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
    {
        return Ok(format!(
            "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — direct invocation did not reach response capture. If the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {}` (or `agent-doc session clear {}`). Otherwise retry from outside the managed pane, restart the owner with `agent-doc start {}`, or abandon the stale cycle only after confirming no response exists.{}",
            state.cycle_id,
            phase_name(state.phase),
            state.last_event,
            state.file,
            state.file,
            state.file,
            ipc_hint
        ));
    }
    let detail = match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => {
            "cycle started but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::ResponseCaptured => {
            "response was captured but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::WriteApplied => {
            "response write landed but no terminal commit followed"
        }
        crate::cycle_state::CyclePhase::Committed => "no terminal commit followed",
        crate::cycle_state::CyclePhase::Abandoned => "cycle was abandoned",
    };
    Ok(format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — {}.{}",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        detail,
        ipc_hint
    ))
}

fn open_cycle_manual_patchback_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<Option<String>> {
    if !matches!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    ) {
        return Ok(None);
    }
    let Some(marker) = detect_bypassed_response_write(file)? else {
        return Ok(None);
    };
    Ok(Some(format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — found visible response patchback {} that is still outside the commit boundary. This looks like a manual repair that stopped before commit; finish it with `agent-doc write --commit {}` if you still have the response body, or commit the repaired document manually once the response is correct.",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        marker,
        file.display()
    )))
}

/// Return the message portion of the last non-empty line in `ops.log`,
/// stripped of the `[epoch_secs] ` timestamp prefix.
///
/// Returns `Ok(None)` when the log file is missing or empty.
pub fn last_ops_event(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = crate::fs_util::read_optional_text(&log_path)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    let last = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rfind(|line| {
            line.contains(&format!("file={canonical_display}"))
                || line.contains(&format!("file={requested_display}"))
        })
        .or_else(|| content.lines().rfind(|l| !l.trim().is_empty()))
        .map(|l| strip_timestamp_prefix(l).to_string());
    Ok(last)
}

pub fn latest_ipc_proof_diagnostic(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = crate::fs_util::read_optional_text(&log_path)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .map(strip_timestamp_prefix)
        .find(|event| {
            event.starts_with(IPC_PROOF_INSUFFICIENT_EVENT)
                && (event.contains(&format!("file={canonical_display}"))
                    || event.contains(&format!("file={requested_display}")))
        })
        .map(str::to_string))
}

pub fn latest_ipc_proof_diagnostic_hint(file: &Path) -> Result<Option<String>> {
    Ok(latest_ipc_proof_diagnostic(file)?
        .map(|event| format!("latest IPC proof diagnostic: {event}")))
}

/// Strip a leading `[NNN] ` timestamp prefix from a log line.
fn strip_timestamp_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(close) = rest.find("] ")
    {
        return &rest[close + 2..];
    }
    line
}

pub fn detect_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    Ok(last_ops_event(file)?.filter(|event| is_write_completed_commit_missing_event(event)))
}

fn is_write_completed_commit_missing_event(event: &str) -> bool {
    event.starts_with(IPC_WRITE_CONSUMED_EVENT) || event.starts_with(SNAPSHOT_SAVED_FILE_IPC_EVENT)
}

fn event_name(event: &str) -> &str {
    event.split_whitespace().next().unwrap_or(event)
}

pub fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    Ok(detect_bypassed_response_write_between(&snapshot, &current))
}

pub fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    // Normalize transient markers before comparison — (HEAD) annotations and
    // boundary IDs legitimately differ between snapshot (clean) and working tree
    // (preserves HEAD). Without this, preserved (HEAD) markers cause false-positive
    // "direct response patchback" detection.
    let norm = |s: &str| crate::git::normalize_transient_agent_doc_markers(s);
    let snap_norm = norm(snapshot_doc);
    let cur_norm = norm(current_doc);
    if cur_norm == snap_norm {
        return None;
    }
    if !has_new_response_heading_marker(&snap_norm, &cur_norm) {
        return None;
    }

    let diff_text = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm)?;

    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
            if let Some(bare_target) =
                first_bare_prompt_prefix_target_before_marker(&diff_text, trimmed)
            {
                return Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                ));
            }
            return Some(trimmed.to_string());
        }
    }
    None
}

fn first_bare_prompt_prefix_target_before_marker(diff_text: &str, marker: &str) -> Option<String> {
    let mut prefix_diff = String::new();
    for line in diff_text.lines() {
        if line
            .strip_prefix('+')
            .is_some_and(|added| added.trim() == marker)
        {
            break;
        }
        prefix_diff.push_str(line);
        prefix_diff.push('\n');
    }
    crate::diff::first_bare_prompt_prefix_target(&prefix_diff)
}

fn has_new_response_heading_marker(snapshot_doc: &str, current_doc: &str) -> bool {
    use std::collections::BTreeMap;

    fn marker_counts(doc: &str) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for line in doc.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
                *counts.entry(trimmed.to_string()).or_insert(0) += 1;
            }
        }
        counts
    }

    let snapshot_counts = marker_counts(snapshot_doc);
    let current_counts = marker_counts(current_doc);
    current_counts
        .into_iter()
        .any(|(marker, count)| count > snapshot_counts.get(&marker).copied().unwrap_or(0))
}

pub fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

/// `#prompt-preempts-auto-queue`: snapshot-independent detection of a live
/// unresolved user prompt in `agent:exchange`. A prompt is unresolved when there
/// is user-authored, non-comment text after the latest `agent:boundary` marker
/// in the exchange and no `### Re:` response heading follows it in that tail
/// segment. Unlike the snapshot-diff path, this fires even when the prompt was
/// already baselined into the snapshot (so the ordinary diff sees only queue
/// bookkeeping). Returns the joined prompt text, or `None` when the tail is
/// empty or already answered.
pub fn unresolved_exchange_prompt(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(unresolved_exchange_prompt_in_content(&content))
}

fn unresolved_exchange_prompt_in_content(content: &str) -> Option<String> {
    let components = crate::component::parse(content).ok()?;
    let exchange = components.iter().find(|c| c.name == "exchange")?;
    let body = exchange.content(content);
    let lines: Vec<&str> = body.lines().collect();

    // The latest boundary marks the end of the last committed/answered segment;
    // everything after it is the new, not-yet-answered tail.
    let tail_start = lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let tail = &lines[tail_start..];

    // `#prompt-preempts-auto-queue` / `#queue-continuation-buries-prompt`: a
    // response heading means the prompt *above it* was answered — but a
    // queue-continuation response (`### Re: do [#id]` / `### Re: re [#id]`)
    // answers a queue/backlog item, NOT a free-text user prompt. When the only
    // response after a free-text prompt is a queue continuation, the prompt is
    // still unresolved; the queue continuation must not let the boundary bury it.
    // Scan only the prompt region up to the FIRST response heading so a queue
    // continuation's own response body is never mistaken for prompt text.
    let first_response_idx = tail
        .iter()
        .position(|line| is_exchange_response_heading(line.trim()));
    if let Some(idx) = first_response_idx {
        let heading = tail[idx].trim();
        if !is_queue_continuation_response_heading(heading) {
            // A genuine free-text answer resolves the prompt.
            return None;
        }
        // Queue-continuation response — does not answer a free-text prompt.
    }
    let prompt_region = match first_response_idx {
        Some(idx) => &tail[..idx],
        None => tail,
    };

    let prompt_lines: Vec<String> = prompt_region
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("<!--")
                && !line.starts_with("-->")
                && !is_exchange_response_heading(line)
        })
        .map(normalized_prompt_for_match)
        .filter(|line| !line.is_empty())
        .collect();
    if prompt_lines.is_empty() {
        return None;
    }
    Some(prompt_lines.join("\n"))
}

fn exchange_tail_has_response_heading(file: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(components) = crate::component::parse(&content) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    let body = exchange.content(&content);
    let lines: Vec<&str> = body.lines().collect();
    let tail_start = lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    lines[tail_start..]
        .iter()
        .any(|line| is_exchange_response_heading(line.trim()))
}

/// `#queue-continuation-buries-prompt`: a queue-continuation response heading
/// (`### Re: do [#id]` / `### Re: re [#id]`, any h-level) answers a queue or
/// backlog item, not a free-text user prompt. Such a heading must not mark a
/// preceding free-text exchange prompt as answered, or a queue continuation can
/// advance the boundary past an unanswered user prompt and bury it in the
/// snapshot (the JB "ignored my previous prompt" class).
pub fn is_queue_continuation_response_heading(trimmed: &str) -> bool {
    let Some(rest) = trimmed
        .strip_prefix("### Re:")
        .or_else(|| trimmed.strip_prefix("#### Re:"))
        .or_else(|| trimmed.strip_prefix("##### Re:"))
        .or_else(|| trimmed.strip_prefix("###### Re:"))
    else {
        return false;
    };
    let topic = rest.trim_start();
    // Queue-continuation topics start with the `do`/`re` directive verb plus a
    // bracketed id, e.g. "do [#6cmx]" or "re [#374n] ...".
    (topic.starts_with("do [#") || topic.starts_with("re [#")) && topic.contains(']')
}

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let Some(change) = first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let label = match change.kind {
        crate::diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        crate::diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        crate::diff::PromptBearingChangeKind::RecoveryArtifact
        | crate::diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    Ok(Some(format!("{label}: {preview}")))
}

pub fn first_unstarted_prompt_bearing_change(
    file: &Path,
) -> Result<Option<crate::diff::PromptBearingChange>> {
    // A fresh session can carry an unanswered exchange tail prompt before any
    // cycle snapshot exists. The queue path activates independently of the
    // snapshot (route queue activation re-saves the snapshot on activation), so
    // a queue write always dispatches; the exchange path relies on this diff, so
    // without a snapshot we must fall back to the committed `HEAD` blob (then to
    // an empty baseline for untracked docs) — otherwise the exchange prompt is
    // invisible and `Run Agent Doc` does nothing while the same write into the
    // queue starts a turn (#codex-exchange-prompt-no-dispatch).
    let baseline = match crate::snapshot::load(file)? {
        Some(snapshot) => snapshot,
        None => crate::git::show_head(file)?.unwrap_or_default(),
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let prompt_bearing_body = |content: &str| {
        let body = crate::frontmatter::parse(content)
            .map(|(_, body)| body.to_string())
            .unwrap_or_else(|_| content.to_string());
        crate::diff::strip_comments(&strip_queue_components_for_unstarted_prompt_guard(&body))
    };
    let norm = |s: &str| crate::git::normalize_committed_exchange_artifacts(s);
    let snap_norm = norm(&prompt_bearing_body(&baseline));
    let cur_norm = norm(&prompt_bearing_body(&current));
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };
    let changes = crate::diff::classify_prompt_bearing_changes(&diff_text);
    let mut skip_answered_response_run = false;
    for (idx, change) in changes.iter().enumerate() {
        match change.kind {
            crate::diff::PromptBearingChangeKind::RecoveryArtifact
            | crate::diff::PromptBearingChangeKind::BoundaryArtifact => continue,
            crate::diff::PromptBearingChangeKind::PromptTarget => {
                if skip_answered_response_run {
                    let preview = change
                        .text
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or(change.text.as_str())
                        .trim();
                    if !crate::diff::line_looks_like_fresh_prompt_after_response(preview) {
                        continue;
                    }
                }
                if crate::diff::prompt_change_is_already_answered(&change.text)
                    || crate::diff::prompt_change_is_answered_by_later_response(&changes, idx)
                    || prompt_target_is_immediately_before_existing_response(&current, &change.text)
                {
                    skip_answered_response_run = true;
                    continue;
                }
                return Ok(Some(change.clone()));
            }
            crate::diff::PromptBearingChangeKind::ContentEdit => {
                continue;
            }
        }
    }
    Ok(None)
}

fn strip_queue_components_for_unstarted_prompt_guard(body: &str) -> String {
    let Ok(components) = crate::component::parse(body) else {
        return body.to_string();
    };
    let mut result = body.to_string();
    for component in components.iter().rev() {
        if component.name == "queue" {
            result = component.replace_content(&result, "");
        }
    }
    result
}

fn prompt_target_is_immediately_before_existing_response(
    current_doc: &str,
    change_text: &str,
) -> bool {
    let target_line = change_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string());
    let answered_prompt_marker = target_line
        .as_deref()
        .is_some_and(|line| line.starts_with('❯'));
    let target = target_line
        .as_deref()
        .map(|line| line.trim_start_matches('❯').trim().to_string());
    let Some(target) = target else {
        return false;
    };
    if target.is_empty() {
        return false;
    }
    let body = crate::frontmatter::parse(current_doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| current_doc.to_string());
    let Ok(components) = crate::component::parse(&body) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let lines: Vec<&str> = exchange.content(&body).lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let normalized = line.trim().trim_start_matches('❯').trim();
        if normalized != target {
            continue;
        }
        for next in lines.iter().skip(idx + 1) {
            let trimmed = next.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                continue;
            }
            if is_exchange_response_heading(trimmed) {
                return true;
            }
            if answered_prompt_marker {
                continue;
            }
            return false;
        }
    }
    false
}

fn prompt_only_exchange_tail(doc: &str) -> Option<String> {
    let body = crate::frontmatter::parse(doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| doc.to_string());
    let components = crate::component::parse(&body).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;

    let mut in_fence: Option<&'static str> = None;
    let mut prompt_preview: Option<String> = None;
    let mut in_assistant_response = false;
    for line in exchange.content(&body).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = match in_fence {
                Some("```") => None,
                None => Some("```"),
                other => other,
            };
            continue;
        }
        if trimmed.starts_with("~~~") {
            in_fence = match in_fence {
                Some("~~~") => None,
                None => Some("~~~"),
                other => other,
            };
            continue;
        }
        if in_fence.is_some() {
            continue;
        }
        if is_exchange_response_heading(trimmed) {
            prompt_preview = None;
            in_assistant_response = true;
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary:") || trimmed == "## User" {
            in_assistant_response = false;
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("<!--")
            || trimmed == "(HEAD)"
            || crate::diff::line_looks_like_plain_response_after_prompt(trimmed)
        {
            continue;
        }
        if crate::diff::text_line_looks_like_prompt_target(trimmed) {
            if in_assistant_response && !trimmed.starts_with('❯') {
                continue;
            }
            prompt_preview.get_or_insert_with(|| {
                trimmed
                    .trim_start_matches('❯')
                    .trim()
                    .chars()
                    .take(160)
                    .collect::<String>()
            });
        }
    }
    prompt_preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process::Command;

    fn inspect(file: &std::path::Path) -> Result<SessionCheckStatus> {
        let _process_global_lock = crate::test_support::env_lock();
        super::inspect(file)
    }

    fn inspect_with_warnings(file: &std::path::Path) -> Result<SessionCheckReport> {
        let _process_global_lock = crate::test_support::env_lock();
        super::inspect_with_warnings(file)
    }

    /// Phase 6 (#lr-content-6): build a `RunContext` whose `DocContentCell` holds
    /// the file's current content, mirroring how `inspect_with_warnings` shares
    /// one context across the guard sweep.
    fn test_rc(file: &std::path::Path) -> crate::graph::RunContext {
        let rc = crate::graph::RunContext::new(file.to_path_buf());
        rc.set_doc_content(std::fs::read_to_string(file).unwrap_or_default());
        rc
    }

    // Phase 6 (#lr-content-6): test-module wrappers that supply the shared
    // `RunContext` the guards now require, so existing single-arg call sites keep
    // working (same shadowing pattern as the `inspect` wrappers above).
    fn check_blocked_closeout_followup_guard(file: &std::path::Path) -> Result<GuardResult> {
        super::check_blocked_closeout_followup_guard(file, &test_rc(file))
    }
    fn check_gated_phase_split_guard(file: &std::path::Path) -> Result<GuardResult> {
        super::check_gated_phase_split_guard(file, &test_rc(file))
    }
    fn check_expect_done_or_gate_guard(file: &std::path::Path) -> Result<GuardResult> {
        super::check_expect_done_or_gate_guard(file, &test_rc(file))
    }
    fn check_queue_response_contamination_guard(file: &std::path::Path) -> Result<GuardResult> {
        super::check_queue_response_contamination_guard(file, &test_rc(file))
    }

    /// Phase 6 (#lr-content-6): the `_with_context` guard-mode resolvers read
    /// frontmatter from the cached `FrontmatterSlot` (populated once via
    /// `set_doc_content`) instead of re-reading the file. Proven by resolving a
    /// guard mode against a path that does not exist on disk — only the slot
    /// content can supply the value.
    #[test]
    fn phase6_guard_mode_resolves_from_frontmatter_slot_not_file() {
        let missing = std::path::Path::new("/nonexistent/phase6-content-slot.md");

        let rc = crate::graph::RunContext::new(missing.to_path_buf());
        rc.set_doc_content(
            "---\nagent_doc_session: test\npending_done_guard: strict\n---\n\nBody\n".to_string(),
        );
        assert_eq!(
            resolve_pending_done_guard_mode_with_context(missing, &rc).unwrap(),
            crate::frontmatter::PendingCaptureGuardMode::Strict,
        );

        let rc_off = crate::graph::RunContext::new(missing.to_path_buf());
        rc_off.set_doc_content(
            "---\nagent_doc_session: test\npending_done_guard: off\n---\n\nBody\n".to_string(),
        );
        assert_eq!(
            resolve_pending_done_guard_mode_with_context(missing, &rc_off).unwrap(),
            crate::frontmatter::PendingCaptureGuardMode::Off,
        );
    }

    /// Phase 6 (#lr-content-6): the shared `ComponentsSlot` parses the same
    /// `DocContentCell` the guards read, so component offsets stay consistent
    /// with `doc_content()` and the slot is cached (parsed once).
    #[test]
    fn phase6_components_slot_matches_doc_content() {
        let missing = std::path::Path::new("/nonexistent/phase6-components.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n"
                .to_string();
        let rc = crate::graph::RunContext::new(missing.to_path_buf());
        rc.set_doc_content(content.clone());

        let doc = rc.doc_content();
        assert_eq!(doc, content);
        let components = rc.components();
        let exchange = components
            .iter()
            .find(|c| c.name == "exchange")
            .expect("exchange component parsed from the cached slot");
        // Offsets index the same string `doc_content()` returns.
        assert!(doc[exchange.open_end..exchange.close_start].contains("hi"));
        assert!(rc.is_components_cached());
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::env_lock();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_frontmatter_only_drift() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = "---\nagent: claude\nagent_doc_session: test\n---\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\
Body\n\
<!-- /agent:exchange -->\n";
        let current = snapshot.replacen("agent: claude", "agent: codex", 1);
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "frontmatter-only metadata drift must not become prompt-bearing"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_detects_fresh_exchange_prompt_without_snapshot() {
        // #codex-exchange-prompt-no-dispatch: a fresh session has no cycle
        // snapshot yet. The queue path activates snapshot-independently, but the
        // exchange path keys off this diff. Without a snapshot it must fall back
        // to the committed HEAD blob so a freshly typed exchange tail prompt is
        // still detected (otherwise `Run Agent Doc` does nothing for exchange
        // writes while a queue write starts a turn).
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        // HEAD: an already-answered exchange, no trailing unanswered prompt.
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        // Working tree: a freshly typed, unanswered exchange tail prompt — and
        // crucially NO snapshot saved (simulating a brand-new session).
        let current = committed.replace(
            "Answer.\n<!-- /agent:exchange -->\n",
            "Answer.\nPlease fix the markdown parser.\n<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, &current).unwrap();
        assert!(
            crate::snapshot::load(&doc).unwrap().is_none(),
            "precondition: fresh session has no snapshot"
        );

        let change = first_unstarted_prompt_bearing_change(&doc)
            .unwrap()
            .expect("fresh exchange tail prompt must be detected via HEAD fallback");
        assert!(
            change.text.contains("Please fix the markdown parser."),
            "detected change should be the new exchange prompt, got: {:?}",
            change.text
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_fresh_queue_only_write_without_snapshot() {
        // Regression guard for #codex-exchange-prompt-no-dispatch: the HEAD
        // fallback must stay exchange-scoped. A queue-only write with no snapshot
        // must NOT surface as an exchange prompt-bearing change — the queue keeps
        // its own snapshot-independent activation path.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        let current = committed.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "<!-- agent:queue -->\n- run the parser fix\n<!-- /agent:queue -->\n",
        );
        fs::write(&doc, &current).unwrap();
        assert!(
            crate::snapshot::load(&doc).unwrap().is_none(),
            "precondition: fresh session has no snapshot"
        );

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "a queue-only write must not become an exchange prompt-bearing change"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_answered_prompt_after_stale_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "Can we run specific rubrics for fine tuning?\n",
            "### Re: specific rubrics — gpt-5\n\n",
            "Yes.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        assert!(crate::diff::text_line_looks_like_prompt_target(
            "Can we run specific rubrics for fine tuning?"
        ));
        assert!(
            crate::diff::prompt_change_is_already_answered(
                "Can we run specific rubrics for fine tuning?\n### Re: specific rubrics — gpt-5\n\nYes.\n"
            ),
            "fixture block should be recognized as already answered"
        );
        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "answered prompt after a stale boundary must not stay actionable"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_raw_answered_prompt_after_stale_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\n",
            "I updated the repo-local references to the renamed GitHub repo.\n\n",
            "- `.gitmodules` now uses `git@github.com:ClaudeScore/buildparty-investor-demo.git`\n",
            "- The checked-out submodule at `buildparty-investor-demo/` now has `origin` set to the same URL\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        assert!(crate::diff::prompt_change_is_already_answered(
            "I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\nI updated the repo-local references to the renamed GitHub repo.\n"
        ));

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "raw assistant completion prose after a stale-boundary prompt must not stay actionable"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_plain_content_edit_noise() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "The service returned 401 from this endpoint\n",
            "### Re: service status — gpt-5\n\n",
            "Already answered.\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = snapshot.replace(
            "The service returned 401 from this endpoint",
            "The service returned 503 from this endpoint",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        let changes = crate::diff::classify_prompt_bearing_changes(
            &crate::diff::unified_diff_from_contents(
                &crate::frontmatter::parse(snapshot).unwrap().1,
                &crate::frontmatter::parse(&fs::read_to_string(&doc).unwrap())
                    .unwrap()
                    .1,
            )
            .unwrap(),
        );
        assert_eq!(changes.len(), 1, "expected one content edit: {changes:?}");
        assert_eq!(
            changes[0].kind,
            crate::diff::PromptBearingChangeKind::ContentEdit
        );

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "session-check should not reopen a committed turn for plain content-edit drift"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_prefixed_response_label_noise() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deploy — gpt-5\n\n",
            "Both redirects confirmed via `curl`.\n",
            "<!-- agent:boundary:done -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deploy — gpt-5\n\n",
            "Both redirects confirmed via `curl`.\n",
            "❯ **Verification:** Both redirects confirmed via `curl`.\n",
            "❯ **Commit / push:**\n",
            "<!-- agent:boundary:done -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "prefixed assistant response labels must not reopen a committed cycle"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_detects_plain_exchange_tail_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        let change = first_unstarted_prompt_bearing_change(&doc)
            .unwrap()
            .expect("plain exchange-tail prompt should remain actionable");
        assert_eq!(
            change.kind,
            crate::diff::PromptBearingChangeKind::PromptTarget
        );
        assert_eq!(
            change.text,
            "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
        );
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_ignores_html_comment_prompt_text() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        let change = first_unstarted_prompt_bearing_change(&doc).unwrap();
        assert!(
            change.is_none(),
            "prompt-like text inside ordinary HTML comments must not reopen the cycle"
        );
    }

    #[test]
    fn prompt_only_exchange_tail_detects_closed_cycle_with_no_response_patchback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "❯ do [#vt-agent-deploy]. spec-test-news-commit-push\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(current), Some(current)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(current), Some(current))
            .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("prompt-only closeout tail"));
                assert!(message.contains("#vt-agent-deploy"));
            }
            other => panic!("expected prompt-only closeout interruption, got {other:?}"),
        }
    }

    #[test]
    fn enforce_clean_closeout_self_heals_late_ipc_overapplication() {
        // #late-ipc-patch-response-uncommitted: a late-IPC stale-patch replay
        // re-adds a duplicate `### Re:` block to the working tree after the cycle
        // committed. enforce_clean_closeout (the finalize boundary) must self-heal
        // by restoring committed HEAD instead of bailing — otherwise the
        // interruption stalls the agent:queue auto-loop.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\n\n",
            "Answer A.\n",
            "### Re: second — opus-4-8\n\n",
            "Answer B.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Late stale-patch replay re-inserts an earlier committed response (A) at
        // the tail (non-adjacent over-application), leaving the real responses in
        // HEAD untouched.
        let overapplied = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: first — opus-4-8\n\nAnswer A.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &overapplied).unwrap();
        assert!(
            detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "precondition: late-IPC over-application present"
        );

        // The finalize boundary must NOT bail — it self-heals.
        enforce_clean_closeout(&doc).expect("enforce_clean_closeout should self-heal, not bail");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            committed,
            "working tree restored to committed HEAD (duplicate dropped)"
        );
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().unwrap(),
            committed,
            "snapshot restored to committed HEAD"
        );
    }

    #[test]
    fn prompt_only_exchange_tail_catches_direct_chat_preset_no_patchback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deploy — gpt-5\n\n",
            "Deployed v1.\n",
            "❯ commit-push\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("prompt-only closeout tail"),
                    "should mention prompt-only closeout tail: {message}"
                );
                assert!(
                    message.contains("commit-push"),
                    "should reference the preset prompt: {message}"
                );
                assert!(
                    message.contains("agent-doc finalize")
                        || message.contains("agent-doc write --commit"),
                    "should name the recovery command: {message}"
                );
            }
            other => panic!(
                "expected prompt-only closeout interruption for direct-chat preset, got {other:?}"
            ),
        }
    }

    #[test]
    fn prompt_only_exchange_tail_catches_opencode_no_patchback() {
        let _env = EnvGuard::set("OPENCODE", "1");
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: sidebar revert assessment — glm-5\n\n",
            "Sidebar revert is safe.\n\n",
            "❯ do [#noexchopencode2]. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(current), Some(current)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(current), Some(current))
            .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("prompt-only closeout tail"),
                    "should mention prompt-only closeout tail: {message}"
                );
                assert!(
                    message.contains("#noexchopencode2"),
                    "should reference the prompt: {message}"
                );
                assert!(
                    message.contains("agent-doc finalize")
                        || message.contains("agent-doc write --commit"),
                    "should name the recovery command: {message}"
                );
            }
            other => panic!(
                "expected prompt-only closeout interruption for OpenCode no-patchback, got {other:?}"
            ),
        }
    }

    #[test]
    fn prompt_only_exchange_tail_ignores_answered_tail_prompt() {
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do [#vt-agent-deploy]. spec-test-news-commit-push\n",
            "### Re: vt agent deploy — gpt-5\n\n",
            "Deployed and verified.\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(prompt_only_exchange_tail(current), None);
    }

    #[test]
    fn prompt_only_exchange_tail_ignores_assistant_closeout_status_after_response_heading() {
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: starting dispatch — gpt-5\n\n",
            "Implemented the route/startup guard and updated the regression coverage.\n\n",
            "The push is still running after closeout and should not require a repair patchback.\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(prompt_only_exchange_tail(current), None);
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn make_project(tmp: &Path) -> std::path::PathBuf {
        fs::create_dir_all(tmp.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(tmp.join(".agent-doc/snapshots")).unwrap();
        let doc = tmp.join("doc.md");
        fs::write(&doc, "body").unwrap();
        doc
    }

    fn track_active_codex_session(root: &Path, doc: &Path, prompt: &str) {
        let session_id = "codex-session";
        let state_dir = root.join(".agent-doc/codex-hooks/sessions");
        fs::create_dir_all(&state_dir).unwrap();
        let hash = crate::ops_log::content_hash(session_id);
        let state_path = state_dir.join(format!("{hash}.json"));
        let payload = serde_json::json!({
            "session_id": session_id,
            "doc_path": doc.display().to_string(),
            "last_turn_id": "turn-1",
            "last_prompt": prompt,
            "updated_at": 1u64
        });
        fs::write(state_path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();
    }

    fn setup_committed_capture(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
    ) -> std::path::PathBuf {
        setup_committed_capture_with_pending(
            root,
            frontmatter,
            response,
            had_pending_mutations,
            None,
            &[],
        )
    }

    fn setup_committed_capture_with_pending(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
        pending_body: Option<&str>,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        setup_committed_capture_with_tracked_work(
            root,
            frontmatter,
            response,
            had_pending_mutations,
            pending_body,
            None,
            pending_done_ids,
        )
    }

    fn setup_committed_capture_with_tracked_work(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
        pending_body: Option<&str>,
        icebox_body: Option<&str>,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let prefix = frontmatter.unwrap_or("---\nagent_doc_session: test\n---\n\n");
        let mut current = format!("{prefix}## Exchange\n\nHello\n");
        if let Some(pending_body) = pending_body {
            current.push_str("\n<!-- agent:pending -->\n");
            current.push_str(pending_body);
            if !pending_body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:pending -->\n");
        }
        if let Some(icebox_body) = icebox_body {
            current.push_str("\n<!-- agent:icebox -->\n");
            current.push_str(icebox_body);
            if !icebox_body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:icebox -->\n");
        }
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if had_pending_mutations {
            crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        }
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(
                &doc,
                &pending_done_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&current), Some(&current))
            .unwrap();
        crate::capture::mark_committed(&doc).unwrap();
        doc
    }

    // #codex-final-response-not-written: a committed turn that ran binary-owned
    // work but never captured a response body must fail closed.

    fn write_committed_turn_doc(
        root: &Path,
        capture: bool,
        had_pending_mutations: bool,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        let _lock = crate::test_support::env_lock();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let current =
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\ndo [#nsga4verify]\n".to_string();
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        if capture {
            crate::capture::capture_response(&doc, "### Re: do #nsga4verify — gpt-5\n\nDone.")
                .unwrap();
        }
        if had_pending_mutations {
            crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        }
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(
                &doc,
                &pending_done_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&current), Some(&current))
            .unwrap();
        doc
    }

    #[test]
    fn committed_without_response_body_guard_fires_on_pending_mutations_no_capture() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_committed_turn_doc(dir.path(), false, true, &[]);
        match check_committed_without_response_body_guard(&doc).unwrap() {
            GuardResult::Error(msg) => {
                assert!(
                    msg.contains(
                        "no assistant `### Re:` response body is present in `agent:exchange`"
                    ),
                    "{msg}"
                );
                assert!(msg.contains("agent-doc write --commit"), "{msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn committed_without_response_body_guard_passes_on_done_ids_without_write_turn() {
        // Backlog bookkeeping (done/reaped ids recorded without the response-write
        // path setting `had_pending_mutations`, e.g. `repair`'s completed-backlog
        // reap) is a legitimate no-response commit that must stay OK.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_committed_turn_doc(dir.path(), false, false, &["nsga4verify"]);
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn committed_without_response_body_guard_is_wired_into_inspect() {
        // Prove the guard runs in the `inspect` chain and flips the status to
        // Interrupted with the recovery command, not just when called directly.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_committed_turn_doc(dir.path(), false, true, &[]);
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(
                    msg.contains(
                        "no assistant `### Re:` response body is present in `agent:exchange`"
                    ),
                    "{msg}"
                );
            }
            other => panic!("expected Interrupted, got {other:?}"),
        }
    }

    #[test]
    fn committed_without_response_body_guard_passes_with_captured_response() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_committed_turn_doc(dir.path(), true, true, &["nsga4verify"]);
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    fn write_queue_drain_doc(root: &std::path::Path, exchange_body: &str) -> std::path::PathBuf {
        let _lock = crate::test_support::env_lock();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{exchange_body}\n<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        // A response WAS captured/parsed this turn (capture_id set)...
        crate::capture::capture_response(&doc, "### Re: do #x — gpt-5\n\nDone.").unwrap();
        crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        // ...and this is a queue-drain turn (a head was recorded).
        crate::cycle_state::record_active_queue_heads(&doc, &["x".to_string()]).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&content), Some(&content))
            .unwrap();
        doc
    }

    #[test]
    fn committed_without_response_body_guard_fires_on_queue_drain_captured_but_no_exchange_body() {
        // #codex-queue-drain-no-response-body: a queue-drain turn that captured a
        // response but committed only status/queue/backlog — exchange holds only a
        // compacted `### Session Summary`, zero `### Re:` — must fire even though
        // capture_id is set (the systematic Codex queue-drain symptom).
        let dir = tempfile::tempdir().unwrap();
        let doc = write_queue_drain_doc(dir.path(), "### Session Summary\n\nCompacted.");
        match check_committed_without_response_body_guard(&doc).unwrap() {
            GuardResult::Error(msg) => {
                assert!(msg.contains("agent:exchange"), "{msg}");
                assert!(msg.contains("codex-queue-drain-no-response-body"), "{msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn committed_without_response_body_guard_passes_queue_drain_with_exchange_body() {
        // Same queue-drain shape but the `### Re:` response body DID land → pass.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_queue_drain_doc(
            dir.path(),
            "### Session Summary\n\nCompacted.\n\n### Re: do #x — gpt-5\n\nDone.",
        );
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn committed_without_response_body_guard_passes_recovered_exchange_body_without_capture_metadata()
     {
        // Recovery may commit a visible `### Re:` after the original queue-drain
        // cycle lost its capture metadata. The committed exchange body is still
        // sufficient proof that the missing-response closeout has been repaired.
        let _lock = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nCompacted.\n\n",
            "### Re: do [#ipc1] / do [#39c5]\n\nRecovered.\n",
            "<!-- /agent:exchange -->\n"
        )
        .to_string();
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        crate::cycle_state::record_pending_done_ids(
            &doc,
            &["ipc1".to_string(), "39c5".to_string()],
        )
        .unwrap();
        crate::cycle_state::record_active_queue_heads(
            &doc,
            &["do [#ipc1]".to_string(), "do [#39c5]".to_string()],
        )
        .unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&content), Some(&content))
            .unwrap();

        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn committed_without_response_body_guard_passes_on_noop_sweep_close() {
        // No capture and no binary write turn (sweep re-commit) must stay OK so
        // ordinary already-committed sweeps do not false-fire.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_committed_turn_doc(dir.path(), false, false, &[]);
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn committed_without_response_body_guard_skips_noop_commit_reap_only_cycle() {
        // Deadlock repro (tsift.md cycle-1780257680821): a `finalize --done X` whose
        // only effect was reaping an item already reflected in HEAD commits a no-op
        // (`commit_already_current`) and sets `had_pending_mutations`, but writes no
        // response body. The guard must NOT fire — a no-op commit committed no
        // binary-owned work this turn, so there is nothing a response would
        // accompany; firing wedges the cycle in an infinite
        // session-check-interrupted loop because the `write --commit` recovery is
        // itself a no-op. A real side-effect commit (`commit_success`) still fires.
        let _lock = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let current =
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\ndo [#nsga4verify]\n".to_string();
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["nsga4verify".to_string()]).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_already_current",
            Some(&current),
            Some(&current),
        )
        .unwrap();
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn stale_open_preflight_with_no_diff_still_interrupts() {
        // #nochange-after-stall-breadth: even when document == snapshot, a
        // non-terminal preflight cycle is not a healthy no-change state. It
        // must surface the stale-open phase and recovery instead of returning OK.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let current = fs::read_to_string(&doc).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("preflight_started"), "{message}");
                assert!(
                    message.contains("cycle started but no write/commit followed"),
                    "{message}"
                );
            }
            other => panic!("expected stale-open interruption, got {other:?}"),
        }
    }

    #[test]
    fn no_response_active_queue_head_fails_on_reap_only_unconsumed_head() {
        // A bookkeeping-only/no-response closeout may be a legitimate no-op when
        // no runnable work is live. It is not legitimate when the cycle recorded
        // an active queue head that remains queued and open in backlog: that is
        // unconsumed executable work hidden behind a clean snapshot.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#livehead] Complete the live queue head\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#livehead]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_reaped_pending_ids(&doc, &["alreadydone".to_string()]).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("committed without an assistant response body"),
                    "{message}"
                );
                assert!(message.contains("#livehead"), "{message}");
                assert!(
                    message.contains("#nochange-after-stall-breadth"),
                    "{message}"
                );
            }
            other => panic!("expected no-response active-head interruption, got {other:?}"),
        }
    }

    #[test]
    fn reaped_queue_head_without_response_fails_on_silent_loss() {
        // #compact-reap-no-response-record: a maintenance/compaction reap can
        // remove a `do #id` head from agent:backlog AND strike it from the queue
        // without the id's `### Re:` ever landing in agent:exchange. The
        // no-response-active-head guard returns None (the head is no longer
        // queued+open), so this guard must catch the silent loss instead.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered something else.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#other] An unrelated open item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#other]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_active_queue_heads(&doc, &["do [#lostresp]".to_string()])
            .unwrap();
        crate::cycle_state::record_reaped_pending_ids(&doc, &["lostresp".to_string()]).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("without an assistant response landing"),
                    "{message}"
                );
                assert!(message.contains("#lostresp"), "{message}");
                assert!(
                    message.contains("#compact-reap-no-response-record"),
                    "{message}"
                );
            }
            other => panic!("expected silent-loss reap interruption, got {other:?}"),
        }
    }

    #[test]
    fn reaped_queue_head_without_response_passes_when_response_materialized() {
        // A legitimate prior-cycle reap: the id was answered in an earlier cycle
        // (its `### Re: ... #id` heading is durably in agent:exchange) and only
        // reaped now. The response is not lost, so the guard must stay quiet.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do [#answered] — gpt-5\n\nShipped the fix.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#other] An unrelated open item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#other]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_active_queue_heads(&doc, &["do [#answered]".to_string()])
            .unwrap();
        crate::cycle_state::record_reaped_pending_ids(&doc, &["answered".to_string()]).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "a reaped id whose response is materialized in the exchange is not a loss"
        );
    }

    #[test]
    fn reaped_queue_head_without_response_passes_for_non_directive_reap() {
        // A normal `--done` backlog item reaped this cycle was never a `do #id`
        // queue-directive head, so its reap carries no response-landing
        // expectation. The guard keys off active_queue_heads and must not fire.
        // No live queue directive head, so the sibling no-response-active-head
        // guard stays quiet too.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#other] An unrelated open item\n",
            "<!-- /agent:backlog -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_reaped_pending_ids(&doc, &["normaldone".to_string()]).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "a non-directive reaped id is not a queue-head response loss"
        );
    }

    // --- #z2jy bkx9-pure-detector: dormant pure per-id loss detector ---

    fn loss_input<'a>(
        directive_ids: &'a [String],
        reaped_ids: &'a [String],
        content: &'a str,
        archives: &'a [String],
    ) -> ReapedResponseLossInput<'a> {
        ReapedResponseLossInput {
            directive_ids,
            reaped_ids,
            content,
            archives,
        }
    }

    #[test]
    fn pure_detector_flags_reap_only_loss() {
        // The reap-only silent-loss shape: the id was reaped this cycle but no
        // `### Re: ... #id` heading exists anywhere — flag it.
        let directive = vec!["lostresp".to_string()];
        let reaped = vec!["lostresp".to_string()];
        let content = "### Re: prior — gpt-5\n\nAnswered something else.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lostresp".to_string()],
        );
    }

    #[test]
    fn pure_detector_flags_captured_but_id_lost() {
        // The #bkx9 residual: a response WAS captured this cycle (the `#kept`
        // heading is present), but `#lost` lost its own `### Re:` in a CRDT
        // merge. The pure detector ignores capture state, so it surfaces `#lost`
        // even though a sibling id materialized in the same cycle.
        let directive = vec!["kept".to_string(), "lost".to_string()];
        let reaped = vec!["kept".to_string(), "lost".to_string()];
        let content = "### Re: do #kept — opus-4-8\n\nShipped the kept fix.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lost".to_string()],
        );
    }

    #[test]
    fn pure_detector_passes_when_materialized_in_archive() {
        // A legitimate prior-cycle reap whose `### Re:` was compacted into a HEAD
        // archive (absent from the live exchange) is not a loss.
        let directive = vec!["archived".to_string()];
        let reaped = vec!["archived".to_string()];
        let content = "### Re: prior — gpt-5\n\nUnrelated live response.\n";
        let archives = vec!["### Re: do #archived — opus-4-8\n\nShipped earlier.\n".to_string()];
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "a reaped id materialized in a HEAD compact archive is not a loss"
        );
    }

    #[test]
    fn pure_detector_ignores_unreaped_directive() {
        // A directive head that was NOT reaped this cycle carries no
        // response-landing expectation, even without a materialized heading.
        let directive = vec!["pending".to_string()];
        let reaped: Vec<String> = Vec::new();
        let content = "### Re: prior — gpt-5\n\nAnswered.\n";
        let archives: Vec<String> = Vec::new();
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "an unreaped directive id is not a loss"
        );
    }

    #[test]
    fn pure_detector_multi_directive_single_heading_false_positive() {
        // KNOWN false-positive class (pinned so the #bkx9 wiring must address it
        // before going live): a single `### Re:` heading legitimately answers
        // `do #a` + `do #b` in one cycle but names only `#a` in the heading line,
        // addressing `#b` in the body. The heading-scoped detector cannot see the
        // body mention, so it flags `#b` as lost — a false positive.
        let directive = vec!["a".to_string(), "b".to_string()];
        let reaped = vec!["a".to_string(), "b".to_string()];
        let single_heading = "### Re: do #a — opus-4-8\n\nFixed #a; also addressed #b inline.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                single_heading,
                &archives
            )),
            vec!["b".to_string()],
            "documents the multi-directive-single-heading false positive"
        );

        // When the grouped heading names BOTH ids, neither is flagged — the
        // recommended shape that avoids the false positive.
        let grouped_heading = "### Re: do #a, #b — opus-4-8\n\nFixed both.\n";
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                grouped_heading,
                &archives
            ))
            .is_empty(),
            "a grouped heading naming both ids is not a loss"
        );
    }

    #[test]
    fn no_response_active_queue_head_passes_when_later_do_item_is_not_current_head() {
        // The no-response closeout guard only protects the current live queue
        // head. Later id-backed queue items can remain queued and open while a
        // free-text prompt sits ahead of them.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#later] Complete the later queue item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- Investigate the current free-text prompt\n",
            "- do [#later]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_reaped_pending_ids(&doc, &["alreadydone".to_string()]).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "later do items are not active heads while a free-text prompt is current"
        );
    }

    #[test]
    fn no_response_active_queue_head_passes_for_noop_queue_preservation() {
        // A pure no-op closeout can record active queue heads simply because they
        // were visible at preflight. If it did not run pending/backlog
        // bookkeeping, preserving the queue head is healthy: the next actor
        // should run it, not get interrupted by a repair/reap classifier.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#livehead] Complete the live queue head\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#livehead]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "preserved live queue heads without bookkeeping proof are ordinary queued work"
        );
    }

    #[test]
    fn no_response_active_queue_head_passes_for_healthy_no_change() {
        // Healthy committed/no-change state with no active queue head remains OK.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "no active queue head means ordinary no-response bookkeeping stays clean"
        );
    }

    fn write_backlog_doc(path: &Path, backlog_body: &str) {
        let content = format!(
            "---\nagent_doc_session: target\n---\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(path, content).unwrap();
    }

    fn backlog_component_hash(path: &Path) -> String {
        let content = fs::read_to_string(path).unwrap();
        let components = crate::component::parse(&content).unwrap();
        let component = components
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
            .unwrap();
        crate::ops_log::content_hash(component.content(&content))
    }

    #[test]
    fn strip_timestamp_prefix_handles_well_formed_line() {
        assert_eq!(
            strip_timestamp_prefix("[1700000000] preflight_diff_start file=/x"),
            "preflight_diff_start file=/x"
        );
    }

    #[test]
    fn strip_timestamp_prefix_passes_through_malformed() {
        assert_eq!(strip_timestamp_prefix("no bracket"), "no bracket");
    }

    #[test]
    fn last_ops_event_missing_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_empty_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(tmp.path().join(".agent-doc/logs/ops.log"), "\n\n").unwrap();
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_returns_final_event_stripped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] preflight_diff_start file=x\n[101] ipc_write_consumed file=x patches=1\n",
        )
        .unwrap();
        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            "ipc_write_consumed file=x patches=1"
        );
    }

    #[test]
    fn last_ops_event_detects_preflight_start_as_last_line() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] ipc_write_consumed file=x\n[101] preflight_diff_start file=x\n",
        )
        .unwrap();
        let last = last_ops_event(&doc).unwrap().unwrap();
        assert!(last.starts_with(PREFLIGHT_START_EVENT));
    }

    #[test]
    fn last_ops_event_prefers_matching_file_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let other = tmp.path().join("other.md");
        fs::write(&other, "body").unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n[101] preflight_diff_start file={}\n",
                doc.display(),
                other.display()
            ),
        )
        .unwrap();
        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            format!("ipc_write_consumed file={} patches=1", doc.display())
        );
    }

    #[test]
    fn latest_ipc_proof_diagnostic_prefers_matching_file_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let other = tmp.path().join("other.md");
        fs::write(&other, "body").unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_proof_insufficient file={} invariant=no_ack recovery=direct_write_fallback\n[101] ipc_proof_insufficient file={} invariant=missing_response_probe recovery=direct_write_fallback\n",
                other.display(),
                doc.display()
            ),
        )
        .unwrap();

        let diagnostic = latest_ipc_proof_diagnostic(&doc).unwrap().unwrap();
        assert!(diagnostic.contains("invariant=missing_response_probe"));
        assert!(diagnostic.contains("recovery=direct_write_fallback"));
    }

    #[test]
    fn detect_write_completed_commit_missing_returns_last_write_event() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] snapshot_saved_file_ipc file=x snap_len=10\n",
        )
        .unwrap();
        assert_eq!(
            detect_write_completed_commit_missing(&doc)
                .unwrap()
                .unwrap(),
            "snapshot_saved_file_ipc file=x snap_len=10"
        );
    }

    #[test]
    fn session_check_open_cycle_state_exits_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("cycle started but no write/commit followed"));
            }
            other => panic!("expected interrupted state, got {other:?}"),
        }
    }

    #[test]
    fn session_check_open_cycle_surfaces_ipc_proof_diagnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        crate::ops_log::log_op(
            &doc,
            &format!(
                "ipc_proof_insufficient file={} source=file_ipc patch_id=p1 invariant=no_ack recovery=direct_write_fallback",
                doc.display()
            ),
        );

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("latest IPC proof diagnostic"));
                assert!(message.contains("invariant=no_ack"));
                assert!(message.contains("recovery=direct_write_fallback"));
            }
            other => panic!("expected interrupted state, got {other:?}"),
        }
    }

    #[test]
    fn session_check_committed_cycle_state_exits_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some("body"), Some("body")).unwrap();
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(!state.is_open());
        assert_eq!(phase_name(state.phase), "committed");
    }

    #[test]
    fn detect_bypassed_response_write_flags_template_heading() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "---\nagent_doc_format: template\n---\n\n## Exchange\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(&doc, format!("{snapshot}### Re: test — gpt-5\n\nBody\n")).unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap();
        assert_eq!(marker.as_deref(), Some("### Re: test — gpt-5"));
    }

    #[test]
    fn detect_bypassed_response_write_flags_inline_assistant_heading() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "## User\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(&doc, format!("{snapshot}\n## Assistant\n\nResponse\n")).unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap();
        assert_eq!(marker.as_deref(), Some("## Assistant"));
    }

    #[test]
    fn detect_bypassed_response_write_ignores_plain_user_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "## User\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(&doc, format!("{snapshot}\nWhy is this still dirty?\n")).unwrap();

        assert!(detect_bypassed_response_write(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_bypassed_response_write_reports_bare_prompt_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\n<!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\nWhy was this missed?\n### Re: test — gpt-5\n\nBody\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap().unwrap();
        assert!(marker.contains("### Re: test — gpt-5"));
        assert!(marker.contains("Why was this missed?"));
    }

    #[test]
    fn detect_bypassed_response_write_does_not_report_response_body_as_bare_prompt_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\n<!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\n### Re: test — gpt-5\n\nCompleted `#adoc-prefix-strip-session-check-whitelist`.\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap().unwrap();
        assert_eq!(marker, "### Re: test — gpt-5");
    }

    #[test]
    fn detect_bypassed_response_write_between_ignores_non_response_local_drift() {
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            detect_bypassed_response_write_between(snapshot, current),
            None,
            "ordinary local drift over HEAD should not look like a bypassed response write"
        );
    }

    #[test]
    fn session_check_interrupts_on_prompt_bearing_diff_without_cycle_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please investigate this startup miss.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("prompt-bearing user changes"));
                assert!(message.contains("prompt_target"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_committed_state_has_new_prompt_diff() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n\n",
            "❯ Follow up on the remaining gap.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("no new agent-doc cycle started"));
                assert!(message.contains("prompt_target"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_allows_committed_state_with_live_queue_prompt_diff() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        let current = committed.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do #liveipcrace. #spec-test-build-install-commit-push\n<!-- /agent:queue -->",
        );
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"), "unexpected ok: {message}");
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_on_active_session_post_commit_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done. Manual active-turn drift.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active harness session changed this document"));
                assert!(message.contains("binary-owned write/commit path"));
                assert!(message.contains("agent-doc"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_active_session_post_commit_comment_only_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "scratch note with prompt-looking text:\n",
            "do #later. spec-test-build-install-commit-push\n",
            "-->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"), "unexpected ok: {message}");
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_active_session_exchange_only_content_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "The service returned 401 from this endpoint\n",
            "### Re: service status — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = committed.replace(
            "The service returned 401 from this endpoint",
            "The service returned 503 from this endpoint",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #commitchurn. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_active_session_canonicalization_only_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do #closeout-bypass. spec-test-build-install-commit-push\n",
            "### Re: #closeout-bypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Do #closeout-bypass. spec-test-build-install-commit-push\n",
            "### Re: #closeout-bypass — gpt-5 (HEAD)\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_active_session_answered_marker_and_backlog_metadata_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] old wording\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "❯ This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5 (HEAD)\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] updated wording\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_answered_prompt_marker_before_existing_response() {
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "❯ This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5 (HEAD)\n\n",
            "Fixed.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(prompt_target_is_immediately_before_existing_response(
            current,
            "❯ JB `/clear` on this document error:"
        ));
    }

    #[test]
    fn active_session_drift_allows_answered_exchange_and_backlog_metadata() {
        let snapshot = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] old wording\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "❯ This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5 (HEAD)\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] updated wording\n",
            "<!-- /agent:backlog -->\n",
        );

        assert!(active_session_drift_is_only_exchange_or_backlog_metadata(
            snapshot, current
        ));
    }

    #[test]
    fn session_check_reports_missing_commit_after_ipc_write() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] ipc_write_consumed file=x patches=1\n",
        )
        .unwrap();
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("response write landed but no commit followed"));
                assert!(message.contains("ipc_write_consumed"));
            }
            other => panic!("expected interrupted state, got {other:?}"),
        }
    }

    #[test]
    fn session_check_recovers_open_write_applied_cycle_from_committed_exchange_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(
                    message.contains("recovered the missing commit boundary from committed historical exchange snapshot drift"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected repaired ok status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "session_check_commit_boundary_recovered");
        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(repaired_snapshot.contains("### Re: #patchbypass — gpt-5"));
    }

    #[test]
    fn session_check_surfaces_manual_patchback_follow_through_for_open_preflight_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #mcrc. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let manual_patchback = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #mcrc. spec-test-build-install-commit-push\n",
            "### Re: #mcrc — gpt-5\n\n",
            "Recovered body.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, manual_patchback).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("visible response patchback"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("agent-doc write --commit"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("manual repair that stopped before commit"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
    }

    #[test]
    fn session_check_recovers_missing_commit_log_from_committed_exchange_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            root.join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n",
                doc.display()
            ),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(
                    message.contains("recovered the missing commit boundary from committed historical exchange snapshot drift"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected repaired ok status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "session_check_commit_boundary_recovered");
    }

    #[test]
    fn session_check_repairs_committed_historical_snapshot_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let tracked = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, tracked).unwrap();
        crate::snapshot::save(&doc, tracked).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let stale_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let historical = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, historical).unwrap();
        crate::snapshot::save(&doc, stale_snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();
        crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(historical)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(historical),
            Some(historical),
        )
        .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("repaired committed historical exchange snapshot drift"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            repaired_snapshot.contains("### Re: historical"),
            "snapshot should advance to the committed historical response:\n{repaired_snapshot}"
        );
        assert!(
            detect_bypassed_response_write(&doc).unwrap().is_none(),
            "snapshot repair should clear the interrupted marker"
        );
    }

    #[test]
    fn session_check_repairs_committed_historical_prompt_and_response_before_new_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#7mqc] Acceptance contract\n",
            "- [ ] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #7mqc. spec-test-news-commit-push\n",
            "### Re: do `#7mqc` — codex\n\n",
            "Done.\n\n",
            "do #sgzy. #spec-test-news-commit-push\n",
            "### Re: do `#sgzy` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#7mqc] Acceptance contract\n",
            "- [x] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "agent updates", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(head)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(head), Some(head)).unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #7mqc. spec-test-news-commit-push\n",
            "### Re: do `#7mqc` — codex\n\n",
            "Done.\n\n",
            "do #sgzy. #spec-test-news-commit-push\n",
            "### Re: do `#sgzy` — codex\n\n",
            "Done.\n\n",
            "What are the next steps?\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#7mqc] Acceptance contract\n",
            "- [x] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("direct response patchback"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("bare prompt target"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(!repaired_snapshot.contains("### Re: do `#sgzy` — codex"));
        assert!(!repaired_snapshot.contains("What are the next steps?"));
    }

    #[test]
    fn session_check_repairs_committed_historical_answered_prompt_prefix_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #wdiv. spec-test-news-commit-push\n",
            "### Re: #wdiv — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do #wdiv. spec-test-news-commit-push\n",
            "### Re: #wdiv — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "normalize prompt", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "repair_preflight_committed_historical",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, committed).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("repaired committed historical exchange snapshot drift"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            repaired_snapshot, committed,
            "snapshot should follow the committed prompt-prefix normalization"
        );
    }

    #[test]
    fn session_check_fails_closed_when_committed_historical_patchback_mutates_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(head)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(head), Some(head)).unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("direct response patchback"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(!repaired_snapshot.contains("### Re: do `#done` — codex"));
        assert!(!repaired_snapshot.contains("Tuned manually."));
    }

    #[test]
    fn session_check_warns_on_uncaptured_recommendations() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Fix the stale closeout path\n3. Update the command spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 2);
        assert!(report.warnings[0].contains("recommendation-like items"));
    }

    #[test]
    fn session_check_clears_startup_miss_superseded_by_newer_registered_owner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/state/startup-miss")).unwrap();
        let miss = crate::startup_miss::StartupMiss {
            file: doc.display().to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };
        let miss_path = tmp
            .path()
            .join(".agent-doc/state/startup-miss")
            .join(format!("{}.json", crate::snapshot::doc_hash(&doc).unwrap()));
        fs::write(&miss_path, serde_json::to_string_pretty(&miss).unwrap()).unwrap();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            doc.display().to_string(),
            crate::sessions::SessionEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-456".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save_in(tmp.path(), &registry).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/session-456.log"),
            concat!(
                "[10] session_start file=doc.md pane=%408 session=session-456\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
        assert!(
            !miss_path.exists(),
            "session-check should clear stale superseded startup-miss markers"
        );
    }

    #[test]
    fn session_check_skips_warning_when_pending_was_added() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Fix the stale closeout path\n",
            true,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_warns_on_unconditional_followup_remaining_work() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: transfer status — opus-4-6\n\nCompleted 5 of 23 diagrams. 18 remaining to transfer.\n\nOptions to continue:\n1. Retry with rate limiting\n2. Use manual upload\n3. Wait for quota reset\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(!report.warnings.is_empty());
        assert!(report.warnings[0].contains("recommendation-like items"));
    }

    #[test]
    fn session_check_strict_mode_blocks_uncaptured_recommendations() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n"),
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("pending_capture_guard = \"warn\""));
            }
            other => panic!("expected strict-mode failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_suppression_marker_disables_guard() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n<!-- no-pending-capture -->\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_frontmatter_overrides_project_guard_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "[guards]\npending_capture = \"off\"\n",
        )
        .unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n"),
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Interrupted(_)));
    }

    #[test]
    fn session_check_warns_on_single_unresolved_bug_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: tmux pane closure — gpt-5\n\nBecause that session was still hitting the older tmux route/sync cleanup bug that #4qgx was meant to close.\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(!report.warnings.is_empty());
        assert!(report.warnings[0].contains("recommendation-like items"));
    }

    #[test]
    fn session_check_blocks_backlog_required_review_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: code review — gpt-5\n\n1. High: Queue closeout can drift.\n2. Medium: Snapshot repair is too permissive.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("required backlog capture"));
            }
            other => panic!("expected backlog-required failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_allows_backlog_required_review_with_explicit_no_followups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: code review — gpt-5\n\nNo actionable follow-up items remained after this pass.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_blocks_when_bug_transfer_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n- [ ] [#old1] Existing item\n",
        );
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("baseline".to_string()),
            baseline_item_ids: vec!["old1".to_string()],
        };

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();
        crate::cycle_state::record_required_explicit_backlog_item_count(&doc, 4).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("described at least 4 distinct issue(s)"));
                assert!(message.contains("only enumerated 2 explicit backlog item(s)"));
            }
            other => panic!("expected bug-transfer inventory failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_blocks_when_response_promises_multiple_new_target_items_but_only_some_exist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#mcrc] Uncommitted repair follow-up\n- [ ] [#lvls] Preserve list-shape constraint\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#old1] Existing item\n",
        );

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("promised new tracked item(s)"));
                assert!(message.contains("#mcrc"));
                assert!(message.contains("#lvls"));
            }
            other => panic!("expected promised-transfer failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_blocks_when_bug_plan_reference_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\nPlan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n",
            false,
        );
        let plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# Plan\n").unwrap();

        crate::cycle_state::record_required_plan_reference_count(&doc, 2).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("required at least 2 explicit plan reference(s)"));
                assert!(message.contains("only cited 1 existing plan path(s)"));
            }
            other => panic!("expected plan-reference inventory failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_warns_on_missing_pending_done_for_completed_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: warn\n---\n\n"),
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 2);
        assert!(report.warnings[0].contains("#4qja"));
        assert!(report.warnings[1].contains("--done 4qja"));
    }

    #[test]
    fn session_check_pending_done_defaults_to_strict_for_session_docs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("--done 4qja"));
                assert!(message.contains("pending_done_guard = \"warn\""));
            }
            other => panic!("expected default strict-mode failure for session doc, got {other:?}"),
        }
    }

    #[test]
    fn session_check_blocks_malformed_tracked_item_before_done_guard() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #pcops — gpt-5\n\nImplemented #pcops.\nVerification:\n- cargo test\n",
            false,
            Some("_- [ ] [#pcops] Project controller ops\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("malformed tracked checklist item"));
                assert!(message.contains("#pcops"));
                assert!(message.contains("_- [ ] [#pcops]"));
            }
            other => panic!("expected malformed tracked-item failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_skips_pending_done_warning_when_id_was_recorded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &["4qja"],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_pending_done_ignores_prose_citation_of_open_id() {
        // #pending-done-guard-false-positive: a response that COMPLETES the head
        // (heading resolves to #cur) but merely CITES another open id (#other) in
        // prose with nearby completion words must NOT demand `--done #other`.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: do #cur — gpt-5\n\nImplemented and committed the fix. Relates to #other, which was fixed in a prior cycle and stays gated.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#cur] current head\n- [ ] [#other] cited-but-not-completed item\n"),
            &["cur"],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(
            matches!(report.status, SessionCheckStatus::Ok(_)),
            "prose citation of #other must not trip the done guard: {:?}",
            report.status
        );
        assert!(
            report.warnings.is_empty(),
            "no done-guard warning expected for a merely-cited id: {:?}",
            report.warnings
        );
    }

    #[test]
    fn session_check_skips_pending_done_warning_when_id_was_kept_open_by_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #fvtg rescope — gpt-5\n\nUpdated the tracked work item to keep the release validation follow-up open.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#fvtg] Release validation follow-up\n"),
            &[],
        );
        crate::cycle_state::record_pending_kept_open_ids(&doc, &["fvtg".to_string()])
            .unwrap()
            .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_skips_pending_done_warning_when_id_was_gated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #qew8 external gate — gpt-5\n\nImplemented the guarded path and left #qew8 gated for rollout verification.\nVerification:\n- cargo test\n",
            false,
            Some("- [/] [#qew8] Await rollout verification\n"),
            &[],
        );
        crate::cycle_state::record_pending_kept_open_ids(&doc, &["#QEW8".to_string()])
            .unwrap()
            .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_pending_done_detects_do_heading_with_later_completion_evidence() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            concat!(
                "### Re: do [#arsq] [#axid] [#rprd] — gpt-5\n\n",
                "Handled the requested docs batch.\n",
                "\n",
                "Changed files:\n",
                "- docs/orbit.md\n",
                "- specs/database.md\n",
                "- prds/livekit.md\n",
                "\n",
                "Commit: abc1234\n",
                "Pushed to origin/dev.\n"
            ),
            false,
            Some(
                "- [ ] [#arsq] Orbit agent tool discriminator\n- [ ] [#axid] Database discriminator section\n- [ ] [#rprd] Relationship PRDs\n",
            ),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("--done arsq"));
                assert!(message.contains("--done axid"));
                assert!(message.contains("--done rprd"));
            }
            other => {
                panic!("expected strict-mode failure for do-heading completions, got {other:?}")
            }
        }
    }

    #[test]
    fn session_check_backlog_replay_guard_accepts_reaped_ids_from_cycle_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: `#done1` manual backlog completion — gpt-5\n\nReaped the user-marked done backlog item.\n",
            false,
            Some("- [ ] [#keep1] Keep backlog item\n"),
            &[],
        );
        let baseline = tmp.path().join(".agent-doc/baselines");
        std::fs::create_dir_all(&baseline).unwrap();
        let canonical = std::fs::canonicalize(&doc).unwrap();
        let hash = crate::snapshot::doc_hash(&canonical).unwrap();
        std::fs::write(
            baseline.join(format!("{hash}.md")),
            concat!(
                "---\nagent_doc_session: test\n---\n\n",
                "## Exchange\n\nHello\n",
                "\n<!-- agent:pending -->\n",
                "- [/] [#done1] Waiting on manual validation\n",
                "- [ ] [#keep1] Keep backlog item\n",
                "<!-- /agent:pending -->\n"
            ),
        )
        .unwrap();
        crate::cycle_state::record_reaped_pending_ids(&doc, &["done1".to_string()])
            .unwrap()
            .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
    }

    #[test]
    fn session_check_pending_done_detects_icebox_only_open_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_tracked_work(
            tmp.path(),
            None,
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented the parked follow-up and verified it.\n",
            false,
            Some("- [ ] [#keep1] Keep backlog item\n"),
            Some("- [ ] [#ice01] Parked follow-up\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("--done ice01"));
                assert!(message.contains("#ice01"));
            }
            other => {
                panic!("expected strict-mode failure for icebox-only tracked work, got {other:?}")
            }
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_backlog_items_remain() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: `#reap1` — gpt-5\n\nImplemented.\n",
            false,
            Some("- [x] [#reap1] Completed but not reaped\n"),
            &[],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#reap1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_backlog_items_were_recorded_this_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: `#reap1` — gpt-5\n\nImplemented.\n",
            false,
            Some("- [x] [#reap1] Completed but stranded after closeout\n"),
            &["reap1"],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#reap1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_icebox_items_remain() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_tracked_work(
            tmp.path(),
            None,
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented.\n",
            false,
            Some("- [ ] [#keep1] Keep backlog item\n"),
            Some("- [x] [#ice01] Completed but not reaped\n"),
            &["ice01"],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#ice01"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    // `#do-id-closeout-open-backlog`: a resolved `do [#id]` directive must end
    // with an explicit lifecycle outcome for its target id.
    #[allow(clippy::too_many_arguments)]
    fn setup_committed_do_directive_cycle(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        pending_body: Option<&str>,
        expect_ids: &[&str],
        done_ids: &[&str],
        kept_open_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let prefix = frontmatter.unwrap_or("---\nagent_doc_session: test\n---\n\n");
        let mut current = format!("{prefix}## Exchange\n\nHello\n");
        if let Some(pending_body) = pending_body {
            current.push_str("\n<!-- agent:backlog -->\n");
            current.push_str(pending_body);
            if !pending_body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:backlog -->\n");
        }
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if !expect_ids.is_empty() {
            crate::cycle_state::record_expect_done_or_gate_ids(
                &doc,
                &expect_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if !done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(
                &doc,
                &done_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if !kept_open_ids.is_empty() {
            crate::cycle_state::record_pending_kept_open_ids(
                &doc,
                &kept_open_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&current), Some(&current))
            .unwrap();
        crate::capture::mark_committed(&doc).unwrap();
        doc
    }

    // `#blocked-closeout-followup-capture`: a directed `do [#id]` cycle that
    // gated its target while reporting blocked/remaining work.
    #[allow(clippy::too_many_arguments)]
    fn setup_blocked_closeout_cycle(
        root: &Path,
        response: &str,
        review_body: Option<&str>,
        backlog_body: Option<&str>,
        expect_ids: &[&str],
        gated_ids: &[&str],
        kept_open_ids: &[&str],
        added: bool,
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let mut current =
            String::from("---\nagent_doc_session: test\n---\n\n## Exchange\n\nHello\n");
        if let Some(body) = backlog_body {
            current.push_str("\n<!-- agent:backlog -->\n");
            current.push_str(body);
            if !body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:backlog -->\n");
        }
        if let Some(body) = review_body {
            current.push_str("\n<!-- agent:review -->\n");
            current.push_str(body);
            if !body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:review -->\n");
        }
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if !expect_ids.is_empty() {
            crate::cycle_state::record_expect_done_or_gate_ids(
                &doc,
                &expect_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if !gated_ids.is_empty() {
            crate::cycle_state::record_pending_gated_ids(
                &doc,
                &gated_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if !kept_open_ids.is_empty() {
            crate::cycle_state::record_pending_kept_open_ids(
                &doc,
                &kept_open_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if added {
            crate::cycle_state::mark_pending_added(&doc).unwrap();
        }
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&current), Some(&current))
            .unwrap();
        crate::capture::mark_committed(&doc).unwrap();
        doc
    }

    const BLOCKED_RESPONSE: &str = "### Re: do #374n — gpt-5\n\nFound a blocker: Merchant Center still has 17 active legacy rows for #374n. Next steps to complete: remove/expire the rows, deliberately delete them through an approved path, or get approval that they are safe blanks.\n";

    #[test]
    fn blocked_closeout_followup_guard_fails_when_gated_without_followup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            BLOCKED_RESPONSE,
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            false,
        );
        match check_blocked_closeout_followup_guard(&doc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("#374n"), "{message}");
                assert!(message.contains("--pending-edit"), "{message}");
            }
            other => {
                panic!("expected strict failure for blocked gate without follow-up, got {other:?}")
            }
        }
    }

    #[test]
    fn blocked_closeout_followup_guard_wired_into_inspect() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            BLOCKED_RESPONSE,
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            false,
        );
        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("#374n"), "{message}")
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    // `#gated-followup-split-enforcement`: kept-open parent item whose body
    // enumerates multiple gated phases without discrete child ids.
    const MULTI_PHASE_REVIEW: &str = "- [/] [#parentfix] [recommended] Follow-ups from the parent fix. Phase 1 landed. Remaining (gated — needs a live pane): (2b) a live Stop-hook regression asserting in-pane output; (3) live-verify a real same-pane run. Plan: tasks/x.md\n";
    const SPLIT_RESPONSE: &str = "### Re: do #parentfix — gpt-5\n\nLanded phase 1 and kept the remaining phases noted on the item.\n";

    #[test]
    fn gated_phase_split_guard_warns_on_multi_phase_kept_open_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            SPLIT_RESPONSE,
            Some(MULTI_PHASE_REVIEW),
            None,
            &["parentfix"],
            &["parentfix"],
            &["parentfix"],
            false,
        );
        match check_gated_phase_split_guard(&doc).unwrap() {
            GuardResult::Warn(lines) => {
                assert!(lines.iter().any(|l| l.contains("#parentfix")), "{lines:?}");
                assert!(
                    lines
                        .iter()
                        .any(|l| l.contains("discrete child backlog IDs")),
                    "{lines:?}"
                );
            }
            other => panic!("expected split-advisory warning, got {other:?}"),
        }
    }

    #[test]
    fn gated_phase_split_guard_quiet_when_phases_already_split_into_child_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Same multi-phase shape, but the phases are already broken out into two
        // discrete child ids — the split work is done, so stay quiet.
        let already_split = "- [/] [#parentfix] [recommended] Remaining gated phases tracked as children: phase (2b) -> #childb, phase (3) -> #childc. Plan: tasks/x.md\n";
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            SPLIT_RESPONSE,
            Some(already_split),
            None,
            &["parentfix"],
            &["parentfix"],
            &["parentfix"],
            false,
        );
        assert!(matches!(
            check_gated_phase_split_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn gated_phase_split_guard_quiet_for_single_phase_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let single = "- [/] [#parentfix] [recommended] One remaining gated phase: live-verify the fix on a real pane. Plan: tasks/x.md\n";
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            SPLIT_RESPONSE,
            Some(single),
            None,
            &["parentfix"],
            &["parentfix"],
            &["parentfix"],
            false,
        );
        assert!(matches!(
            check_gated_phase_split_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn gated_phase_split_guard_suppressed_by_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            "### Re: do #parentfix — gpt-5\n\nKept phases as one unit. <!-- no-gated-phase-split-guard -->\n",
            Some(MULTI_PHASE_REVIEW),
            None,
            &["parentfix"],
            &["parentfix"],
            &["parentfix"],
            false,
        );
        assert!(matches!(
            check_gated_phase_split_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn gated_phase_split_guard_is_advisory_not_blocking() {
        // The guard only warns — a multi-phase kept-open item must not interrupt
        // closeout (warn-first), so `inspect` still reports Ok with the warning.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            SPLIT_RESPONSE,
            Some(MULTI_PHASE_REVIEW),
            None,
            &["parentfix"],
            &[],
            &["parentfix"],
            false,
        );
        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Ok(_) => {}
            other => panic!("split advisory must not block closeout, got {other:?}"),
        }
    }

    // `#queue-audit-partial-completion`: a queue-completion audit that collapses
    // partial progress into "none complete."
    #[test]
    fn queue_audit_guard_warns_when_none_complete_collapses_partial_progress() {
        let tmp = tempfile::TempDir::new().unwrap();
        let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the six queue items are complete. Same-day QA is complete and the URL validate-only check was clean, but each row still has at least one remaining action.\n";
        let doc =
            setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
        match check_queue_audit_partial_completion_guard(&doc).unwrap() {
            GuardResult::Warn(lines) => {
                assert!(
                    lines.iter().any(|l| l.contains("partially complete")),
                    "{lines:?}"
                );
            }
            other => panic!("expected queue-audit collapse warning, got {other:?}"),
        }
    }

    #[test]
    fn queue_audit_guard_quiet_when_partial_states_already_given() {
        let tmp = tempfile::TempDir::new().unwrap();
        let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the queue items are fully complete, but several are partially complete: same-day QA is complete and the validate-only check was clean, each with one remaining action.\n";
        let doc =
            setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
        assert!(matches!(
            check_queue_audit_partial_completion_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn queue_audit_guard_quiet_when_not_about_queue() {
        let tmp = tempfile::TempDir::new().unwrap();
        let response = "### Re: status — gpt-5\n\nNone of the migration steps are complete. The schema dump is complete and the backup was clean.\n";
        let doc =
            setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
        assert!(matches!(
            check_queue_audit_partial_completion_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn queue_audit_guard_quiet_without_extra_completion_evidence() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Blanket none-complete with no additional substep-completion evidence is
        // a legitimate "nothing done yet" answer, not a collapse.
        let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the queue items are complete yet; every row is still blocked on input.\n";
        let doc =
            setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
        assert!(matches!(
            check_queue_audit_partial_completion_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn queue_audit_guard_suppressed_by_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the six queue items are complete. Same-day QA is complete and the check was clean. <!-- no-queue-audit-guard -->\n";
        let doc =
            setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
        assert!(matches!(
            check_queue_audit_partial_completion_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn blocked_closeout_followup_guard_passes_when_new_followup_added() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            BLOCKED_RESPONSE,
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            true,
        );
        assert!(matches!(
            check_blocked_closeout_followup_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn blocked_closeout_followup_guard_passes_when_kept_open_in_backlog() {
        let tmp = tempfile::TempDir::new().unwrap();
        // `--pending-edit` keeps the id in agent:backlog and records no gate.
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            BLOCKED_RESPONSE,
            None,
            Some("- [ ] [#374n] Removal cleanup — remove/expire 17 legacy rows\n"),
            &["374n"],
            &[],
            &["374n"],
            false,
        );
        assert!(matches!(
            check_blocked_closeout_followup_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn blocked_closeout_followup_guard_passes_with_explicit_no_followup_phrase() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            "### Re: do #374n — gpt-5\n\nImplementation complete for #374n; awaiting code review. No additional backlog follow-up is needed because the remaining rows are still blocked on review only.\n",
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            false,
        );
        assert!(matches!(
            check_blocked_closeout_followup_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn blocked_closeout_followup_guard_passes_for_clean_review_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            "### Re: do #374n — gpt-5\n\nImplementation complete for #374n and pushed; ready for review.\n",
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            false,
        );
        assert!(matches!(
            check_blocked_closeout_followup_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn blocked_closeout_followup_guard_ignores_blocked_phrase_not_tied_to_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Blocked phrasing exists but does not mention the gated directed id.
        let doc = setup_blocked_closeout_cycle(
            tmp.path(),
            "### Re: do #374n — gpt-5\n\nDone. Separately, an unrelated PR remains blocked on CI but that is not part of this work.\n",
            Some("- [/] [#374n] Removal cleanup\n"),
            None,
            &["374n"],
            &["374n"],
            &[],
            false,
        );
        assert!(matches!(
            check_blocked_closeout_followup_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn do_directive_target_ids_extracts_bracketed_and_bare_forms() {
        let prompts = vec![
            "do [#alpha]".to_string(),
            "❯ do #beta".to_string(),
            "[queue] do #gamma".to_string(),
            "investigate #delta".to_string(),
        ];
        let ids = do_directive_target_ids(&prompts);
        assert_eq!(ids, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn do_directive_target_ids_optional_do_stage2_bare_and_reference_forms() {
        // Optional-`do` Stage 2: the `do` verb is optional for a bare leading id
        // token, and a `re` reference never targets an id.
        let prompts = vec![
            "[#solo]".to_string(),                      // bare bracketed → id-backed
            "- [#listed] do the small fix".to_string(), // bare after list marker
            "#hashbare proceed".to_string(),            // bare hash token
            "re [#ref]".to_string(),                    // reference → inert
            "re #ref2".to_string(),                     // reference → inert
            "[#note]: just prose".to_string(),          // trailing `:` → inert
            "see [#mention] for context".to_string(),   // not leading → inert
            "do [#explicit]".to_string(),               // explicit still works
        ];
        let ids = do_directive_target_ids(&prompts);
        assert_eq!(ids, vec!["solo", "listed", "hashbare", "explicit"]);
    }

    #[test]
    fn expect_done_or_gate_guard_fails_when_directive_target_left_open() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nShipped the repo/API/deploy work and cleared the queue.\n",
            Some("- [ ] [#nstep2] Tracked work the directive resolved\n"),
            &["nstep2"],
            &[],
            &[],
        );

        match check_expect_done_or_gate_guard(&doc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("#nstep2"), "{message}");
                assert!(message.contains("--done nstep2"), "{message}");
                assert!(message.contains("agent:backlog"), "{message}");
            }
            other => {
                panic!("expected strict-mode failure for open directive target, got {other:?}")
            }
        }
    }

    #[test]
    fn expect_done_or_gate_guard_is_wired_into_inspect() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCompleted.\n",
            Some("- [ ] [#nstep2] Tracked work the directive resolved\n"),
            &["nstep2"],
            &[],
            &[],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("#nstep2"), "{message}");
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn expect_done_or_gate_guard_passes_when_target_marked_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        // `--done` reaps the item, so it is no longer open in the backlog and is
        // also recorded in `pending_done_ids`.
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCompleted.\n",
            Some("- [ ] [#keep1] Unrelated open item\n"),
            &["nstep2"],
            &["nstep2"],
            &[],
        );

        assert!(matches!(
            check_expect_done_or_gate_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn expect_done_or_gate_guard_passes_when_target_gated_to_review() {
        let tmp = tempfile::TempDir::new().unwrap();
        // `--pending-gate` moves the item out of backlog into review and records
        // it as kept-open for the cycle.
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nImplemented; awaiting review.\n",
            Some("- [ ] [#keep1] Unrelated open item\n"),
            &["nstep2"],
            &[],
            &["nstep2"],
        );

        assert!(matches!(
            check_expect_done_or_gate_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn expect_done_or_gate_guard_does_not_fire_on_unrelated_open_backlog() {
        let tmp = tempfile::TempDir::new().unwrap();
        // No directive recorded this cycle (incidental open backlog only).
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: investigation — gpt-5\n\nLooked into it; relates to #keep1.\n",
            Some("- [ ] [#keep1] Open and intentionally left open\n"),
            &[],
            &[],
            &[],
        );

        assert!(matches!(
            check_expect_done_or_gate_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn expect_done_or_gate_guard_off_mode_skips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: off\n---\n\n"),
            "### Re: do #nstep2 — gpt-5\n\nCompleted.\n",
            Some("- [ ] [#nstep2] Tracked work left open\n"),
            &["nstep2"],
            &[],
            &[],
        );

        assert!(matches!(
            check_expect_done_or_gate_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn partial_closeout_state_guard_warns_on_shipped_with_remaining_live_work() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCommitted and pushed the repo + tests. Live deploy and live verification remain; not deployed yet.\n",
            Some("- [ ] [#nstep2] Original full-task text the directive resolved\n"),
            &["nstep2"],
            &[],
            // Kept open (gated/kept) but the item text was not narrowed.
            &["nstep2"],
        );

        match check_partial_closeout_state_guard(&doc).unwrap() {
            GuardResult::Warn(lines) => {
                let joined = lines.join("\n");
                assert!(joined.contains("#nstep2"), "{joined}");
                assert!(joined.contains("--pending-edit"), "{joined}");
                assert!(
                    joined.contains("next phase") || joined.contains("next-phase"),
                    "{joined}"
                );
            }
            other => panic!("expected WARN for partial closeout, got {other:?}"),
        }
    }

    #[test]
    fn partial_closeout_state_guard_silent_without_remaining_signal() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCommitted and pushed. Completed the full task.\n",
            Some("- [ ] [#nstep2] Tracked work\n"),
            &["nstep2"],
            &[],
            &["nstep2"],
        );

        assert!(matches!(
            check_partial_closeout_state_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn partial_closeout_state_guard_suppressed_by_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCommitted and pushed; live verification remains.\n<!-- no-partial-closeout-guard -->\n",
            Some("- [ ] [#nstep2] Narrowed to next phase\n"),
            &["nstep2"],
            &[],
            &["nstep2"],
        );

        assert!(matches!(
            check_partial_closeout_state_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn partial_closeout_state_guard_silent_when_target_reaped() {
        let tmp = tempfile::TempDir::new().unwrap();
        // `--done` reaps the item; partial-completion prose must not warn about a
        // target that is no longer open in agent:backlog.
        let doc = setup_committed_do_directive_cycle(
            tmp.path(),
            None,
            "### Re: do #nstep2 — gpt-5\n\nCommitted and pushed; not deployed yet.\n",
            Some("- [ ] [#keep1] Unrelated open item\n"),
            &["nstep2"],
            &["nstep2"],
            &[],
        );

        assert!(matches!(
            check_partial_closeout_state_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    fn partial_staging_git(root: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_partial_staging_repo(root: &std::path::Path) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();

        partial_staging_git(root, &["init"]);
        partial_staging_git(root, &["config", "user.email", "test@example.com"]);
        partial_staging_git(root, &["config", "user.name", "Test"]);

        let doc = root.join("doc.md");
        let doc_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nHello\n";
        fs::write(&doc, doc_content).unwrap();
        crate::snapshot::save(&doc, doc_content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(doc_content), Some(doc_content)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(doc_content),
            Some(doc_content),
        )
        .unwrap();

        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"old queue output\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"old queue output\"); }\n",
        )
        .unwrap();
        partial_staging_git(
            root,
            &["add", "doc.md", "src/render.rs", "tests/render_test.rs"],
        );
        partial_staging_git(root, &["commit", "-m", "initial", "--no-verify"]);

        doc
    }

    fn commit_partial_staging_source_change(root: &std::path::Path) {
        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"new queue output\" }\n",
        )
        .unwrap();
        partial_staging_git(root, &["add", "src/render.rs"]);
        partial_staging_git(root, &["commit", "-m", "source only", "--no-verify"]);
    }

    #[test]
    fn partial_staging_closeout_guard_warns_on_dirty_companion_test_literal() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = setup_partial_staging_repo(root);

        commit_partial_staging_source_change(root);

        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"new queue output\"); }\n",
        )
        .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        let joined = report.warnings.join("\n");
        assert!(joined.contains("partial staging closeout"), "{joined}");
        assert!(joined.contains("src/render.rs"), "{joined}");
        assert!(joined.contains("tests/render_test.rs"), "{joined}");
        assert!(joined.contains("new queue output"), "{joined}");
    }

    #[test]
    fn partial_staging_closeout_guard_warns_on_dirty_same_file_literal() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = setup_partial_staging_repo(root);

        commit_partial_staging_source_change(root);
        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"new queue output\" /* missed cleanup */ }\n",
        )
        .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        let joined = report.warnings.join("\n");
        assert!(joined.contains("partial staging closeout"), "{joined}");
        assert!(joined.contains("src/render.rs"), "{joined}");
        assert!(joined.contains("new queue output"), "{joined}");
    }

    #[test]
    fn partial_staging_closeout_guard_quiet_when_committed_tree_is_clean() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = setup_partial_staging_repo(root);

        commit_partial_staging_source_change(root);

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        let joined = report.warnings.join("\n");
        assert!(!joined.contains("partial staging closeout"), "{joined}");
    }

    #[test]
    fn partial_staging_closeout_guard_ignores_cross_document_markdown_noise() {
        // #partial-staging-guard-cross-doc-noise: a markdown-document commit plus a
        // dirty companion markdown doc sharing incidental prose vocabulary (e.g.
        // `make check`) must NOT trip the source+test partial-staging guard, which
        // previously WARNed on nearly every closeout in a multi-session superproject.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = setup_partial_staging_repo(root);

        // Latest commit changes only a markdown doc carrying a common phrase.
        fs::write(
            root.join("notes_a.md"),
            "Run `make check` before committing the agent-doc change.\n",
        )
        .unwrap();
        partial_staging_git(root, &["add", "notes_a.md"]);
        partial_staging_git(root, &["commit", "-m", "notes a", "--no-verify"]);

        // A dirty companion markdown doc shares the same incidental phrase.
        fs::write(
            root.join("notes_b.md"),
            "Reminder: `make check` is required for the agent-doc workflow.\n",
        )
        .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        let joined = report.warnings.join("\n");
        assert!(
            !joined.contains("partial staging closeout"),
            "markdown cross-document vocabulary must not trip the source+test guard:\n{joined}"
        );
    }

    #[test]
    fn session_check_interrupts_when_open_backlog_item_exists_only_in_shadow_copy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: off\n---\n\n"),
            "### Re: backlog shadow check — gpt-5\n\nInvestigated.\n",
            false,
            Some("- [ ] [#keep1] Keep live\n"),
            &[],
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&doc)
            .unwrap()
            .write_all(b"\n<!-- parked digest\n- [ ] [#lost1] Drifted copy\n-->\n")
            .unwrap();

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("open backlog item(s) exist only outside"));
                assert!(message.contains("#lost1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_warns_when_live_backlog_item_has_shadow_copy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: off\n---\n\n"),
            "### Re: backlog shadow check — gpt-5\n\nInvestigated.\n",
            false,
            Some("- [ ] [#keep1] Keep live\n"),
            &[],
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&doc)
            .unwrap()
            .write_all(b"\n<!-- parked digest\n- [ ] [#keep1] Duplicate copy\n-->\n")
            .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("outside live agent:backlog"));
        assert!(report.warnings[0].contains("#keep1"));
    }

    #[test]
    fn session_check_pending_done_strict_mode_blocks_missing_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: strict\n---\n\n"),
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("--done 4qja"));
                assert!(message.contains("pending_done_guard = \"warn\""));
            }
            other => panic!("expected strict-mode failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_pending_done_suppression_marker_disables_guard() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\n<!-- no-pending-done-guard -->\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_snapshot_committed_guard_fails_when_snapshot_differs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let old_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n";
        fs::write(&doc, old_content).unwrap();
        crate::snapshot::save(&doc, old_content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate: write applied a response to the snapshot but commit never
        // happened AND the user typed a new prompt on top, so the working tree
        // diverges from the snapshot. This is the "true direct patchback"
        // shape — distinct from the Phase 3 (#jbccc3) jb_cache_conflict_cancel
        // pattern (doc ≈ snapshot) which is now auto-recoverable.
        let snapshot_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n";
        let working_tree = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n\n❯ extra user prompt that diverges from snapshot\n";
        fs::write(&doc, working_tree).unwrap();
        crate::snapshot::save(&doc, snapshot_content).unwrap();

        // Mark cycle as committed (simulating a bug where cycle_state lied)
        crate::cycle_state::start_preflight(&doc, Some(old_content), Some(old_content)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(snapshot_content),
            Some(snapshot_content),
        )
        .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(
                    msg.contains("snapshot does not match HEAD")
                        || msg.contains("uncommitted exchange changes")
                        || msg.contains("unresolved prompt-bearing"),
                    "expected uncommitted closeout guard failure, got: {msg}"
                );
            }
            SessionCheckStatus::Ok(msg) => {
                panic!("expected Interrupted, got Ok: {msg}");
            }
        }
    }

    /// Phase 3 (#jbccc3): the jb_cache_conflict_cancel pattern — cycle marked
    /// Committed, snapshot has the response, HEAD does not, working tree
    /// matches snapshot — must now be reported as OK by session-check so
    /// preflight can transparently auto-commit on the next invocation. Before
    /// Phase 3 this same shape surfaced as "snapshot does not match HEAD"
    /// (misclassifying the JB-cache-conflict cancel as a missing commit).
    #[test]
    fn session_check_snapshot_committed_guard_skips_jb_cache_conflict_cancel_pattern() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let old_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n";
        fs::write(&doc, old_content).unwrap();
        crate::snapshot::save(&doc, old_content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Cancel pattern: snapshot and working tree both have the response,
        // HEAD does not, cycle marked Committed.
        let new_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n";
        fs::write(&doc, new_content).unwrap();
        crate::snapshot::save(&doc, new_content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(old_content), Some(old_content)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(new_content),
            Some(new_content),
        )
        .unwrap();

        assert!(
            detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: cancel pattern must be detected"
        );
        let status = inspect(&doc).unwrap();
        assert!(
            matches!(status, SessionCheckStatus::Ok(_)),
            "expected Ok (auto-recoverable), got: {status:?}"
        );
    }

    #[test]
    fn session_check_classifies_jb_cache_conflict_accept_duplicate_replay() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #gsqlwrite — gpt-5\n\n",
            "Committed response.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed response", "--no-verify"])
            .output()
            .unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        let replayed = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: #gsqlwrite — gpt-5 (HEAD)\n\nCommitted response.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, replayed).unwrap();

        let replay = detect_jb_cache_conflict_accept_duplicate_replay(&doc)
            .unwrap()
            .expect("duplicate replay should be detected");
        assert_eq!(replay.deduped_content, committed);
        assert_eq!(replay.heading, "### Re: #gsqlwrite — gpt-5 (HEAD)");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("File Cache Conflict accept replay duplicate"),
                    "expected dedicated accept-replay classification: {message}"
                );
                assert!(
                    message.contains("matches committed HEAD"),
                    "expected committed-HEAD proof in message: {message}"
                );
            }
            other => panic!("expected accept replay interruption, got {other:?}"),
        }
    }

    #[test]
    fn recursive_direct_invocation_abandoned_cycle_passes_session_check() {
        // #recguard-abandon: when the recursive same-pane guard refuses to
        // dispatch, it abandons the empty preflight cycle (terminal) instead of
        // leaving it `preflight_started`. session-check must then accept the
        // terminal abandoned state — no manual `agent-doc cancel` required.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        // Run opened a preflight cycle, then the recursive guard fired before any
        // response capture and abandoned it.
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_abandoned(
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(
                    message.contains("abandoned"),
                    "terminal abandoned cycle should report an abandoned OK state: {message}"
                );
            }
            other => {
                panic!("abandoned recursive-guard cycle must pass session-check, got {other:?}")
            }
        }
    }

    #[test]
    fn recursive_abandoned_cycle_with_unresolved_prompt_reports_missed_prompt() {
        // #codex-owned-pane-prompt-miss: an abandoned recursive-guard cycle is
        // NOT sufficient closeout when an unresolved exchange prompt still
        // remains. session-check must fail closed with a missed-prompt recovery
        // path instead of accepting the abandoned cycle as OK.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        // Exchange tail after the boundary is an unanswered user prompt.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "Please assist in placing GA4 Analytics credentials in passage.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_abandoned(
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("unresolved exchange prompt"),
                    "expected missed-prompt classification: {message}"
                );
                assert!(
                    message.contains("GA4 Analytics"),
                    "expected the unresolved prompt excerpt in the message: {message}"
                );
                assert!(
                    message.contains("write --commit"),
                    "expected the recovery path in the message: {message}"
                );
            }
            other => panic!("expected missed-prompt interruption, got {other:?}"),
        }
    }

    #[test]
    fn codex_final_gate_adopts_manual_patchback_when_response_is_visible() {
        // #finalize-owned-pane-response-patchback: when a recursive same-pane
        // invocation was blocked (abandoned cycle, no capture), but the
        // response was already patched into agent:exchange manually, the
        // codex_final_gate must NOT block — adopt the visible response
        // idempotently.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        // The response was manually patched into exchange after the boundary,
        // so the prompt IS answered (no unresolved exchange prompt).
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "Please assist in placing GA4 Analytics credentials.\n",
            "### Re: GA4 — codex\n\nDone. Credentials placed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_abandoned(
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // The response is visible in exchange (no unresolved prompt), so
        // codex_final_gate should adopt it instead of blocking.
        match run_with_options(&doc, true) {
            Ok(()) => {}
            other => panic!("expected codex_final_gate to adopt manual patchback, got {other:?}"),
        }
    }

    #[test]
    fn session_check_classifies_late_ipc_response_overapplication() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        // Two distinct committed responses A, B in HEAD.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — gpt-5\n\nAnswer A.\n",
            "### Re: second — gpt-5\n\nAnswer B.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed responses", "--no-verify"])
            .output()
            .unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Late-IPC replay re-adds the EARLIER response A at the tail — a
        // non-consecutive duplicate the JB-cache replay detector misses.
        let overapplied = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: first — gpt-5\n\nAnswer A.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, overapplied).unwrap();

        assert!(
            detect_jb_cache_conflict_accept_duplicate_replay(&doc)
                .unwrap()
                .is_none(),
            "non-adjacent duplicate is not a consecutive accept-replay"
        );
        assert!(
            detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "late-IPC over-application should be detected"
        );

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("late-IPC committed-response over-application"),
                    "expected late-IPC over-application classification: {message}"
                );
                assert!(
                    !message.contains("direct response patchback"),
                    "must not misclassify over-application as a manual patchback: {message}"
                );
            }
            other => panic!("expected late-IPC over-application interruption, got {other:?}"),
        }
    }

    #[test]
    fn detects_prompt_prefixed_corrupted_duplicate_as_overapplication() {
        // #finalize-retry-ipc-response-duplication: a multi-retry / late-IPC
        // reposition left a duplicate response whose stale copy had its body
        // wrongly prefixed with `❯ `. HEAD still holds a single clean copy, so
        // the over-application detector must recognize the corrupted duplicate
        // and remediate by restoring committed HEAD — no manual `git checkout`.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do [#fix-thing]\n",
            "### Re: fix thing — gpt-5\n\n",
            "**Scope:** narrow.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed response", "--no-verify"])
            .output()
            .unwrap();

        // Working tree gains a stale duplicate whose body line is `❯ `-prefixed.
        let corrupted = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: fix thing — gpt-5\n\n❯ **Scope:** narrow.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, corrupted).unwrap();

        let overapplication = detect_late_ipc_response_overapplication(&doc)
            .unwrap()
            .expect("prompt-prefixed corrupted duplicate must be detected");
        assert_eq!(
            overapplication.remediated_content, committed,
            "remediation must restore the clean committed HEAD"
        );
    }

    #[test]
    fn session_check_fails_closed_on_dropped_exchange_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        // HEAD = content_ours: the assistant response, but NOT the user's "go".
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        for args in [
            vec!["add", "doc.md"],
            vec!["commit", "-m", "ours", "--no-verify"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        // Adoption-time evidence: the user's "go" was dropped into content_ours.
        crate::cycle_state::record_dropped_exchange_prompts(&doc, &["go".to_string()]).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("dropped during an IPC content_ours merge"),
                    "expected dropped-prompt classification: {message}"
                );
                assert!(
                    message.contains("go"),
                    "should name the dropped prompt: {message}"
                );
            }
            other => panic!("expected dropped-prompt interruption, got {other:?}"),
        }
    }

    #[test]
    fn session_check_clears_dropped_prompt_marker_once_in_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        // HEAD now DOES contain the previously-dropped prompt "go" (a later cycle
        // recovered it), so the recorded marker is stale and must clear.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "go\n",
            "### Re: go — gpt-5\n\nStarted.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        for args in [
            vec!["add", "doc.md"],
            vec!["commit", "-m", "recovered", "--no-verify"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        crate::cycle_state::record_dropped_exchange_prompts(&doc, &["go".to_string()]).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard should clear when the dropped prompt is present in HEAD"
        );
        assert!(
            crate::cycle_state::load(&doc)
                .unwrap()
                .expect("state")
                .dropped_exchange_prompts
                .is_empty(),
            "resolved marker should be cleared"
        );
    }

    fn init_committed_doc_for_queue_guard(root: &Path, committed: &str) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        let doc = root.join("doc.md");
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        for args in [
            vec!["add", "doc.md"],
            vec!["commit", "-m", "ours", "--no-verify"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        doc
    }

    #[test]
    fn session_check_fails_closed_on_dropped_queue_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        // HEAD = content_ours: queue lacks the user-added head, no consumption.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue auto -->\n- do [#existing]\n<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#gscaccess]".to_string()])
            .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("agent:queue edit(s) were dropped"),
                    "expected dropped-queue classification: {message}"
                );
                assert!(
                    message.contains("gscaccess"),
                    "should name the dropped queue edit: {message}"
                );
            }
            other => panic!("expected dropped-queue interruption, got {other:?}"),
        }
    }

    #[test]
    fn session_check_clears_dropped_queue_marker_when_preserved_in_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        // HEAD's queue DOES contain the user-added head — preserved, marker stale.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue auto -->\n- do [#gscaccess]\n- do [#existing]\n<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#gscaccess]".to_string()])
            .unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard should clear when the dropped queue edit is preserved in HEAD"
        );
        assert!(
            crate::cycle_state::load(&doc)
                .unwrap()
                .expect("state")
                .dropped_queue_prompts
                .is_empty(),
            "resolved marker should be cleared"
        );
    }

    #[test]
    fn session_check_clears_dropped_queue_marker_when_consumed_this_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        // HEAD's queue lacks the head because the response consumed #gscaccess
        // (recorded as done this cycle) — legitimate, not a silent deletion.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: do #gscaccess — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue auto -->\n- do [#existing]\n<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#gscaccess]".to_string()])
            .unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["gscaccess".to_string()]).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard should clear when the dropped queue head was consumed this cycle"
        );
    }

    // `#queue-clear-unrun-items` — committed doc with the six monsterrodholders
    // heads removed from the queue while their backlog items stay open, the
    // convqa head consumed/done. Recorded preflight heads = all six.
    fn queue_clear_fixture(queue_body: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: do #convqa-rerun — gpt-5\n\nRefreshed the conversion QA gate.\n",
                "<!-- /agent:exchange -->\n\n",
                "## Backlog\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#hydroapproval] approve hydro listing\n",
                "- [ ] [#nbapproval] approve nb listing\n",
                "- [ ] [#shopcachewatch] watch shop cache\n",
                "- [ ] [#shoplabelgate] gate shop labels\n",
                "- [ ] [#accessorymargin] recompute accessory margin\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:queue auto -->\n{}<!-- /agent:queue -->\n",
            ),
            queue_body
        )
    }

    const QUEUE_CLEAR_HEADS: &[&str] = &[
        "do [#convqa-rerun]",
        "do [#hydroapproval]",
        "do [#nbapproval]",
        "do [#shopcachewatch]",
        "do [#shoplabelgate]",
        "do [#accessorymargin]",
    ];

    fn record_queue_clear_heads(doc: &Path) {
        let heads: Vec<String> = QUEUE_CLEAR_HEADS.iter().map(|s| s.to_string()).collect();
        crate::cycle_state::record_active_queue_heads(doc, &heads).unwrap();
    }

    fn capture_test_response_and_commit(doc: &Path, response: &str) {
        crate::capture::capture_response(doc, response).unwrap();
        let content = fs::read_to_string(doc).unwrap();
        crate::cycle_state::mark_committed(doc, "commit_success", Some(&content), Some(&content))
            .unwrap();
        crate::capture::mark_committed(doc).unwrap();
    }

    #[test]
    fn queue_head_removal_guard_fails_closed_on_silently_dropped_open_heads() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Queue collapsed to empty; convqa was consumed (done), the other five
        // open backlog heads were dropped without any closeout.
        let committed = queue_clear_fixture("");
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        record_queue_clear_heads(&doc);
        crate::cycle_state::record_pending_done_ids(&doc, &["convqa-rerun".to_string()]).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                for id in [
                    "hydroapproval",
                    "nbapproval",
                    "shopcachewatch",
                    "shoplabelgate",
                    "accessorymargin",
                ] {
                    assert!(
                        message.contains(id),
                        "should name dropped open head #{id}: {message}"
                    );
                }
                assert!(
                    !message.contains("convqa-rerun"),
                    "consumed/done head must not be flagged: {message}"
                );
            }
            other => panic!("expected queue-head-removal interruption, got {other:?}"),
        }
    }

    #[test]
    fn queue_head_removal_guard_allows_consumed_head_when_rest_preserved() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Only the consumed convqa head is removed; the five open heads stay
        // queued — legitimate single-head consumption.
        let committed = queue_clear_fixture(concat!(
            "- do [#hydroapproval]\n",
            "- do [#nbapproval]\n",
            "- do [#shopcachewatch]\n",
            "- do [#shoplabelgate]\n",
            "- do [#accessorymargin]\n",
        ));
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        let current = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        record_queue_clear_heads(&doc);
        crate::cycle_state::record_pending_done_ids(&doc, &["convqa-rerun".to_string()]).unwrap();
        capture_test_response_and_commit(
            &doc,
            "### Re: do #convqa-rerun — gpt-5\n\nRefreshed the conversion QA gate.",
        );

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard must not fire when the only removed head was consumed and the rest stay queued"
        );
    }

    // `#manual-queue-head-loss` — a fixture mirroring the monsterrodholders repro:
    // backlog keeps `#shipstationaudit` open; the committed queue does NOT contain
    // the head (it was dropped during a stalled dispatch). The head was never in
    // the preflight-recorded set; only `observe_live_queue_heads` (the live
    // pre-write working tree the user typed into) makes it visible to the guard.
    fn manual_head_loss_fixture(queue_body: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: prior — gpt-5\n\nAnswered.\n",
                "<!-- /agent:exchange -->\n\n",
                "## Backlog\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#shipstationaudit] audit shipstation sync\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:queue auto -->\n{}<!-- /agent:queue -->\n",
            ),
            queue_body
        )
    }

    #[test]
    fn observe_live_queue_heads_catches_dropped_manual_head_added_after_preflight() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Committed queue keeps an unrelated head; #shipstationaudit was dropped.
        let committed = manual_head_loss_fixture("- do [#unrelated]\n");
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        // preflight recorded only `do [#unrelated]` (no manual head). Simulate the
        // live pre-write working tree the user typed the manual head into.
        let live = manual_head_loss_fixture("- do [#unrelated]\n- do [#shipstationaudit]\n");
        crate::cycle_state::observe_live_queue_heads(&doc, &live).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("shipstationaudit"),
                    "should name the dropped manual head #shipstationaudit: {message}"
                );
            }
            other => panic!("expected manual-head-loss interruption, got {other:?}"),
        }
    }

    #[test]
    fn observe_live_queue_heads_allows_manual_head_still_queued() {
        let tmp = tempfile::TempDir::new().unwrap();
        // The manual head is preserved in the committed queue — no silent drop.
        let committed = manual_head_loss_fixture("- do [#unrelated]\n- do [#shipstationaudit]\n");
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        let live = committed.clone();
        crate::cycle_state::observe_live_queue_heads(&doc, &live).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard must not fire when the manual head stays queued in the committed doc"
        );
    }

    #[test]
    fn observe_live_queue_heads_allows_manual_head_consumed_this_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        // The manual head is gone from the committed queue, but it was consumed
        // and the backlog item marked done this cycle — legitimate removal.
        let committed = manual_head_loss_fixture("- do [#unrelated]\n");
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        let live = manual_head_loss_fixture("- do [#unrelated]\n- do [#shipstationaudit]\n");
        crate::cycle_state::observe_live_queue_heads(&doc, &live).unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["shipstationaudit".to_string()])
            .unwrap();

        // #shipstationaudit is still in `agent:backlog` in the fixture, but the
        // done-id proof for this cycle must clear it from the removal guard.
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    !message.contains("shipstationaudit"),
                    "consumed/done manual head must not be flagged: {message}"
                );
            }
            SessionCheckStatus::Ok(_) => {}
        }
    }

    #[test]
    fn queue_head_removal_guard_suppressed_by_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Heads dropped, but an explicit user-removal marker suppresses the guard.
        let mut committed = queue_clear_fixture("");
        committed.push_str("\n<!-- no-queue-removal-guard -->\n");
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &committed);
        record_queue_clear_heads(&doc);

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "explicit user-removal marker should suppress the queue-head-removal guard"
        );
    }

    #[test]
    fn queue_head_removal_guard_quiet_when_backlog_items_resolved() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Queue empty AND backlog empty: every head's id left agent:backlog, so
        // each deletion is proven (done/gate/reap) — no fire.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: closeout — gpt-5\n\nAll resolved.\n<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
        record_queue_clear_heads(&doc);

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "guard must not fire when the removed heads' backlog items are no longer open"
        );
    }

    #[test]
    fn free_text_queue_head_guard_fires_on_missing_response() {
        let tmp = tempfile::TempDir::new().unwrap();
        let head = "Can CellHandle::set also apply to the multi-threaded Context?";
        let with_head = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n- {head}\n<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &with_head);

        let without_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n<!-- /agent:queue -->\n",
        );
        fs::write(&doc, without_head).unwrap();
        crate::snapshot::save(&doc, without_head).unwrap();

        let result = inspect(&doc).unwrap();
        match result {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(msg.contains("free-text"), "got: {msg}");
            }
            SessionCheckStatus::Ok(warnings) => {
                assert!(warnings.contains("free-text"), "got: {warnings}");
            }
        }
    }

    #[test]
    fn free_text_queue_head_guard_fires_when_binary_consume_lacks_response() {
        let tmp = tempfile::TempDir::new().unwrap();
        let head = "monsterrodholders.md queue items that are completed lack exchange history";
        let with_head = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nUnrelated closeout.\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n- {head}\n<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &with_head);

        let without_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nUnrelated closeout.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n<!-- /agent:queue -->\n",
        );
        fs::write(&doc, without_head).unwrap();
        crate::snapshot::save(&doc, without_head).unwrap();
        crate::cycle_state::record_dropped_queue_prompts(&doc, &[head.to_string()]).unwrap();

        let rc = crate::graph::RunContext::new(doc.clone());
        rc.set_doc_content(without_head.to_string());
        match check_free_text_queue_head_provenance(&doc, &rc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("free-text"), "got: {message}");
                assert!(message.contains("response/echo"), "got: {message}");
            }
            GuardResult::Warn(lines) => {
                let message = lines.join("\n");
                assert!(message.contains("free-text"), "got: {message}");
                assert!(message.contains("response/echo"), "got: {message}");
            }
            GuardResult::None => {
                panic!("binary consume marker alone must not prove a free-text head")
            }
        }
    }

    #[test]
    fn free_text_queue_head_guard_passes_with_committed_response_echo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let head = "monsterrodholders.md queue items that are completed lack exchange history";
        let with_head = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nUnrelated closeout.\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n- {head}\n<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &with_head);

        let with_echo = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: queue closeout — gpt-5\n\n",
                "> **Queue prompt:**\n>\n> {head}\n\n",
                "The completed queue item now has durable exchange history.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        fs::write(&doc, &with_echo).unwrap();
        crate::snapshot::save(&doc, &with_echo).unwrap();

        let rc = crate::graph::RunContext::new(doc.clone());
        rc.set_doc_content(with_echo);
        assert!(
            matches!(
                check_free_text_queue_head_provenance(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "committed queue-prompt echo proves the consumed free-text head"
        );
    }

    #[test]
    fn free_text_queue_head_guard_passes_when_head_still_queued() {
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n- Can CellHandle::set also apply to the multi-threaded Context?\n<!-- /agent:queue -->\n",
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "head still queued = no fire"
        );
    }

    #[test]
    fn free_text_queue_head_guard_suppressed_by_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let head = "some question";
        let with_head = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n- {head}\n<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        let doc = init_committed_doc_for_queue_guard(tmp.path(), &with_head);

        let without_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n<!-- /agent:queue -->\n",
            "<!-- no-free-text-queue-head-guard -->\n",
        );
        fs::write(&doc, without_head).unwrap();
        crate::snapshot::save(&doc, without_head).unwrap();

        assert!(
            matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
            "marker suppresses guard"
        );
    }

    #[test]
    fn queue_contamination_guard_flags_response_prose_in_queue() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let prose = "Yes. I drove the already-authenticated Google Ads browser session with chromium-bridge to demote the campaign.";
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\n---\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: #gads106demote — gpt-5\n\n{prose}\n",
                "<!-- /agent:exchange -->\n",
                "\n<!-- agent:queue auto -->\n",
                "- do [#nbsearch]\n",
                "- {prose}\n",
                "<!-- /agent:queue -->\n",
            ),
            prose = prose
        );
        fs::write(&doc, content).unwrap();
        match check_queue_response_contamination_guard(&doc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("assistant response prose"), "{message}");
                assert!(message.contains("I drove"), "{message}");
            }
            other => panic!("expected contamination error, got {other:?}"),
        }
    }

    #[test]
    fn queue_contamination_guard_skips_user_prompt_mentioning_slash_command() {
        // #queue-contamination-guard-false-positive: a legit user queue prompt
        // that mentions slash commands must not be flagged as contamination
        // just because the response discussed the same commands (sharing a
        // verbatim 40-char run with the prompt).
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let user_prompt =
            "JB Run Agent Doc /clear opt-in should pre-emptively run /clear when the context threshold is exceeded";
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\n---\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: clear opt-in — gpt-5\n\n",
                "JB Run Agent Doc /clear opt-in should pre-emptively run /clear at the configured threshold; I wired the /agent-doc console path accordingly.\n",
                "<!-- /agent:exchange -->\n",
                "\n<!-- agent:queue auto -->\n",
                "- do [#nbsearch]\n",
                "- {user_prompt}\n",
                "<!-- /agent:queue -->\n",
            ),
            user_prompt = user_prompt
        );
        fs::write(&doc, content).unwrap();
        assert!(
            matches!(
                check_queue_response_contamination_guard(&doc).unwrap(),
                GuardResult::None
            ),
            "a user prompt mentioning /clear and /agent-doc must not be flagged as response contamination"
        );
    }

    #[test]
    fn queue_contamination_guard_still_flags_prose_without_slash_command() {
        // Guard rail for the slash-command skip: response prose copied into the
        // queue that does NOT reference a slash command is still flagged.
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let prose = "Yes. I drove the already-authenticated Google Ads browser session with chromium-bridge to demote the campaign.";
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\n---\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: #gads106demote — gpt-5\n\n{prose}\n",
                "<!-- /agent:exchange -->\n",
                "\n<!-- agent:queue auto -->\n",
                "- {prose}\n",
                "<!-- /agent:queue -->\n",
            ),
            prose = prose
        );
        fs::write(&doc, content).unwrap();
        assert!(
            matches!(
                check_queue_response_contamination_guard(&doc).unwrap(),
                GuardResult::Error(_)
            ),
            "response prose without a slash command must still be flagged as contamination"
        );
    }

    #[test]
    fn queue_contamination_guard_allows_directive_only_queue() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: #gads106demote — gpt-5\n\nYes. I drove the already-authenticated Google Ads session.\n",
            "<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue auto -->\n",
            "preset #spec-test\n- do [#nbsearch]\n- do [#bidstrat]\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, content).unwrap();
        assert!(matches!(
            check_queue_response_contamination_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }

    #[test]
    fn queue_contamination_guard_allows_free_text_prompt_not_from_response() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nAn unrelated answer about caching.\n",
            "<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue auto -->\n",
            "- check the deploy status on staging before release\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, content).unwrap();
        assert!(
            matches!(
                check_queue_response_contamination_guard(&doc).unwrap(),
                GuardResult::None
            ),
            "legitimate free-text queue prompt must not be flagged"
        );
    }

    /// `#jb-run-agent-doc-response-queue-contamination` blockquote-echo false
    /// positive: a still-live free-text queue HEAD whose verbatim text the
    /// answering `### Re:` response quotes in its `> **Queue prompt:**` echo must
    /// NOT be flagged as contamination. The response legitimately quotes the
    /// prompt it answered; the blockquote is a prompt-echo, not answer prose.
    #[test]
    fn queue_contamination_ignores_blockquoted_prompt_echo() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        let head = "The backlog has not been updating with the queue progress. Some queue items remain uncommitted over several runs.";
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\n---\n\n",
                "<!-- agent:exchange -->\n",
                "### Re: Backlog freshness — opus-4-8\n\n",
                "> **Queue prompt:**\n>\n> {head}\n\n",
                "Diagnosed the freshness symptom; steady-state reconcile is sound.\n",
                "<!-- /agent:exchange -->\n",
                "\n<!-- agent:queue auto -->\n",
                "- {head}\n",
                "<!-- /agent:queue -->\n",
            ),
            head = head
        );
        fs::write(&doc, content).unwrap();
        assert!(
            matches!(
                check_queue_response_contamination_guard(&doc).unwrap(),
                GuardResult::None
            ),
            "a live free-text head quoted in the answering response's blockquote echo must not be flagged"
        );
    }

    #[test]
    fn unresolved_exchange_prompt_detects_unanswered_tail_after_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier prompt\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "What are #next-steps to complete review items?\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("What are #next-steps to complete review items?")
        );
    }

    #[test]
    fn unresolved_exchange_prompt_none_when_answered_after_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "new prompt\n### Re: new prompt — gpt-5\n\nAnswered too.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(unresolved_exchange_prompt_in_content(content), None);
    }

    #[test]
    fn unresolved_exchange_prompt_none_when_tail_empty_after_boundary() {
        // Normal post-closeout shape: boundary at the very end, nothing after.
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ prompt\n### Re: prompt — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(unresolved_exchange_prompt_in_content(content), None);
    }

    #[test]
    fn unresolved_exchange_prompt_unmasked_by_queue_continuation_response() {
        // `#queue-continuation-buries-prompt`: a free-text user prompt followed
        // only by a queue-continuation response (`### Re: do [#id]`) is still
        // unresolved — that response answered the queue item, not the prompt.
        // This is the JB "agent-doc ignored my previous prompt" failure: a
        // concurrent queue continuation must not let the boundary bury it.
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:committed -->\n",
            "❯ JB Run Agent Doc on monsterrodholders.md stalled.\n",
            "### Re: do [#6cmx] — gpt-5\n\nI gated #6cmx.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("JB Run Agent Doc on monsterrodholders.md stalled."),
            "a free-text prompt followed only by a queue-continuation response must stay unresolved"
        );
    }

    #[test]
    fn is_queue_continuation_response_heading_distinguishes_directive_topics() {
        assert!(is_queue_continuation_response_heading("### Re: do [#6cmx]"));
        assert!(is_queue_continuation_response_heading(
            "#### Re: re [#374n] follow-up"
        ));
        // Free-text answer topics are NOT queue continuations.
        assert!(!is_queue_continuation_response_heading(
            "### Re: JB Run Agent Doc deadlock — opus-4-8"
        ));
        assert!(!is_queue_continuation_response_heading(
            "### Re: do this thing"
        ));
        assert!(!is_queue_continuation_response_heading("not a heading"));
    }

    #[test]
    fn unresolved_exchange_prompt_detects_fresh_prompt_without_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "do [#xyz]\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("do [#xyz]")
        );
    }

    #[test]
    fn session_check_snapshot_committed_guard_reports_side_effect_recovery_hint() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join("news/2026-05-01")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let news_index = root.join("news/README.md");
        let news_day = root.join("news/2026-05-01/README.md");
        fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n",
        )
        .unwrap();
        fs::write(&news_index, "old news index\n").unwrap();
        fs::write(&news_day, "old daily news\n").unwrap();
        crate::snapshot::save(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n",
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args([
                "add",
                "doc.md",
                "news/README.md",
                "news/2026-05-01/README.md",
            ])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // No cycle state, response heading present, snapshot ≠ HEAD, side
        // effects exist. Phase 3 (#jbccc3) only auto-recovers when the cycle
        // is at WriteApplied or Committed — without any cycle state, the
        // bypassed_response_write path still fires and must keep emitting the
        // side-effect recovery hint so the operator can diagnose the broken
        // closeout.
        let new_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: create today's news — codex\nresponse\n";
        fs::write(&doc, new_content).unwrap();
        crate::snapshot::save(&doc, new_content).unwrap();
        fs::write(&news_index, "new news index\n").unwrap();
        fs::write(&news_day, "new daily news\n").unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(msg.contains("tracked side-effect edits"));
                assert!(msg.contains("news/README.md"));
                assert!(msg.contains("news/2026-05-01/README.md"));
                assert!(msg.contains("agent-doc write --commit"));
            }
            SessionCheckStatus::Ok(msg) => {
                panic!("expected Interrupted, got Ok: {msg}");
            }
        }
    }

    #[test]
    fn session_check_snapshot_committed_guard_passes_when_committed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\nbody\n### Re: test\nresponse\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Ok(_) => {}
            SessionCheckStatus::Interrupted(msg) => {
                panic!("expected Ok, got Interrupted: {msg}");
            }
        }
    }

    #[test]
    fn session_check_active_session_drift_message_is_harness_agnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/captures")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/codex-hooks")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "old prompt\n",
            "### Re: old — gpt-5\n\n",
            "old response\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();

        let drifted = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done. Manual active-turn drift.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "old prompt\n",
            "### Re: old — gpt-5\n\n",
            "old response\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, drifted).unwrap();

        crate::codex_hook::record_external_prompt_for_file(&doc, "test-session", "new prompt")
            .unwrap();
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "test-session");

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(
                    !msg.contains("active Codex session"),
                    "error message should be harness-agnostic, not Codex-specific: {msg}"
                );
                assert!(
                    msg.contains("active harness session"),
                    "error message should say 'active harness session': {msg}"
                );
                assert!(
                    !msg.contains("let the Stop hook recover"),
                    "error message should not reference Stop hook exclusively: {msg}"
                );
                assert!(
                    msg.contains("let the hook recover"),
                    "error message should say 'let the hook recover': {msg}"
                );
            }
            other => panic!("expected Interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn uncommitted_exchange_drift_detected_without_codex_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ run the post docker commands\n",
            "### Re: docker deploy — glm-5.1\n\n",
            "Deploy completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ run the post docker commands\n",
            "### Re: docker deploy — glm-5.1\n\n",
            "Deploy completed.\n\n",
            "| File | Change |\n",
            "|------|--------|\n",
            "| mrh-performance.php | Reverted caching changes |\n",
            "| test script | Updated assertions |\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("uncommitted exchange changes")
                        || message.contains("direct response patchback"),
                    "message should mention uncommitted exchange drift or direct response patchback: {message}"
                );
                assert!(
                    message.contains("agent-doc finalize")
                        || message.contains("agent-doc write --commit")
                        || message.contains("agent-doc write --commit"),
                    "message should prescribe finalize or write --commit: {message}"
                );
            }
            other => panic!("expected interrupted status for #rspcmt6, got {other:?}"),
        }
    }

    #[test]
    fn uncommitted_exchange_drift_detects_prompt_plus_response_append() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ previous deploy\n",
            "### Re: previous deploy — gpt-5\n\n",
            "Previous deploy completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ previous deploy\n",
            "### Re: previous deploy — gpt-5\n\n",
            "Previous deploy completed.\n",
            "❯ do [#rspcmt7]. spec-test-build-install-commit-push\n",
            "### Re: SessionShare root closeout — gpt-5\n\n",
            "BuildParty demo deployed from commit `2336083`.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        let drift = detect_uncommitted_exchange_drift(&doc)
            .unwrap()
            .expect("prompt+response append should count as exchange drift");
        assert!(
            drift.contains("uncommitted working tree drift"),
            "unexpected drift detail: {drift}"
        );

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("uncommitted exchange changes")
                        || message.contains("uncommitted working tree drift")
                        || message.contains("direct response patchback"),
                    "message should mention uncommitted exchange drift: {message}"
                );
                assert!(
                    message.contains("agent-doc finalize")
                        || message.contains("agent-doc write --commit"),
                    "message should prescribe a closeout recovery command: {message}"
                );
            }
            other => panic!("expected interrupted status for prompt+response drift, got {other:?}"),
        }
    }

    #[test]
    fn uncommitted_exchange_drift_ignored_when_only_status_changed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Old status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "New status updated by user.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(_) => {}
            other => panic!("expected ok status for status-only drift, got {other:?}"),
        }
    }

    #[test]
    fn codex_final_gate_blocks_on_recursive_invocation_without_captured_response() {
        // #finalize-owned-pane-response-patchback: when a recursive direct
        // invocation was blocked (abandoned cycle) and no response body was
        // captured, codex_final_gate must exit 2 to prevent a final chat answer
        // from bypassing binary-owned closeout.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();

        // Recursive guard abandoned the cycle with no captured response.
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_abandoned(
            &doc,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Without final gate: session-check passes.
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(_) => {}
            other => panic!("expected ok for abandoned cycle without final gate, got {other:?}"),
        }

        // With final gate: must exit 2 (captured via child process since
        // run_with_options calls std::process::exit).
        // Resolve the `agent-doc` binary robustly. `src/` unit tests do not get
        // `CARGO_BIN_EXE_agent-doc`, so prefer an explicit `AGENT_DOC_TEST_BIN`,
        // then the workspace `target/debug/agent-doc` that `cargo nextest
        // --all-targets` builds (keeps CI coverage), then a bare PATH lookup.
        // CI runs `make check` without installing the binary to PATH, so the old
        // bare-`agent-doc` fallback spawned nothing and `.output().unwrap()`
        // panicked — failing this test (and the whole branch CI) even though the
        // codex-final-gate behavior was fine. Skip gracefully only when no
        // binary is spawnable at all.
        let bin = std::env::var("AGENT_DOC_TEST_BIN")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(|root| root.join("target/debug/agent-doc"))
                    .filter(|p| p.exists())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "agent-doc".to_string())
            });
        let output = match Command::new(&bin)
            .current_dir(root)
            .args(["session-check", "--codex-final-gate", doc.to_str().unwrap()])
            .env("AGENT_DOC_TEST_BIN", &bin)
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                eprintln!(
                    "[test] skipping codex_final_gate_blocks_on_recursive_invocation_without_captured_response: \
                     cannot spawn agent-doc binary `{bin}` ({err}); set AGENT_DOC_TEST_BIN to the built binary"
                );
                return;
            }
        };
        assert_eq!(
            output.status.code(),
            Some(2),
            "codex_final_gate should exit 2 for abandoned recursive invocation without captured response\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
