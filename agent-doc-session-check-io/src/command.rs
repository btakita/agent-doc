//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` inspects the state-backbone closeout projection first, then the
//!   persisted per-document cycle-state compatibility JSON in
//!   `.agent-doc/state/cycles/<hash>.json`, and exits nonzero when the most
//!   recent cycle is still open (`preflight_started`, `response_captured`, or
//!   `write_applied`).
//! - Falls back to the JSON sidecar and then the last `ops.log` event when no
//!   closeout projection exists yet, preserving compatibility for older repos.
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

use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_turn::CyclePhase;
use agent_doc_turn::op_log::{
    PREFLIGHT_START_EVENT, event_name, is_write_completed_commit_missing_event,
};
use agent_doc_workflow::session_check::{BlockedCloseoutMessage, GuardResult};
use anyhow::Result;
use std::path::Path;

use crate::{
    detect_jb_cache_conflict_accept_duplicate_replay,
    detect_jb_cache_conflict_cancel_recoverable_with_context,
    detect_late_ipc_response_overapplication, detect_unstarted_prompt_bearing_diff,
    operator_live_buffer_contains_heading,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

pub struct SessionCheckReport {
    pub status: SessionCheckStatus,
    pub warnings: Vec<String>,
}

pub trait SessionCheckEffects {
    fn closeout_recovery_hint(&self, file: &Path) -> String;
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &Path,
    ) -> Result<Option<&'static str>>;
    fn recover_missing_commit_boundary(
        &self,
        file: &Path,
        event: &str,
    ) -> Result<Option<&'static str>>;
}

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path, effects: &impl SessionCheckEffects) -> Result<()> {
    run_with_options(file, false, effects)
}

/// `#qpausemix`: the queue-continuation guidance `session-check` prints when
/// `queue_continuation_required == true`, resolved against the controller pause
/// state. Reads the effective `admin queue pause` reason and composes the
/// pause-aware [`agent_doc_queue::queue_continuation::continuation_guidance`] — the SAME
/// single source consumed by the preflight `queue_continuation_guidance` field.
///
/// Printing the bare `CONTINUATION_NO_STALL_GUIDANCE` constant here (the prior
/// behavior) let the mixed-signal resolution reach preflight JSON but not
/// session-check stdout, so an agent reading session-check saw `queue_paused:
/// true` next to `queue_continuation_required: true` with no explanation and
/// stalled deciding whether the pause was operator intent or transient
/// drain-coordination state. The guidance now carries the "queue_paused is NOT a
/// contradiction" preamble and recorded reason whenever the queue is paused.
fn continuation_guidance_for(file: &Path) -> String {
    let pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    agent_doc_queue::queue_continuation::continuation_guidance(pause_reason.as_deref())
}

fn log_supervisor_drain_handoff(file: &Path, head: &str, outcome_fields: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_supervisor_drain_handoff file={} head_bytes={} head_sha256={} {}",
            file.display(),
            head.len(),
            agent_doc_hash::content_hash(head),
            outcome_fields
        ),
    );
}

/// `session-check` with the optional Codex final-gate.
///
/// Default (`codex_final_gate = false`): keeps exit 0 for a clean document and
/// prints `queue_continuation_required=...` as an informational typed detail.
/// Strict (`codex_final_gate = true`): exits `2` when a clean document still
/// owes an active `agent:queue auto` continuation, so Codex direct-exec closeout
/// paths cannot send a final answer past a stalled queue.
/// (#codex-auto-queue-stalled-final-gate)
pub fn run_with_options(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    self_heal_late_ipc_overapplication(file, effects)?;
    // Phase E rung 2 (`#adstatechart2`): advisory read-only observability of the
    // local-process four-region state, logged alongside the existing ops.log
    // markers. Never gates closeout — emitted regardless of the check outcome.
    agent_doc_state_observer_io::log_advisory_snapshot(file);
    let report = inspect_with_warnings(file, effects)?;
    for warning in &report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(message) => {
            println!("{}", message);
            // `#wd40` / `#staleloop-recycle-restart`: a stale route-owned supervisor
            // that can never reach its own recycle boundary during a continuously
            // self-draining session asks the in-session loop to yield one boundary.
            // Surface that as a distinct, intentional yield (NOT a drained queue or
            // a stall) so the loop ends its turn cleanly; the idle boundary lets the
            // `execve` recycle fire and the drain resumes on the fresh binary. Never
            // force the Codex final-gate here — yielding is the desired outcome.
            if agent_doc_controller_io::project_controller::supervisor_recycle_yield_pending_for_file(file) {
                let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                    agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
                )
                .expect("static no-drainable-work outcome is valid")
                .log_fields();
                println!(
                    "queue_continuation_required=false queue_recycle_yield=true {outcome_fields}"
                );
                eprintln!(
                    "[session-check] {}",
                    agent_doc_queue::queue_continuation::RECYCLE_YIELD_GUIDANCE
                );
                return Ok(());
            }
            if let Some(continuation) = agent_doc_queue_io::queue_continuation::detect(file)? {
                // #prompt-preempts-auto-queue: a live unresolved exchange prompt
                // must run before queue continuation, even when it was already
                // baselined into the snapshot. Defer the queue (do not force the
                // Codex final-gate) while such a prompt exists so the next cycle
                // answers it instead of skipping to the queue head.
                if let Some(unresolved) = crate::unresolved_exchange_prompt(file)? {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForOperatorProof,
                    )
                    .expect("static deferred operator-proof outcome is valid")
                    .log_fields();
                    println!(
                        "queue_continuation_required=false queue_deferred_for_unresolved_exchange_prompt={:?} next_queue_prompt={:?} {}",
                        unresolved, continuation.head_prompt, outcome_fields
                    );
                    eprintln!(
                        "[session-check] queue continuation deferred for {}: unresolved exchange prompt {:?} must run before the queue head {:?} (#prompt-preempts-auto-queue). {}",
                        file.display(),
                        unresolved,
                        continuation.head_prompt,
                        outcome_fields
                    );
                    return Ok(());
                }
                if let Some(command) =
                    agent_doc_queue::queue_command::slash_command_text(&continuation.head_prompt)
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
                // #degraded-ipc-no-stall: binary-authoritative "keep draining"
                // guidance so the loop is not stalled on a degraded transport /
                // stale supervisor / accretion / semantic-completion warning.
                //
                // #qpausemix: emit the pause-aware guidance, NOT the bare
                // `CONTINUATION_NO_STALL_GUIDANCE` constant, so a controller-paused
                // queue (`queue_paused: true` alongside this
                // `queue_continuation_required: true`) prints the "queue_paused is
                // NOT a contradiction" preamble + recorded pause reason here too.
                eprintln!("[session-check] {}", continuation_guidance_for(file));
                // #qpausemix-verify / #j9ja: the pause-aware guidance above only
                // reaches session-check stderr. When the queue is controller-paused,
                // also drop a distinctive SUCCESS marker into ops.log so a live
                // operator test of the "queue_paused is NOT a contradiction" preamble
                // is provable/disprovable from the log (auto-verify resolves the gate
                // via `--pending-set-verify
                // verify=ops_log:queue_paused_continuation_guidance_emitted`).
                if let Some(reason) =
                    agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(
                        file,
                    )
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_paused_continuation_guidance_emitted pause_reason={reason:?} #qpausemix-verify"
                        ),
                    );
                }
                // #qstallguard Layer B: this is a clean closeout that STILL requires
                // continuation. Record a one-shot continuation-pending projection so
                // the next preflight can emit `queue_stall_detected` if the loop neither
                // continued nor recorded a valid stop reason (the prose no-stall
                // guidance is advisory; this makes the violation a hard signal).
                let stall_cycle_id = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                    .ok()
                    .flatten()
                    .map(|s| s.cycle_id)
                    .unwrap_or_default();
                if let Err(err) = agent_doc_controller_io::project_controller::record_queue_drain_stall_continuation_pending_for_file(
                    file,
                    &stall_cycle_id,
                ) {
                    eprintln!(
                        "[session-check] warning: failed to record continuation projection: {err}"
                    );
                }
                if codex_final_gate {
                    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(
                        &continuation.head_prompt,
                    ) {
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
                let content =
                    crate::resolve_current_document_content(file, "deferred_queue_head_count").ok();
                let deferred = content
                    .as_deref()
                    .map(agent_doc_queue::queue_continuation::deferred_head_count)
                    .unwrap_or(0);
                // #goqstall2/#freshqueueauth: pre-materialized queue lines that
                // match the exact non-drainable noise predicate (pasted console
                // evidence / agent fragments, never ordinary prose reports) are
                // counted so the idle queue reads as "deferred + N predicate-proven
                // lines to clear", not a silent stall. Fresh drainable operator
                // prompts remain authoritative and are never classified through
                // this path.
                let noise = content
                    .as_deref()
                    .map(agent_doc_queue::queue_continuation::queue_stale_noise_lines)
                    .unwrap_or(0);
                // #qfocsup: the in-session loop has no drainable head, but a
                // `[focused-cycle]` head may still remain that the SUPERVISOR
                // clear-and-continue path will drain. In this branch the in-session
                // `detect` already returned None, so a `Some` supervisor head ⟺ a
                // focused-cycle head — the queue is NOT operator-stalled; the agent
                // yields and the supervisor force-`/clear`s + re-dispatches it.
                let supervisor_head = content.as_deref().and_then(|content| {
                    agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                        content,
                        agent_doc_queue::queue_continuation::DrainScope::Supervisor,
                    )
                });
                if let Some(supervisor_head) = supervisor_head {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForSupervisorDrain,
                    )
                    .expect("static deferred supervisor-drain outcome is valid")
                    .log_fields();
                    log_supervisor_drain_handoff(file, &supervisor_head, &outcome_fields);
                    println!(
                        "queue_continuation_required=false queue_deferred_heads={} queue_stale_noise_lines={} {}",
                        deferred, noise, outcome_fields
                    );
                    eprintln!(
                        "[session-check] queue continues via supervisor: a [focused-cycle] head remains that the CPC/supervisor clear-and-continue path drains (force /clear + re-dispatch to a fresh session). End this turn so the supervisor takes over — NOT an operator stall ({}; #qfocsup). {}",
                        file.display(),
                        outcome_fields
                    );
                } else if deferred > 0 || noise > 0 {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForOperatorProof,
                    )
                    .expect("static deferred operator-proof outcome is valid")
                    .log_fields();
                    println!(
                        "queue_continuation_required=false queue_deferred_heads={} queue_stale_noise_lines={} {}",
                        deferred, noise, outcome_fields
                    );
                    eprintln!(
                        "[session-check] queue idle: {} head(s) deferred (operator-verify), {} predicate-proven noise line(s) — operator-gated heads need a human / predicate-proven noise can be cleared with `agent-doc queue prune-noise` ({}; #goqueuestall/#goqstall2/#freshqueueauth). {}",
                        deferred,
                        noise,
                        file.display(),
                        outcome_fields
                    );
                } else {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
                    )
                    .expect("static no-drainable-work outcome is valid")
                    .log_fields();
                    println!("queue_continuation_required=false {outcome_fields}");
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
                && let Some(cycle) = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                    .ok()
                    .flatten()
                && matches!(cycle.phase, agent_doc_turn::CyclePhase::Abandoned)
                && cycle
                    .last_event
                    .contains("recursive_direct_invocation_blocked")
                && cycle.capture_id.is_none()
                && cycle.response_sha256.is_none()
            {
                let has_visible_response = crate::unresolved_exchange_prompt(file)?.is_none()
                    && crate::exchange_tail_has_response_heading(file)?;
                if has_visible_response {
                    eprintln!(
                        "[session-check] codex-final-gate: recursive direct invocation was blocked for {} but the response is already visible in agent:exchange — adopting the manual patchback idempotently.",
                        file.display()
                    );
                    agent_doc_ops_log_io::log_op(
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

pub fn inspect(file: &Path, effects: &impl SessionCheckEffects) -> Result<SessionCheckStatus> {
    Ok(inspect_with_warnings(file, effects)?.status)
}

pub fn inspect_with_warnings(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<SessionCheckReport> {
    let mut report = SessionCheckReport {
        status: inspect_core(file, effects)?,
        warnings: Vec::new(),
    };
    if matches!(report.status, SessionCheckStatus::Ok(_)) {
        // Build one CycleContext for the guard sweep and seed it with the resolved
        // CurrentDocument. Guards that need content, frontmatter, or components
        // read from that lazily graph instead of independently resolving and
        // parsing the current document.
        let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
        // #rtwwire (rung 3): seed the guard-sweep cache from the authoritative
        // current document. Active editors resolve through the CRDT relay; disk
        // is consulted only when no editor is attached.
        rc.set_current_document(
            agent_doc_document_realtime_io::try_resolve_current_document(file)?,
        );
        match crate::check_dropped_exchange_prompt_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_dropped_queue_prompt_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_queue_response_contamination_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        if let Some(message) = crate::check_completed_pending_reap_guard(file, &rc)? {
            report.status = SessionCheckStatus::Interrupted(message);
            return Ok(report);
        }
        match crate::check_shadow_backlog_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_malformed_tracked_item_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_backlog_replay_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_snapshot_committed_guard(file, &rc, |file| {
            effects.closeout_recovery_hint(file)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_parent_submodule_pointer_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_committed_without_response_body_guard(file, |file| {
            effects.closeout_recovery_hint(file)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_no_response_active_queue_head(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_reaped_queue_head_without_response(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_prompt_only_exchange_tail_guard(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        for guard in [
            crate::check_pending_capture_guard(file, &rc)?,
            crate::check_pending_done_guard(file, &rc)?,
            crate::check_expect_done_or_gate_guard(file, &rc)?,
            crate::check_partial_closeout_state_guard(file)?,
            crate::check_partial_staging_closeout_guard(file)?,
            crate::check_blocked_closeout_followup_guard(file, &rc)?,
            crate::check_gated_phase_split_guard(file, &rc)?,
            crate::check_queue_audit_partial_completion_guard(file)?,
            crate::check_queue_head_removal_guard(file, &rc)?,
            crate::check_free_text_queue_head_provenance(file, &rc)?,
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
        if let Ok(Some(miss)) = agent_doc_supervisor_io::startup_miss::load_startup_miss(file) {
            if let Some(supersession) =
                agent_doc_supervisor_io::startup_miss::superseded_by_newer_registered_start(
                    agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
                    file,
                    &miss,
                )?
            {
                agent_doc_supervisor_io::startup_miss::clear_startup_miss(file)?;
                agent_doc_ops_log_io::log_op(
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
                let detail = agent_doc_supervisor_io::startup_miss::session_log_diagnostic(
                    file,
                    &miss.session_id,
                )
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
    if let Some(message) =
        agent_doc_controller_io::project_controller::stale_supervisor_warning_for_doc(file)
    {
        report
            .warnings
            .push(format!("[session-check] WARNING: {message}"));
    }
    Ok(report)
}

///
/// Kept out of the read-only `inspect*` path on purpose: only the mutating
/// command entrypoints (`enforce_clean_closeout` on the finalize boundary,
/// `run_with_options` for direct-exec `agent-doc session-check`) repair in place.
fn self_heal_late_ipc_overapplication(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<bool> {
    let Some(overapplication) = detect_late_ipc_response_overapplication(file)? else {
        return Ok(false);
    };
    agent_doc_ops_log_io::log_op(
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
    effects.atomic_write(file, &overapplication.remediated_content)?;
    agent_doc_snapshot_io::save(
        file,
        &overapplication.remediated_content,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(true)
}

pub fn enforce_clean_closeout(file: &Path, effects: &impl SessionCheckEffects) -> Result<()> {
    self_heal_late_ipc_overapplication(file, effects)?;
    let report = inspect_with_warnings(file, effects)?;
    for warning in report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(_) => Ok(()),
        SessionCheckStatus::Interrupted(message) => anyhow::bail!(message),
    }
}

pub fn detect_uncommitted_closeout_drift(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<Option<String>> {
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    detect_uncommitted_closeout_drift_with_context(file, &rc, effects)
}

pub fn detect_uncommitted_closeout_drift_with_context(
    file: &Path,
    rc: &agent_doc_run_context_io::CycleContext,
    effects: &impl SessionCheckEffects,
) -> Result<Option<String>> {
    if effects
        .repair_committed_historical_snapshot_drift(file)?
        .is_some()
    {
        return Ok(None);
    }
    if let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? {
        return Ok(Some(agent_doc_git::parent_submodule_pointer_message(
            &drift.relative_path,
            drift.parent_head.as_deref(),
            &drift.submodule_head,
            &file.display().to_string(),
        )));
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
    if let Some(marker) = crate::detect_bypassed_response_write(file)? {
        return Ok(Some(format!(
            "found likely direct response patchback without agent-doc cycle: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            effects.closeout_recovery_hint(file)
        )));
    }
    if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
        if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
            return Ok(None);
        }
        return Ok(Some(format!(
            "document has uncommitted exchange changes beyond the committed snapshot: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            effects.closeout_recovery_hint(file)
        )));
    }
    match rc.snapshot_commit_status() {
        agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead {
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
                agent_doc_git_io::status::tracked_side_effect_note(file)?,
                effects.closeout_recovery_hint(file)
            )))
        }
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoSnapshot
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoHead
        | agent_doc_snapshot_io::SnapshotCommitStatus::NotInGitRepo => Ok(None),
    }
}

fn closeout_projection_event_label(
    projection: &agent_doc_cycle_state_io::ProjectedCloseoutState,
    phase: CyclePhase,
) -> String {
    match phase {
        CyclePhase::PreflightStarted => "state_backbone_preflight_started".to_string(),
        CyclePhase::ResponseCaptured => projection
            .capture_id
            .as_deref()
            .map(|capture_id| format!("state_backbone_response_captured capture_id={capture_id}"))
            .unwrap_or_else(|| "state_backbone_response_captured".to_string()),
        CyclePhase::WriteApplied => projection
            .patch_id
            .as_deref()
            .map(|patch_id| format!("state_backbone_write_applied patch_id={patch_id}"))
            .unwrap_or_else(|| "state_backbone_write_applied".to_string()),
        CyclePhase::Committed => projection
            .commit
            .as_deref()
            .map(|commit| format!("state_backbone_commit_observed commit={commit}"))
            .unwrap_or_else(|| "state_backbone_commit_observed".to_string()),
        CyclePhase::Abandoned => projection
            .abandoned_reason
            .as_deref()
            .map(|reason| format!("state_backbone_cycle_abandoned reason={reason}"))
            .unwrap_or_else(|| "state_backbone_cycle_abandoned".to_string()),
    }
}

fn apply_closeout_projection_to_cycle_state(
    file: &Path,
    state: &mut agent_doc_cycle_state_io::CycleState,
    projection: Option<&agent_doc_cycle_state_io::ProjectedCloseoutState>,
) {
    let Some(projection) = projection else {
        return;
    };
    let Some(projected_phase) = projection.phase else {
        return;
    };
    if state.phase != projected_phase {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "session_check_closeout_projection_preferred file={} cycle_id={} json_phase={} projected_phase={}",
                file.display(),
                state.cycle_id,
                state.phase.as_str(),
                projected_phase.as_str()
            ),
        );
    }
    state.phase = projected_phase;
    state.last_event = closeout_projection_event_label(projection, projected_phase);
    if state.capture_id.is_none() {
        state.capture_id = projection.capture_id.clone();
    }
    if state.response_sha256.is_none() {
        state.response_sha256 = projection.response_sha256.clone();
    }
}

fn projected_open_closeout_message(
    file: &Path,
    projection: &agent_doc_cycle_state_io::ProjectedCloseoutState,
    phase: CyclePhase,
) -> String {
    let cycle_id = projection.cycle_id.as_deref().unwrap_or("unknown");
    let detail = match phase {
        CyclePhase::PreflightStarted => "cycle started but no write/commit followed",
        CyclePhase::ResponseCaptured => "response captured but no write/commit followed",
        CyclePhase::WriteApplied => "response write landed but no commit followed",
        CyclePhase::Committed | CyclePhase::Abandoned => "cycle is terminal",
    };
    format!(
        "[session-check] INTERRUPTED: state-backbone closeout projection cycle `{}` is `{}` — {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
        cycle_id,
        phase.as_str(),
        detail,
        file.display(),
        file.display()
    )
}

fn inspect_core(file: &Path, effects: &impl SessionCheckEffects) -> Result<SessionCheckStatus> {
    let initial_last_ops_event = agent_doc_ops_log_io::last_ops_event(file)?;

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

    let closeout_projection = agent_doc_cycle_state_io::load_closeout_projection(file)?;

    if let Some(mut state) = agent_doc_cycle_state_io::load(file)? {
        let projected_same_cycle = closeout_projection
            .as_ref()
            .filter(|projection| projection.matches_cycle(&state.cycle_id));
        apply_closeout_projection_to_cycle_state(file, &mut state, projected_same_cycle);
        if state.is_open() {
            if let Some(blocked) = state.blocked_closeout.as_ref() {
                return Ok(SessionCheckStatus::Interrupted(blocked_closeout_message(
                    file, &state, blocked,
                )));
            }
            if let Some(reason) = effects
                .recover_missing_commit_boundary(file, "session_check_commit_boundary_recovered")?
            {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` was `{}` ({}), recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        state.phase.as_str(),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` was `{}` ({}); recovered the missing commit boundary from {}",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    reason
                )));
            }
            if let Some(message) = crate::open_cycle_manual_patchback_message(file, &state)? {
                return Ok(SessionCheckStatus::Interrupted(message));
            }
            return Ok(SessionCheckStatus::Interrupted(crate::open_cycle_message(
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
        if matches!(state.phase, agent_doc_turn::CyclePhase::Abandoned)
            && state
                .last_event
                .contains("recursive_direct_invocation_blocked")
            && let Some(unresolved) = crate::unresolved_exchange_prompt(file)?
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
        if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
            if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    reason,
                    prompt_marker
                )));
            }
            return Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                state.cycle_id,
                state.phase.as_str(),
                state.last_event,
                reason
            )));
        }
        if let Some(marker) = crate::detect_bypassed_response_write(file)? {
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        state.phase.as_str(),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    reason
                )));
            }
            // #patchback-head-tolerant: the patchback heuristic compares the
            // snapshot against the working tree, never against HEAD — so a response
            // that WAS committed through `finalize` (it reached HEAD) but whose
            // snapshot sidecar is stale, or whose working tree was re-drifted by the
            // post-commit IPC listener (#postcommit-ipc-worktree-corruption), trips a
            // FALSE "direct response patchback without agent-doc cycle". If the flagged
            // `### Re:` heading is present in HEAD, the binary's write/commit path DID
            // run for it — it is not a bypassed patchback. Do not interrupt; fall
            // through to the remaining guards (post-commit drift etc.) which catch a
            // genuine working-tree problem without the false closeout-violation.
            let marker_committed_in_head = agent_doc_git_io::revision::show_head(file)?
                .is_some_and(|head| {
                    agent_doc_document::write_normalization::response_marker_present_in_content(
                        &head, &marker,
                    )
                });
            if marker_committed_in_head {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "session_check_patchback_tolerated_head_committed file={} marker={:?} (#patchback-head-tolerant)",
                        file.display(),
                        marker.chars().take(80).collect::<String>(),
                    ),
                );
            } else {
                return Ok(SessionCheckStatus::Interrupted(
                    agent_doc_workflow::session_check::likely_direct_response_patchback_message(
                        &marker,
                        &agent_doc_git_io::status::tracked_side_effect_note(file)?,
                        &effects.closeout_recovery_hint(file),
                    ),
                ));
            }
        }
        let mut latest_head_response_visible_in_live_buffer = false;
        let latest_head_response_missing = match agent_doc_git_io::revision::show_head(file)? {
            Some(head) => {
                if let Ok(disk) = std::fs::read_to_string(file)
                    && let Some(heading) =
                        agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
                            &head, &disk,
                        )
                    && operator_live_buffer_contains_heading(file, &heading)
                {
                    latest_head_response_visible_in_live_buffer = true;
                }
                match crate::resolve_current_document_content(file, "latest_head_response_missing")
                {
                    Ok(working) => {
                        let heading =
                        agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
                            &head, &working,
                        );
                        match heading {
                            Some(heading)
                                if operator_live_buffer_contains_heading(file, &heading) =>
                            {
                                latest_head_response_visible_in_live_buffer = true;
                                None
                            }
                            other => other,
                        }
                    }
                    Err(_) => None,
                }
            }
            None => None,
        };
        if let Some(heading) = latest_head_response_missing {
            return Ok(SessionCheckStatus::Interrupted(
                agent_doc_workflow::session_check::committed_head_response_missing_message(
                    &file.display().to_string(),
                    &state.cycle_id,
                    state.phase,
                    &state.last_event,
                    &heading,
                ),
            ));
        }
        if let Some(marker) = crate::detect_active_session_post_commit_drift(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                state.cycle_id,
                state.phase.as_str(),
                state.last_event,
                marker
            )));
        }
        if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
            if latest_head_response_visible_in_live_buffer {
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({})",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event
                )));
            }
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the document has uncommitted exchange changes beyond the committed snapshot: {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle before reporting success.",
                state.cycle_id,
                state.phase.as_str(),
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
                state.phase.as_str(),
                state.last_event,
                marker
            )));
        }
        return Ok(SessionCheckStatus::Ok(format!(
            "[session-check] ok — cycle `{}` is `{}` ({})",
            state.cycle_id,
            state.phase.as_str(),
            state.last_event
        )));
    }

    if let Some(projection) = closeout_projection.as_ref()
        && let Some(phase) = projection.phase
        && phase.is_open()
    {
        return Ok(SessionCheckStatus::Interrupted(
            projected_open_closeout_message(file, projection, phase),
        ));
    }

    match initial_last_ops_event {
        None => {
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
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
            if let Some(marker) = crate::detect_bypassed_response_write(file)? {
                if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
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
                    agent_doc_git_io::status::tracked_side_effect_note(file)?,
                    effects.closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = crate::detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
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
            if let Some(reason) = effects
                .recover_missing_commit_boundary(file, "session_check_commit_boundary_recovered")?
            {
                let repaired_cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
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
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
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
            if let Some(marker) = crate::detect_bypassed_response_write(file)? {
                if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
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
                    agent_doc_git_io::status::tracked_side_effect_note(file)?,
                    effects.closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = crate::detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
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

fn blocked_closeout_message(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    blocked: &agent_doc_cycle_state_io::BlockedCloseout,
) -> String {
    let editor_authority = blocked_closeout_editor_authority_note(file, blocked);
    let file_display = file.display().to_string();
    agent_doc_workflow::session_check::blocked_closeout_message(BlockedCloseoutMessage {
        file: &file_display,
        kind: &blocked.kind,
        cycle_id: &state.cycle_id,
        phase: state.phase,
        last_event: &state.last_event,
        source: &blocked.source,
        reason: &blocked.reason,
        patch_id: blocked.patch_id.as_deref(),
        recovery: blocked.recovery.as_deref(),
        detail: blocked.detail.as_deref(),
        recovery_command: blocked.recovery_command.as_deref(),
        editor_authority_note: &editor_authority,
    })
}

fn blocked_closeout_editor_authority_note(
    file: &Path,
    blocked: &agent_doc_cycle_state_io::BlockedCloseout,
) -> String {
    if blocked.kind != "editor_convergence_required" {
        return String::new();
    }
    let Ok(canonical) = file.canonicalize() else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(&canonical) else {
        return String::new();
    };
    let Some(snapshot) = agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
        &canonical.to_string_lossy(),
        &content,
    ) else {
        return String::new();
    };
    let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
    format!(
        " Live editor `{editor_id}` lacks required capability `{}`; reload or restart the editor plugin before retrying so delivery can preserve operator text.",
        agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
    )
}

fn detect_duplicate_response_patchback(file: &Path) -> Result<Option<String>> {
    let content = crate::resolve_current_document_content(file, "duplicate_response_patchback")?;
    Ok(agent_doc_turn::response_replay::first_duplicate_response_heading(&content))
}
