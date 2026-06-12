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
        // #rtwwire (rung 3): seed the guard-sweep cache from the realtime document
        // model (newest of disk vs the editor's unsaved buffer) so every guard
        // reasons about what the user actually sees, not a staler disk view. This
        // is what removes the "buffer differs from disk" false INTERRUPTED whack-a-
        // mole: a queue/exchange edit that lives only in the unsaved buffer is now
        // visible to the dropped-prompt / contamination guards instead of looking
        // dropped. Staleness-gated (`#rtwfeed`) — the buffer only wins when it
        // provably holds unsaved edits ahead of disk; no editor attached returns
        // disk unchanged.
        let disk = std::fs::read_to_string(file)?;
        rc.set_doc_content(crate::realtime_model::resolve_current_doc(file, &disk).content);
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
        .unwrap_or(trimmed)
        .trim();
    // #queue-user-edit-overwrite false positive: a consumed queue head is
    // struck in place (`~~text~~`, legacy `~text~`) and queue maintenance may
    // pin lines (`:round_pushpin:` / `:pushpin:`), so strip both before
    // identity matching — a struck or re-pinned line visibly reached the
    // document and was not silently lost.
    let unstruck = trimmed
        .strip_prefix("~~")
        .and_then(|s| s.strip_suffix("~~"))
        .or_else(|| trimmed.strip_prefix('~').and_then(|s| s.strip_suffix('~')))
        .unwrap_or(trimmed);
    let unpinned = crate::queue::strip_priority_markers(unstruck);
    normalized_prompt_for_match(&unpinned)
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
/// reaped now) is filtered out by [`reaped_directive_ids_without_response`], which
/// finds the `### Re: ... #id` heading in the live exchange or a HEAD compact archive.
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
    let archives: Vec<String> = head
        .as_deref()
        .map(|head| {
            crate::flow::closeout::compact_archive_pointers(head)
                .into_iter()
                .filter_map(|pointer| {
                    crate::flow::closeout::read_head_compact_archive(file, pointer)
                })
                .collect()
        })
        .unwrap_or_default();

    // Reaped `do #id` directive heads, deterministically ordered so the `bkx9`
    // diagnostic and the detector input are stable across runs.
    let mut ordered_ids: Vec<String> = directive_ids
        .into_iter()
        .filter(|id| reaped.contains(id))
        .collect();
    ordered_ids.sort();
    if ordered_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    // #bkx9wire: per-id response-loss diagnostic. Emitted even when a response was
    // captured this cycle, so a reproduced `#ipc-crdt-response-drift` (found=false)
    // is catchable from ops.log and a multi-id-under-one-heading cycle (found=true
    // for each id) proves no false positive — no live-verify needed.
    for id in &ordered_ids {
        let source = directive_response_source(&content, &archives, id);
        crate::ops_log::log_op(
            file,
            &format!(
                "bkx9 directive_response_materialized id={} found={} source={}",
                id,
                source.is_some(),
                source.map_or("none", ResponseSource::as_str),
            ),
        );
    }

    // Canonical lost set via the now-wired per-id detector (#z2jy bkx9-pure-detector).
    let lost = reaped_directive_ids_without_response(&ReapedResponseLossInput {
        directive_ids: &ordered_ids,
        reaped_ids: &ordered_ids,
        content: &content,
        archives: &archives,
    });

    // Guard ESCALATION stays scoped to reap-only / bookkeeping closeouts: when a
    // response was captured this cycle the diagnostic above still records any
    // captured-but-id-lost residual, but a false positive on the known multi-id
    // single-heading class must never wedge a committed closeout — so do not escalate.
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        return Ok(GuardResult::None);
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

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
/// materialized: the live exchange or a HEAD-referenced compact archive.
#[derive(Clone, Copy)]
enum ResponseSource {
    Exchange,
    Archive,
}

impl ResponseSource {
    fn as_str(self) -> &'static str {
        match self {
            ResponseSource::Exchange => "exchange",
            ResponseSource::Archive => "archive",
        }
    }
}

/// Resolve where `id`'s `### Re:` response heading materialized, if anywhere.
/// Pure over already-resolved `content` (the live committed exchange) and
/// `archives` (HEAD compact-archive bodies). Used by
/// [`check_reaped_queue_head_without_response`] for the `#bkx9`
/// `directive_response_materialized` diagnostic and to distinguish a legitimate
/// prior-cycle reap (response durably recorded, possibly archived) from a silent
/// loss.
fn directive_response_source(
    content: &str,
    archives: &[String],
    id: &str,
) -> Option<ResponseSource> {
    if content_has_re_heading_for_id(content, id) {
        return Some(ResponseSource::Exchange);
    }
    if archives
        .iter()
        .any(|archive| content_has_re_heading_for_id(archive, id))
    {
        return Some(ResponseSource::Archive);
    }
    None
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
// Wired into [`check_reaped_queue_head_without_response`] by `#bkx9wire`.
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
/// Wired into the live guard by `#bkx9wire`: the guard emits a per-id
/// `bkx9 directive_response_materialized` diagnostic (including on captured
/// cycles, surfacing the captured-but-id-lost residual in ops.log) but only
/// ESCALATES on reap-only / bookkeeping closeouts, because this guard runs at
/// every `write --commit` closeout and a false positive would wedge all
/// closeouts. The known false-positive class is pinned by the unit tests: a
/// single `### Re:` heading that answers `do #A` + `do #B` but names only `#A`
/// flags `#B` as lost.
///
/// See `specs/07-closeout-commands.md` `#compact-reap-no-response-record`.
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
    // Queue priority pins (`:round_pushpin:` / `:pushpin:` / 📌) are cosmetic
    // annotations on the directive, not part of it: `:round_pushpin: [#id]`
    // targets `#id` exactly like the unpinned spelling
    // (#queue-user-edit-overwrite consumed-head accounting).
    let unpinned = crate::queue::strip_priority_markers(normalized);
    let mut normalized = unpinned.as_str();
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
mod tests;
