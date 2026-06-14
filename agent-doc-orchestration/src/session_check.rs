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

mod response_guards;
pub(crate) use response_guards::*;
mod detect;
pub use detect::*;
mod partial_staging;
pub(crate) use partial_staging::*;

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
pub(crate) enum GuardResult {
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
                // #goqueuestall: when continuation is not required because every
                // remaining queue head is undrainable in the current session type
                // (`[clean-session]` under live IPC, or `[operator-verify]`),
                // surface a one-line deferred-heads note so the idle queue reads
                // as deferred work, not a silent stall.
                let deferred = crate::queue_continuation::deferred_head_count(file);
                // #goqstall2: pre-materialized bulleted free-text lines that are not
                // actionable drain targets (pasted bug-report observations / stale
                // console evidence) are inert noise — counted so the idle queue reads
                // as "deferred + N stale lines to clear", not a silent stall. Never
                // auto-deleted (the live IPC supervisor races on direct queue edits).
                let noise = crate::queue_continuation::queue_stale_noise_lines(file);
                if deferred > 0 || noise > 0 {
                    println!(
                        "queue_continuation_required=false queue_deferred_heads={} queue_stale_noise_lines={}",
                        deferred, noise
                    );
                    eprintln!(
                        "[session-check] queue idle: {} head(s) deferred (clean-session/operator-verify), {} stale noise line(s) — drain deferred heads from a clean session and clear the noise lines ({}; #goqueuestall/#goqstall2).",
                        deferred,
                        noise,
                        file.display()
                    );
                } else {
                    println!("queue_continuation_required=false");
                }
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
    // #fccsupwarn: surface a stale hosting controller/supervisor at closeout too, so a
    // drifted post-commit cycle points the operator at a recycle instead of re-filing a
    // File-Cache-Conflict dialog. Fail-open — any status/stat error yields no warning.
    if let Some(message) = crate::project_controller::stale_supervisor_warning_for_doc(file) {
        report
            .warnings
            .push(format!("[session-check] WARNING: {message}"));
    }
    Ok(report)
}


/// `#nochange-after-stall-breadth`: a no-response repair/reap-only closeout
/// must not make an active queue head look complete. The missing-response guard
/// intentionally skips no-op bookkeeping commits to avoid deadlocking ordinary
/// `--done` repairs, but when the same cycle recorded a runnable `agent:queue`
/// head and that head is still both queued and open in `agent:backlog`, the
/// turn made no durable progress on executable work. Fail closed so the next
/// actor runs the head instead of reporting a plain no-change/clean closeout.
mod queue_head_guards;
pub(crate) use queue_head_guards::*;

mod backlog_guards;
pub use backlog_guards::*;
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

mod pending_guards;
pub use pending_guards::*;

mod done_signals;
pub use done_signals::*;
/// `#do-id-closeout-open-backlog`: a resolved `do [#id]` directive must end with
/// an explicit lifecycle outcome for its target id. If the cycle committed a
/// response (queue cleared, status updated) but the target id is still open in
/// `agent:backlog` and was not recorded as done / kept-open / reaped this cycle,
/// fail closed so the directive cannot silently leave its target `[ ]`.
mod queue_head_provenance_guards;
pub(crate) use queue_head_provenance_guards::*;
/// (`--pending-edit <id>=...`), adding a new follow-up item (`--pending-add*`),
/// or an explicit no-follow-up justification phrase in the response. A
/// `<!-- no-blocked-followup-guard -->` marker also suppresses it.
/// `#blocked-closeout-followup-capture`: a directed `do [#id]` cycle that moves
/// its target to the review/gated component (`--pending-gate`) while the
/// response says the work is blocked / still needs future action must capture an
/// actionable follow-up before clean closeout — otherwise the document explains
/// the blocker but the active backlog no longer drives the remaining work.
///
/// Satisfied by any of: keeping the id open in `agent:backlog`
mod closeout_guards;
pub use closeout_guards::*;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
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
        crate::capture::capture_response(&doc, "### Re: do #nsga4verify — gpt-5\n\nDone.").unwrap();
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
    let mut current = String::from("---\nagent_doc_session: test\n---\n\n## Exchange\n\nHello\n");
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
// `#gated-followup-split-enforcement`: kept-open parent item whose body
// enumerates multiple gated phases without discrete child ids.
const MULTI_PHASE_REVIEW: &str = "- [/] [#parentfix] [recommended] Follow-ups from the parent fix. Phase 1 landed. Remaining (gated — needs a live pane): (2b) a live Stop-hook regression asserting in-pane output; (3) live-verify a real same-pane run. Plan: tasks/x.md\n";
const SPLIT_RESPONSE: &str = "### Re: do #parentfix — gpt-5\n\nLanded phase 1 and kept the remaining phases noted on the item.\n";
// `#queue-audit-partial-completion`: a queue-completion audit that collapses
// partial progress into "none complete."
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
        .unwrap();
    doc
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
#[test]
fn committed_without_response_body_guard_fires_on_pending_mutations_no_capture() {
    let dir = tempfile::tempdir().unwrap();
    let doc = write_committed_turn_doc(dir.path(), false, true, &[]);
    match check_committed_without_response_body_guard(&doc).unwrap() {
        GuardResult::Error(msg) => {
            assert!(
                msg.contains("no assistant `### Re:` response body is present in `agent:exchange`"),
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
                msg.contains("no assistant `### Re:` response body is present in `agent:exchange`"),
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
fn committed_without_response_body_guard_skips_equityfundingsource_noop_queue_recovery() {
    // #eqrecovery: the equityfundingsource reentrant recovery cycle had already
    // converged the drained queue/backlog state. A later no-op closeout still
    // carried queue-turn evidence, but `commit_already_current` means no new
    // binary-owned content was committed without a response body.
    let _lock = crate::test_support::env_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    let doc = root.join("doc.md");
    let current = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\n",
        "Compacted.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Queue\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog priority queue -->\n",
        "<!-- /agent:backlog -->\n",
    )
    .to_string();
    fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, &current).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
    crate::cycle_state::mark_pending_mutations(&doc).unwrap();
    crate::cycle_state::record_active_queue_heads(&doc, &["do [#eqrecovery]".to_string()]).unwrap();
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
    assert!(
        matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
        "drained no-op queue recovery must not re-interrupt a committed cycle"
    );
    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("committed_without_response_body_guard_skipped_noop_commit"),
        "{ops_log}"
    );
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
fn no_response_active_queue_head_skips_operator_verify_deferred_head() {
    // #goqueuestall: a reap-only/no-response closeout whose only remaining
    // runnable head is `[operator-verify]` (never agent-drainable) is NOT a
    // silent-stall — the head is deferred by design, so the guard must stay quiet
    // instead of perpetually re-INTERRUPTing an undrainable queue.
    let tmp = tempfile::TempDir::new().unwrap();
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: prior — gpt-5\n\nAnswered.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#qflood2] [operator-verify] live drive only\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#qflood2]\n",
        "<!-- /agent:queue -->\n",
    );
    let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
    crate::cycle_state::record_reaped_pending_ids(&doc, &["alreadydone".to_string()]).unwrap();

    assert!(
        matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
        "an operator-verify deferred head must not trip the no-response active-head guard"
    );
}
#[test]
fn no_response_active_queue_head_interrupts_on_clean_session_head_regardless_of_ipc() {
    // #qcontdrain: a `[clean-session]` head is now ALWAYS drainable in-loop, so a
    // committed-without-response active queue head on it must INTERRUPT whether or
    // not a live editor-IPC listener is running — live IPC no longer defers it.
    let tmp = tempfile::TempDir::new().unwrap();
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: prior — gpt-5\n\nAnswered.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#cleanhead] [clean-session] needs a quiet session\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#cleanhead]\n",
        "<!-- /agent:queue -->\n",
    );
    let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
    crate::cycle_state::record_reaped_pending_ids(&doc, &["alreadydone".to_string()]).unwrap();

    // Without a live listener the clean-session head is drainable → INTERRUPT.
    match inspect(&doc).unwrap() {
        SessionCheckStatus::Interrupted(message) => {
            assert!(message.contains("#cleanhead"), "{message}");
            assert!(
                message.contains("committed without an assistant response body"),
                "{message}"
            );
        }
        other => panic!("expected interrupt for drainable clean-session head, got {other:?}"),
    }

    // Start a live editor-IPC listener for the project root. The clean-session
    // head stays drainable (#qcontdrain), so the guard must STILL interrupt.
    let root = tmp.path().to_path_buf();
    let root_clone = root.clone();
    let server = std::thread::spawn(move || {
        crate::ipc_socket::start_listener(&root_clone, |_msg| {
            Some(serde_json::json!({"type": "ack"}).to_string())
        })
        .ok();
    });
    // Wait until the listener is provably active before asserting.
    for _ in 0..50 {
        if crate::ipc_socket::is_listener_active(&root) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        crate::ipc_socket::is_listener_active(&root),
        "listener should be active for the project root"
    );
    match inspect(&doc).unwrap() {
        SessionCheckStatus::Interrupted(message) => {
            assert!(message.contains("#cleanhead"), "{message}");
        }
        other => panic!(
            "clean-session head must still interrupt under live IPC (#qcontdrain), got {other:?}"
        ),
    }

    let _ = std::fs::remove_file(crate::ipc_socket::socket_path(&root));
    drop(server);
}
#[test]
fn no_response_active_queue_head_still_fires_on_drainable_head() {
    // #goqueuestall regression guard: a genuinely drainable (plain, untagged)
    // head with no response must STILL INTERRUPT — the deferral exclusion must not
    // swallow real unanswered queue work.
    let tmp = tempfile::TempDir::new().unwrap();
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: prior — gpt-5\n\nAnswered.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#splitmodswrite] plain drainable head\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#splitmodswrite]\n",
        "<!-- /agent:queue -->\n",
    );
    let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
    crate::cycle_state::record_reaped_pending_ids(&doc, &["alreadydone".to_string()]).unwrap();

    match inspect(&doc).unwrap() {
        SessionCheckStatus::Interrupted(message) => {
            assert!(message.contains("#splitmodswrite"), "{message}");
            assert!(
                message.contains("committed without an assistant response body"),
                "{message}"
            );
        }
        other => panic!("expected interrupt for drainable head, got {other:?}"),
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
    crate::cycle_state::record_active_queue_heads(&doc, &["do [#lostresp]".to_string()]).unwrap();
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
    crate::cycle_state::record_active_queue_heads(&doc, &["do [#answered]".to_string()]).unwrap();
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
#[test]
fn pure_detector_flags_reap_only_loss() {
    // The reap-only silent-loss shape: the id was reaped this cycle but no
    // `### Re: ... #id` heading exists anywhere — flag it.
    let directive = vec!["lostresp".to_string()];
    let reaped = vec!["lostresp".to_string()];
    let content = "### Re: prior — gpt-5\n\nAnswered something else.\n";
    let archives: Vec<String> = Vec::new();
    assert_eq!(
        reaped_directive_ids_without_response(&loss_input(&directive, &reaped, content, &archives)),
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
        reaped_directive_ids_without_response(&loss_input(&directive, &reaped, content, &archives)),
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
        reaped_directive_ids_without_response(&loss_input(&directive, &reaped, content, &archives))
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
        reaped_directive_ids_without_response(&loss_input(&directive, &reaped, content, &archives))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(snapshot), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(historical), Some(historical))
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
#[test]
fn queue_audit_guard_warns_when_none_complete_collapses_partial_progress() {
    let tmp = tempfile::TempDir::new().unwrap();
    let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the six queue items are complete. Same-day QA is complete and the URL validate-only check was clean, but each row still has at least one remaining action.\n";
    let doc = setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
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
    let doc = setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
    assert!(matches!(
        check_queue_audit_partial_completion_guard(&doc).unwrap(),
        GuardResult::None
    ));
}
#[test]
fn queue_audit_guard_quiet_when_not_about_queue() {
    let tmp = tempfile::TempDir::new().unwrap();
    let response = "### Re: status — gpt-5\n\nNone of the migration steps are complete. The schema dump is complete and the backup was clean.\n";
    let doc = setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
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
    let doc = setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
    assert!(matches!(
        check_queue_audit_partial_completion_guard(&doc).unwrap(),
        GuardResult::None
    ));
}
#[test]
fn queue_audit_guard_suppressed_by_marker() {
    let tmp = tempfile::TempDir::new().unwrap();
    let response = "### Re: which queue items are complete? — gpt-5\n\nNone of the six queue items are complete. Same-day QA is complete and the check was clean. <!-- no-queue-audit-guard -->\n";
    let doc = setup_blocked_closeout_cycle(tmp.path(), response, None, None, &[], &[], &[], false);
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
fn do_directive_target_ids_strips_queue_priority_pins() {
    // Queue maintenance pins lines with `:round_pushpin:` / `:pushpin:`;
    // the pinned spelling targets the same id as the unpinned one
    // (#queue-user-edit-overwrite consumed-head accounting).
    let prompts = vec![
        ":round_pushpin: [#pinned]".to_string(),
        "- :pushpin: do [#opin]".to_string(),
        "📌 #emoji proceed".to_string(),
    ];
    let ids = do_directive_target_ids(&prompts);
    assert_eq!(ids, vec!["pinned", "opin", "emoji"]);
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
    let snapshot_content =
        "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n";
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
    let new_content =
        "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n";
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
#[test]
fn session_check_clears_dropped_queue_marker_when_head_struck_in_place() {
    let tmp = tempfile::TempDir::new().unwrap();
    // The pinned head was consumed and struck in place (`~~...~~`), and the
    // pending id was reaped this cycle — the user edit visibly reached the
    // document, so neither the strike nor the pin is a silent deletion
    // (#queue-user-edit-overwrite false positive on a consumed head).
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange -->\n### Re: do [#gscaccess] — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
        "\n<!-- agent:queue auto -->\n- ~~:round_pushpin: [#gscaccess]~~\n- do [#existing]\n<!-- /agent:queue -->\n",
    );
    let doc = init_committed_doc_for_queue_guard(tmp.path(), committed);
    crate::cycle_state::record_dropped_queue_prompts(
        &doc,
        &[":round_pushpin: [#gscaccess]".to_string()],
    )
    .unwrap();

    assert!(
        matches!(inspect(&doc).unwrap(), SessionCheckStatus::Ok(_)),
        "guard should clear when the dropped queue head is struck in place"
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
    crate::cycle_state::record_pending_done_ids(&doc, &["shipstationaudit".to_string()]).unwrap();

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
    let user_prompt = "JB Run Agent Doc /clear opt-in should pre-emptively run /clear when the context threshold is exceeded";
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

    crate::codex_hook::record_external_prompt_for_file(&doc, "test-session", "new prompt").unwrap();
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
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

