use agent_doc_turn::repair::RepairOutcome;
use anyhow::{Context, Result};
use std::path::Path;

/// Durable identity of a binary-owned finalize operation. The response capture
/// and cycle projection already persist these fields, so the supervisor can
/// resume after the originating agent process has returned or crashed without
/// recapturing (and therefore without duplicating) the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFinalizeResumeKey {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedFinalizeResumeOutcome {
    Committed {
        repair_outcome: String,
    },
    Superseded,
    /// The effect completed successfully enough to observe that a newer
    /// document-state edge is required. Do not put this on a timer.
    WaitingForSignal {
        reason: String,
    },
    /// A state edge may arrive sooner, but lease expiry is itself a typed time
    /// edge and must re-arm the same capture even when no process releases it.
    RetryAt {
        reason: String,
        retry_at_secs: u64,
    },
    /// The effect itself failed transiently; controller reconnect/backoff may
    /// retry this exact capture without inventing a new response operation.
    RetryableEffect {
        reason: String,
    },
    NeedsOperator {
        reason: String,
    },
}

pub fn run_write_command_with_empty_response_recovery(
    options: agent_doc_write_command_io::CommandOptions,
    commit_mode: agent_doc_write_command_io::CommitMode,
) -> Result<()> {
    agent_doc_write_runtime_io::run_command_with_empty_response_recovery(
        options,
        commit_mode,
        recover_empty_response_for_strict_closeout,
    )
}

pub fn recover_empty_response_for_strict_closeout(
    file: &Path,
    strict_closeout: bool,
    has_pending_mutation: bool,
    force_disk: bool,
) -> Result<bool> {
    agent_doc_repair_io::recover_empty_response_for_strict_closeout(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        strict_closeout,
        has_pending_mutation,
        Some(force_disk),
    )
}

#[cfg(test)]
pub fn run(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::run(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::repair(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

/// Reclaim the empty preflight owned by a harness run that has already reached
/// a live actor boundary. Callers must establish that boundary before granting
/// the run-cancel authority used here.
pub fn cancel_preflight_cycle_after_run_cancel(
    file: &Path,
) -> Result<agent_doc_turn::repair::CancelOutcome> {
    agent_doc_repair_io::cancel_preflight_cycle_after_run_cancel(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        file,
    )
}

/// Return the stable operation key only when the durable cycle contains a
/// non-empty captured response that is eligible for binary-owned closeout.
pub fn captured_finalize_resume_key(file: &Path) -> Result<Option<CapturedFinalizeResumeKey>> {
    if let Some(capture) =
        agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
        && let Some(key) = captured_finalize_resume_key_from_capture(&capture)
    {
        return Ok(Some(key));
    }
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !matches!(
        state.phase,
        agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
    ) {
        return Ok(None);
    }
    let (Some(capture_id), Some(response_sha256)) = (
        state.capture_id.as_deref(),
        state.response_sha256.as_deref(),
    ) else {
        return Ok(None);
    };
    let Some(capture) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    else {
        return Ok(None);
    };
    if capture.cycle_id != state.cycle_id || capture.response_sha256 != response_sha256 {
        return Ok(None);
    }
    Ok(captured_finalize_resume_key_from_capture(&capture))
}

fn captured_finalize_resume_key_from_capture(
    capture: &agent_doc_cycle_state_io::ProjectedCapturedResponse,
) -> Option<CapturedFinalizeResumeKey> {
    (!capture.response_body.trim().is_empty()).then(|| CapturedFinalizeResumeKey {
        cycle_id: capture.cycle_id.clone(),
        capture_id: capture.capture_id.clone(),
        response_sha256: capture.response_sha256.clone(),
    })
}

/// Resume one exact captured finalize operation through the existing strict
/// repair/write/commit path. Disk fallback is explicitly disabled: a live or
/// reconnecting editor remains authoritative, and convergence failures retain
/// the same durable capture until a later controller state edge.
pub fn resume_captured_finalize(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> CapturedFinalizeResumeOutcome {
    let current = match captured_finalize_resume_key(file) {
        Ok(current) => current,
        Err(err) => {
            return classify_captured_finalize_resume_error(&format!("{err:#}"));
        }
    };
    if current.as_ref() != Some(expected) {
        return CapturedFinalizeResumeOutcome::Superseded;
    }

    // `#writeappliedcontinuation`: the response command owns capture and
    // materialization only through `write_applied`. Once that durable phase is
    // visible, replaying the command would start capture again after pending
    // response intent has already been cleared. Continue the exact capture
    // through retained delivery/snapshot/commit instead.
    let projected_phase = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()
        .filter(|state| {
            state.cycle_id == expected.cycle_id
                && state.capture_id.as_deref() == Some(expected.capture_id.as_str())
                && state.response_sha256.as_deref() == Some(expected.response_sha256.as_str())
        })
        .map(|state| state.phase);
    if matches!(
        projected_phase,
        Some(agent_doc_turn::CyclePhase::WriteApplied | agent_doc_turn::CyclePhase::Committed)
    ) {
        return resume_materialized_captured_finalize(file, expected);
    }

    let result = resume_captured_finalize_intent(file, expected);
    match result {
        Ok(outcome) => {
            let committed = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                .ok()
                .flatten()
                .is_some_and(|state| {
                    state.cycle_id == expected.cycle_id
                        && matches!(state.phase, agent_doc_turn::CyclePhase::Committed)
                });
            if committed {
                CapturedFinalizeResumeOutcome::Committed {
                    repair_outcome: format!("{outcome:?}"),
                }
            } else if captured_finalize_resume_key(file).ok().flatten().as_ref() != Some(expected) {
                CapturedFinalizeResumeOutcome::Superseded
            } else {
                CapturedFinalizeResumeOutcome::WaitingForSignal {
                    reason: format!(
                        "strict repair returned {outcome:?} but the captured cycle is still open"
                    ),
                }
            }
        }
        Err(err) => classify_captured_finalize_resume_error(&format!("{err:#}")),
    }
}

/// Load the durable capture that matches `expected`, retained projection first.
fn captured_closeout_for(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> Result<Option<agent_doc_cycle_state_io::ProjectedCapturedResponse>> {
    let matches_expected = |capture: &agent_doc_cycle_state_io::ProjectedCapturedResponse| {
        capture.cycle_id == expected.cycle_id
            && capture.capture_id == expected.capture_id
            && capture.response_sha256 == expected.response_sha256
    };
    if let Some(capture) =
        agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
            .filter(matches_expected)
    {
        return Ok(Some(capture));
    }
    Ok(
        agent_doc_cycle_state_io::load_projected_captured_response(file, &expected.capture_id)?
            .filter(|capture| {
                capture.cycle_id == expected.cycle_id
                    && capture.response_sha256 == expected.response_sha256
            }),
    )
}

/// `#resumereplaynotidempotent`: drop the plan entries the document already
/// shows, so a resume cannot replay a mutation that provably landed.
///
/// `recorded_tracked_work_is_unlanded` answers for the cycle as a whole, and its
/// only id witnesses are `--done` ids, explicit add ids, and
/// `requested_mutations && !mutations_applied`. `--review-resolve`,
/// `--review-remove` and `--backlog-ungate` carry no id witness, and
/// `mutations_applied` flips only after the pending-write *transaction* succeeds
/// — so when the mutation content reached the document but its transaction was
/// retained, the flag under-reports and the whole plan is replayed.
///
/// Those three commands are not idempotent: each fails when its target is gone.
/// Observed live 2026-09-20 on `cycle-1789880782658` — the envelope had applied
/// (ops.log showed `review-resolve: archived 1 entry for #fpecompactheal`), the
/// write was retained, and every resume then refused with `review item not
/// found: #fpecompact…`. That retry can never succeed: the id will never return
/// to `agent:review`, so the cycle latched at `write_applied` until a manual
/// `agent-doc commit` cleared it.
///
/// The fix is per-entry, witnessed against the already-resolved current content,
/// and deliberately local to this function so the four guarded
/// `recorded_tracked_work_is_unlanded` call sites are untouched. Both witnesses
/// fail safe toward *keeping* a replay, because a dropped mutation is silent
/// tracked-work loss while a kept one is caught by the write path.
///
/// Returns a `shape:#id` descriptor per dropped entry, for the ops log.
/// The id an ops-log drop descriptor should name.
///
/// Bare-id flags carry the id itself; the `id=text` edit flags carry a pair, and
/// logging the whole pair would put an arbitrary backlog body in the ops log.
fn dropped_entry_id(entry: &str) -> &str {
    let id = entry.split_once('=').map_or(entry, |(id, _)| id);
    id.trim().trim_start_matches('#')
}

pub fn drop_already_applied_mutations(
    options: &mut agent_doc_write_command_io::CommandOptions,
    current_content: &str,
) -> Vec<String> {
    let mut dropped = Vec::new();
    let mut retain_witnessed =
        |shape: &str, entries: &mut Vec<String>, still_pending: &dyn Fn(&str) -> bool| {
            entries.retain(|entry| {
                if still_pending(entry) {
                    return true;
                }
                dropped.push(format!("{shape}:#{}", dropped_entry_id(entry)));
                false
            });
        };
    // Resolve and remove both TAKE the item out of `agent:review`, so presence
    // in that component is the exact "not yet applied" witness. Not
    // `collect_gated_review_ids`: it matches only the untyped `- [/]` prefix, so
    // a typed gate (`- [/release]`) would read as applied and be dropped.
    let review_witness =
        |id: &str| agent_doc_element_review::review_component_contains_id(current_content, id);
    retain_witnessed(
        "review-resolve",
        &mut options.review_resolve,
        &review_witness,
    );
    retain_witnessed("review-remove", &mut options.review_remove, &review_witness);
    // Ungate resolves against `agent:review` first and falls back to the backlog
    // component, so "absent from review" is the WRONG witness for it — an item
    // gated in the backlog would read as already ungated.
    retain_witnessed("backlog-ungate", &mut options.pending_ungate, &|id| {
        agent_doc_element_review::id_is_gated_in_tracked_work(current_content, id)
    });
    // The `id=text` edit family. Each searches exactly ONE component and errors
    // when the id is not in it, so each needs its own scope: an item this cycle
    // gated out of `agent:backlog` into `agent:review` is still plainly present
    // in the document while being unreachable to `--backlog-edit`. Observed live
    // 2026-09-20 on `cycle-1789921516976`, where `#ads-delivery-blackout` moved
    // to review and the replayed edit refused forever with `pending edit: no
    // item with id [#ads-delivery-blackout]`.
    //
    // Re-applying an edit whose target IS still present is idempotent — it sets
    // the text to the same value — so "present" safely keeps the replay.
    for (shape, pairs, scope) in [
        (
            "backlog-edit",
            &mut options.pending_edit,
            agent_doc_element_review::TrackedWorkScope::Backlog,
        ),
        (
            "icebox-edit",
            &mut options.icebox_edit,
            agent_doc_element_review::TrackedWorkScope::Icebox,
        ),
        (
            "review-edit",
            &mut options.review_edit,
            agent_doc_element_review::TrackedWorkScope::Review,
        ),
    ] {
        retain_witnessed(shape, pairs, &|pair| {
            // A pair with no `=` is malformed; leave it for the write path to
            // reject with its own message rather than silently swallowing it.
            let Some((id, _)) = pair.split_once('=') else {
                return true;
            };
            agent_doc_element_review::tracked_work_id_present(current_content, id, scope)
        });
    }
    // `#resumereplayauditrest`: the remaining error-on-missing shapes.
    //
    // `--backlog-gate` refuses only once the id is open NOWHERE: `gate` takes
    // the item from the backlog, else accepts an already-open review item as a
    // NoOp, else falls through to `gate_in_place`, whose `op_gate` is the only
    // step that errors. Re-gating an item that IS still open is a
    // `validate_transition` NoOp, so keeping that replay is free.
    //
    // `--backlog-set-gate-type` and `--backlog-set-verify` resolve through
    // `find_open_tracked_work_component_in_content`, which fails with `id not
    // found in backlog/icebox` on the same condition. Their `id=value` pairs
    // reuse `dropped_entry_id` so the ops log names the id, not the payload.
    let open_witness =
        |id: &str| agent_doc_element_review::id_is_open_in_tracked_work(current_content, id);
    retain_witnessed("backlog-gate", &mut options.pending_gate, &open_witness);
    let open_pair_witness = |pair: &str| {
        // Malformed pairs belong to the write path's own error message.
        let Some((id, _)) = pair.split_once('=') else {
            return true;
        };
        open_witness(id)
    };
    retain_witnessed(
        "backlog-set-gate-type",
        &mut options.pending_set_gate_type,
        &open_pair_witness,
    );
    retain_witnessed(
        "backlog-set-verify",
        &mut options.pending_set_verify,
        &open_pair_witness,
    );
    // `--backlog-reorder` is ONE comma-separated id list, but `op_reorder`
    // validates every id against the backlog component before reordering
    // anything and bails on the first missing one — so a single reaped id kills
    // the whole reorder forever. The witness is therefore per id: drop the ids
    // the backlog no longer lists, keep the reorder for the survivors, and drop
    // the flag entirely when none survive. Any state counts, because
    // `op_reorder` sees `[x]` items that have not been reaped yet.
    if let Some(order) = options.pending_reorder.take() {
        let mut kept: Vec<String> = Vec::new();
        for id in order.split(',').map(str::trim).filter(|id| !id.is_empty()) {
            if agent_doc_element_review::id_is_in_backlog_any_state(current_content, id) {
                kept.push(id.to_string());
            } else {
                dropped.push(format!("backlog-reorder:#{}", dropped_entry_id(id)));
            }
        }
        // A single surviving id is still a real permutation — `op_reorder`
        // moves it to the head — so keep the flag whenever anything survives.
        if !kept.is_empty() {
            options.pending_reorder = Some(kept.join(","));
        }
    }
    dropped
}

/// `#deferredmutdrop`: replay this closeout's tracked-work half when only its
/// response half materialized.
///
/// A capture reaches `write_applied` as soon as the response cell is durable.
/// The tracked-work half runs AFTER that write and can be retained by the same
/// no-live-editor barrier that retained the response — and the retained document
/// write carries only the response target. Continuing straight to commit from
/// there publishes a half-applied cycle: observed live 2026-09-10 on
/// `cycle-1789080212116`, where `respond --done ... --backlog-gate ...
/// --backlog-add ...` committed its response as `3e61bbb726` while backlog,
/// queue, and review stayed byte-identical to their pre-cycle state, and manual
/// `write --pending-only --commit` + `commit` was the only recovery.
///
/// The capture already carries the validated mutation plan
/// (`capture_closeout_mutation_plan_before_authority_resolution` attaches it to
/// the same capture precisely so recovery cannot commit only the response), so
/// replay it as a tracked-work-only write with NO commit: the materialized
/// continuation below still commits both halves as one transaction.
///
/// The replay is gated on this cycle's own recorded mutations still being
/// unlanded — the same predicate `commit` and `session-check` use — so a
/// closeout whose mutations DID apply is left alone and a resume that runs
/// repeatedly cannot double-apply. An error here is deliberately propagated:
/// a mutation half that cannot land must fail closed rather than let the
/// continuation report a committed cycle.
fn apply_unlanded_captured_mutation_plan(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> Result<bool> {
    let Some(capture) = captured_closeout_for(file, expected)? else {
        return Ok(false);
    };
    let Some(plan_json) = capture.mutation_plan_json.as_deref() else {
        return Ok(false);
    };
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(false);
    };
    if state.cycle_id != expected.cycle_id {
        return Ok(false);
    }
    let current_content =
        agent_doc_document_realtime_io::try_resolve_current_doc_from_file_with_source(
            file,
            "captured_finalize_resume_tracked_work_witness",
        )
        .map(|resolved| resolved.content)?;
    if !agent_doc_turn::write_ownership::recorded_tracked_work_is_unlanded(
        agent_doc_turn::write_ownership::RecordedTrackedWork {
            done_ids: &state.pending_done_ids,
            added_ids: &state.pending_added_ids,
            requested_done_ids: &state.requested_done_ids,
            requested_added_ids: &state.requested_added_ids,
            // `#mutplanwitness`: a captured plan carrying only gates,
            // ungates, edits, reorders, review-edits or a --status change has
            // no id the text witnesses can see, so without this the resume
            // reported "landed" and dropped the tracked-work half.
            requested_mutations: state.requested_tracked_work_mutations,
            mutations_applied: state.tracked_work_mutations_applied,
        },
        &current_content,
    ) {
        return Ok(false);
    }
    let plan: agent_doc_write_command_io::CapturedCloseoutMutationPlan =
        serde_json::from_str(plan_json)
            .map_err(|err| anyhow::anyhow!("decode captured closeout mutation plan: {err}"))?;
    let mut options =
        agent_doc_write_command_io::CommandOptions::recovery_from_captured_closeout_mutation_plan(
            file, plan,
        );
    // A tracked-work-only write rejects the response-shaped transports.
    options.is_template = false;
    options.is_stream = false;
    options.is_ipc = false;
    options.pending_only = true;
    options.origin = Some("captured_finalize_resume_tracked_work".to_string());
    let dropped = drop_already_applied_mutations(&mut options, &current_content);
    if !dropped.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "captured_finalize_resume_tracked_work_already_applied file={} cycle_id={} capture_id={} dropped={}",
                file.display(),
                expected.cycle_id,
                expected.capture_id,
                dropped.join(","),
            ),
        );
    }
    if !options.has_pending_mutation() {
        return Ok(false);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "captured_finalize_resume_tracked_work_replay file={} cycle_id={} capture_id={} plan_hash={} recovery=pending_only_no_commit",
            file.display(),
            expected.cycle_id,
            expected.capture_id,
            agent_doc_hash::content_hash(plan_json),
        ),
    );
    agent_doc_write_runtime_io::run_command_with_response(
        options,
        agent_doc_write_command_io::CommitMode::None,
        String::new(),
    )
    .with_context(|| {
        format!(
            "the response half of the captured closeout for {} is materialized but its tracked-work half could not be replayed; refusing to commit a half-applied cycle",
            file.display()
        )
    })?;
    Ok(true)
}

fn resume_materialized_captured_finalize(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> CapturedFinalizeResumeOutcome {
    use agent_doc_session_check_io::SessionCheckEffects;

    // `#deferredmutdrop`: continue the SAME closeout, not just its response half.
    if let Err(err) = apply_unlanded_captured_mutation_plan(file, expected) {
        return classify_captured_finalize_resume_error(&format!("{err:#}"));
    }

    match agent_doc_closeout_runtime_io::session_check_effects().resume_captured_finalize(file) {
        Ok(agent_doc_session_check_io::CapturedFinalizeResumeOutcome::Committed) => {
            CapturedFinalizeResumeOutcome::Committed {
                repair_outcome: "ContinuedWriteAppliedCapture".to_string(),
            }
        }
        Ok(agent_doc_session_check_io::CapturedFinalizeResumeOutcome::Superseded) => {
            CapturedFinalizeResumeOutcome::Superseded
        }
        Ok(agent_doc_session_check_io::CapturedFinalizeResumeOutcome::Retained { reason }) => {
            CapturedFinalizeResumeOutcome::WaitingForSignal { reason }
        }
        Ok(agent_doc_session_check_io::CapturedFinalizeResumeOutcome::RetryAt {
            reason,
            retry_at_secs,
        }) => CapturedFinalizeResumeOutcome::RetryAt {
            reason,
            retry_at_secs,
        },
        Ok(agent_doc_session_check_io::CapturedFinalizeResumeOutcome::NotApplicable) => {
            CapturedFinalizeResumeOutcome::WaitingForSignal {
                reason: "the materialized captured closeout continuation is not yet applicable"
                    .to_string(),
            }
        }
        Err(err) => classify_captured_finalize_resume_error(&format!("{err:#}")),
    }
}

/// Run the operator-facing status-only session check. Durable capture replay is
/// owned by finalize/write/preflight and the route supervisor; observing status
/// must never manufacture a second document mutation.
pub fn run_session_check(file: &Path, codex_final_gate: bool) -> Result<()> {
    agent_doc_session_check_io::run_read_only_with_options(
        file,
        codex_final_gate,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
}

fn resume_captured_finalize_intent(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> Result<RepairOutcome> {
    let retained = agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
        .filter(|capture| {
            capture.cycle_id == expected.cycle_id
                && capture.capture_id == expected.capture_id
                && capture.response_sha256 == expected.response_sha256
        });
    let capture = match retained {
        Some(capture) => Some(capture),
        None => {
            agent_doc_cycle_state_io::load_projected_captured_response(file, &expected.capture_id)?
                .filter(|capture| {
                    capture.cycle_id == expected.cycle_id
                        && capture.response_sha256 == expected.response_sha256
                })
        }
    };
    if let Some(capture) = capture
        && let Some(plan_json) = capture.mutation_plan_json.as_deref()
    {
        let plan: agent_doc_write_command_io::CapturedCloseoutMutationPlan =
            serde_json::from_str(plan_json)
                .map_err(|err| anyhow::anyhow!("decode captured closeout mutation plan: {err}"))?;
        let options =
            agent_doc_write_command_io::CommandOptions::recovery_from_captured_closeout_mutation_plan(
                file, plan,
            );
        let mut response = capture.intent_body.unwrap_or(capture.response_body);
        if let Some(normalized) =
            agent_doc_template::response_materialization::normalize_retained_legacy_patch_markers(
                &response,
            )
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "captured_finalize_legacy_patch_markers_normalized file={} cycle_id={} capture_id={} strategy=transient_replay_only",
                    file.display(),
                    expected.cycle_id,
                    expected.capture_id,
                ),
            );
            response = normalized;
        }
        agent_doc_write_runtime_io::run_command_with_response(
            options,
            agent_doc_write_command_io::CommitMode::Required,
            response,
        )?;
        return Ok(RepairOutcome::ReplayedResponse);
    }

    // Backward-compatible recovery for captures written before the typed
    // mutation plan became part of the Lazily intent.
    agent_doc_repair_io::repair_preserving_live_authority(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

fn classify_captured_finalize_resume_error(reason: &str) -> CapturedFinalizeResumeOutcome {
    if let Some(retry_at_secs) = reason
        .split_once(agent_doc_write_runtime_io::CLOSEOUT_OWNER_RETRY_AT_MARKER)
        .and_then(|(_, suffix)| {
            suffix
                .split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .filter(|digits| !digits.is_empty())
        .and_then(|digits| digits.parse::<u64>().ok())
    {
        return CapturedFinalizeResumeOutcome::RetryAt {
            reason: reason.to_string(),
            retry_at_secs,
        };
    }
    let lower = reason.to_ascii_lowercase();
    // Every needle here matches error *prose* except one. `#retainconv`:
    // `await_editor_replica` was written for the name of the typed error the
    // whole retained-write class is built from — but `Display` prints only the
    // message, so nothing ever carried it and the canonical "wait for a state
    // edge" failure took the default `NeedsOperator` arm instead. The emitting
    // constructor now stamps `AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN`
    // (`agent-doc-document-realtime-io`) into every such refusal, which is what
    // makes this needle true by construction rather than by coincidence of
    // wording — including across the process boundaries (ops-log `reason_head`,
    // retained-capture reason, Codex stop hook) where a typed downcast cannot
    // reach.
    let waiting_for_signal = [
        "retry_without_disk_write",
        "editor_convergence_required",
        "compare_and_swap_raced",
        "await_editor_replica",
        "no relay replica is registered",
        "editor authority stayed",
        "active recovery attempt",
        "operation is already in progress",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if waiting_for_signal {
        return CapturedFinalizeResumeOutcome::WaitingForSignal {
            reason: reason.to_string(),
        };
    }
    let retryable_effect = [
        "controller_model_backpressure",
        "timed out",
        "timeout",
        "resource temporarily unavailable",
        "connection reset",
        "broken pipe",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if retryable_effect {
        return CapturedFinalizeResumeOutcome::RetryableEffect {
            reason: reason.to_string(),
        };
    }
    CapturedFinalizeResumeOutcome::NeedsOperator {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod captured_finalize_resume_tests {
    use super::*;

    #[test]
    fn convergence_failures_wait_for_a_state_edge() {
        for reason in [
            "recovery=retry_without_disk_write",
            "closeout blocked by editor_convergence_required",
            "compare_and_swap_raced",
        ] {
            assert!(matches!(
                classify_captured_finalize_resume_error(reason),
                CapturedFinalizeResumeOutcome::WaitingForSignal { .. }
            ));
        }
    }

    #[test]
    fn closeout_owner_collision_preserves_its_lease_deadline() {
        let reason = format!(
            "tracked-work replay: closeout operation is already in progress [{}123456]",
            agent_doc_write_runtime_io::CLOSEOUT_OWNER_RETRY_AT_MARKER,
        );
        assert!(matches!(
            classify_captured_finalize_resume_error(&reason),
            CapturedFinalizeResumeOutcome::RetryAt {
                retry_at_secs: 123456,
                ..
            }
        ));
    }

    #[test]
    fn transport_and_backpressure_failures_retry_the_effect() {
        for reason in [
            "controller_model_backpressure",
            "controller lookup timed out",
            "connection reset by peer",
        ] {
            assert!(matches!(
                classify_captured_finalize_resume_error(reason),
                CapturedFinalizeResumeOutcome::RetryableEffect { .. }
            ));
        }
    }

    /// `#retainconv` — the 2026-08-23 `agent-doc-bugs.md` latch.
    ///
    /// A retained-delivery refusal is the canonical "wait for a state edge"
    /// failure, yet none of the prose needles matched it, so it took the default
    /// arm and demanded an operator for a write that had already landed on disk.
    /// Bind the assertion to the emitting crate's exported token, not to a copy
    /// of the message, so the two sides cannot drift apart silently.
    #[test]
    fn a_retained_delivery_refusal_waits_for_a_state_edge() {
        let reason = format!(
            "queue consume: failed to write document: visible document write for plan.md is \
             retained by the lazy delivery projection because the editor state projection has not \
             converged; no secondary snapshot/commit or forced disk write was attempted. [{}]",
            agent_doc_document_realtime_io::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
        );
        assert!(
            matches!(
                classify_captured_finalize_resume_error(&reason),
                CapturedFinalizeResumeOutcome::WaitingForSignal { .. }
            ),
            "a deferred write must not demand an operator: {reason}"
        );
    }

    #[test]
    fn ambiguous_content_failure_requires_operator_instead_of_retrying() {
        assert!(matches!(
            classify_captured_finalize_resume_error(
                "visible user-authored content diverged from the captured baseline"
            ),
            CapturedFinalizeResumeOutcome::NeedsOperator { .. }
        ));
    }

    #[test]
    fn retained_capture_produces_a_resume_key_without_current_cycle_state() {
        let capture = agent_doc_cycle_state_io::ProjectedCapturedResponse {
            cycle_id: "retained-cycle".to_string(),
            capture_id: "retained-capture".to_string(),
            response_sha256: "retained-response-sha".to_string(),
            response_body: "response body".to_string(),
            intent_body: None,
            mutation_plan_json: None,
            file_hash: None,
            snapshot_hash: None,
            baseline_content: None,
        };

        assert_eq!(
            captured_finalize_resume_key_from_capture(&capture),
            Some(CapturedFinalizeResumeKey {
                cycle_id: "retained-cycle".to_string(),
                capture_id: "retained-capture".to_string(),
                response_sha256: "retained-response-sha".to_string(),
            }),
        );
    }
}
