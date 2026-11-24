//! Binary-owned completion feedback for background-subagent queue claims.
//!
//! Workers never write the shared session document. The coordinator reports a
//! worker result through `queue report-complete`. The binary appends the actual
//! worker response as one inert, attested artifact in `agent:exchange` through
//! the editor/CRDT authority.
//! Reporting keeps the claim active while integration and verification run;
//! ordinary claim release never implies that a worker completed successfully.

use std::path::{Path, PathBuf};

use agent_doc_element::component_scope::{changed_component_scope, with_component_write_scope};
use agent_doc_queue::QueueReportState;
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result, bail};

const SOURCE: &str = "queue_subagent_completion_feedback";
use agent_doc_queue_io::completion_feedback_lock::{
    COMPLETION_FEEDBACK_LOCK_WAIT, CompletionFeedbackLock, ProgressLockRound, ProgressLockWait,
};
// The cell grammar and its structural recognition are shared with every
// recovery/closeout classifier (`agent_doc_document::subagent_completion`).
use agent_doc_document::subagent_completion::{
    BODY as COMPLETION_BODY, LEGACY_HEADING_PREFIX as LEGACY_COMPLETION_HEADING_PREFIX,
};

#[derive(Debug)]
pub(crate) struct CompletionFeedbackOutcome {
    pub claim: QueueClaim,
    /// The operator gave the original row children after the worker bound.
    /// Its response is evidence for that attempt, with no queue row to close.
    pub structural_context: bool,
    pub appended: bool,
    pub replaced: bool,
    /// Durable editor-authority target still awaiting visible projection.
    pub projection_pending: bool,
    pub intent_id: Option<String>,
    /// The hand-back's action items queued as follow-up rows in this same
    /// successor (`#workerresponsestructure`).
    pub followups: agent_doc_queue::handback_followups::HandbackFollowups,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryCompletion {
    pub source_repo: PathBuf,
    pub source_sha: String,
    pub sink_repo: PathBuf,
    pub sink_ref: String,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    pub base_sha: Option<String>,
    pub pushed_ref: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CompletionFeedbackRepairOutcome {
    pub repaired: usize,
    pub accepted: bool,
}

/// `#goalpresetterminal`: before reporting, requeue a goal-loop row's
/// hand-back that does not open on a declared final state. Serialized with
/// `report-complete` under the same completion-feedback lock and resolved
/// against the same live document cut. `Ok(None)` means the ordinary report
/// path applies.
pub(crate) fn requeue_nonterminal_goal_hand_back(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    response: &str,
) -> Result<Option<agent_doc_queue_io::goal_loop_handoff::GoalLoopRequeue>> {
    let _lock = acquire_completion_lock(
        file,
        "report subagent completion",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    let _apply_guard = agent_doc_queue_io::queue_edit_owner::QueueEditGuard::acquire(file)?;
    let resolved =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let current =
        agent_doc_document_realtime_io::deferred_document_write_reconnect_content(file, &resolved)?
            .unwrap_or(resolved);
    agent_doc_queue_io::goal_loop_handoff::requeue_nonterminal_hand_back(
        file, &current, item, generation, response,
    )
}

/// The exact attempt a prompt-pressure hand-back named (`#promptpressure`).
/// A review hand-back closes the attempt, so this is the attempt's identity,
/// not a live claim (`#blockerreportterminal`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PressuredAttempt {
    pub item_text: String,
    pub owner: String,
    pub generation: u64,
}

impl From<QueueClaim> for PressuredAttempt {
    fn from(claim: QueueClaim) -> Self {
        Self {
            item_text: claim.item_text,
            owner: claim.owner,
            generation: claim.generation,
        }
    }
}

/// A prompt hand-back `queue report-complete` requeued instead of reporting
/// (`#promptpressure`).
#[derive(Debug)]
pub(crate) struct PromptPressureRequeue {
    pub claim: PressuredAttempt,
    pub kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind,
    /// Set when the hand-back was a review structure.
    pub review_id: Option<String>,
    pub external_evidence: Option<String>,
    pub incomplete: String,
    pub complete_by: String,
    /// `#closepressure`: requeued because the row requires a pull request the
    /// hand-back neither opened nor traced to an earlier attempt of the row.
    pub close_pressure: bool,
}

/// `#promptpressure`: before reporting, requeue a hand-back that does not
/// finish the prompt. A review structure is hoisted to the front of
/// `agent:review` first; the essay is not written into `agent:exchange`.
/// `Ok(None)` means the ordinary report path applies: no live subagent claim,
/// a goal hand-back that already opens on a declared final state, execution
/// evidence, or a prose blocker that also delivered a repository result.
pub(crate) fn requeue_prompt_pressure_hand_back(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    response: &str,
    delivered: bool,
) -> Result<Option<PromptPressureRequeue>> {
    requeue_prompt_pressure_hand_back_with(
        file,
        item,
        generation,
        response,
        delivered,
        agent_doc_queue_io::attempt_lineage_effect::github_pull_request_facts,
    )
}

/// [`requeue_prompt_pressure_hand_back`] reading a cited pull request's facts
/// with `observe` (`#attemptlineage`).
fn requeue_prompt_pressure_hand_back_with(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    response: &str,
    delivered: bool,
    observe: impl FnMut(
        &agent_doc_queue::attempt_lineage_effect::CitedPullRequest,
    ) -> Option<agent_doc_queue::attempt_lineage_effect::PullRequestFacts>,
) -> Result<Option<PromptPressureRequeue>> {
    let _lock = acquire_completion_lock(
        file,
        "report subagent completion",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    let resolved =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let current =
        agent_doc_document_realtime_io::deferred_document_write_reconnect_content(file, &resolved)?
            .unwrap_or(resolved);
    let ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, &current)?;
    let Some(claim) = ledger.resolve_selector(item)?.cloned() else {
        return recorded_prompt_blocker_hand_back(file, &current, item, generation, response);
    };
    if !claim.owner.starts_with("subagent:")
        || agent_doc_queue_io::queue_claim::bound_attempt_is_structural_context(&current, &claim)
    {
        return Ok(None);
    }
    let blocker = agent_doc_queue::prompt_pressure::parse_prompt_blocker(response);
    let external = blocker
        .as_ref()
        .is_some_and(|blocker| blocker.evidence.is_some());
    anyhow::ensure!(
        !external || generation.is_some() || claim.generation == 0,
        "external blocker requires --generation for the exact started worker attempt"
    );
    let generation = generation.unwrap_or(claim.generation);
    if generation != claim.generation {
        anyhow::bail!(
            "stale queue claim generation {generation}; current generation is {}",
            claim.generation
        );
    }
    let presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(&current, file);
    let final_states = agent_doc_queue::goal_preset_terminal::head_final_states_with_presets(
        &claim.item_text,
        &presets,
    );
    let bodies = agent_doc_queue::goal_preset_terminal::head_goal_preset_bodies_with_presets(
        &claim.item_text,
        &presets,
    );
    // `#operatororderclosed`: an already-answered human-review BLOCKED is not final.
    if !external
        && agent_doc_queue::goal_preset_terminal::hand_back_completes_goal_loop(
            response,
            &final_states,
            &bodies,
        )
    {
        return Ok(None);
    }
    if let Some(mut blocker) = blocker {
        if external {
            anyhow::ensure!(
                blocker.matches_prompt(&claim.item_text),
                "external blocker names a different queue row than {:?}",
                claim.item_text
            );
            anyhow::ensure!(
                claim.worker.is_some()
                    && matches!(
                        claim.state,
                        agent_doc_queue::queue_claim::QueueClaimState::Running
                            | agent_doc_queue::queue_claim::QueueClaimState::Lost
                    ),
                "external blocker requires the exact started, unfinished worker attempt"
            );
            blocker.bind_external_hold(&claim.item_text);
        }
        let prior_hold = external
            .then(|| {
                agent_doc_queue::prompt_pressure::open_review_blocking_prompt(
                    &current,
                    &claim.item_text,
                )
            })
            .flatten()
            .filter(|prior| prior.id == blocker.id && prior.evidence == blocker.evidence);
        if external
            && claim
                .frozen
                .as_ref()
                .is_some_and(agent_doc_queue::queue_claim::QueueWorkerFreeze::is_abandon)
        {
            anyhow::ensure!(
                prior_hold.is_some(),
                "external blocker for {:?} is from a relinquished attempt; current evidence superseded it",
                claim.item_text
            );
            // `#blockerreportterminal`: an older binary retained this
            // attempt abandoned. The review already holds the row, so close
            // it now rather than leave it reading `replacement_pending`.
            agent_doc_queue_io::queue_claim::close_attempt_for_prompt_blocker(
                file,
                &claim.item_text,
                claim.generation,
            )?;
            return Ok(Some(PromptPressureRequeue {
                claim: claim.into(),
                kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review,
                review_id: Some(blocker.id),
                external_evidence: blocker.evidence,
                incomplete: blocker.incomplete,
                complete_by: blocker.complete_by,
                close_pressure: false,
            }));
        }
        let updated = if prior_hold.is_some() {
            current.clone()
        } else {
            agent_doc_queue::prompt_pressure::hoist_prompt_blocker(&current, &blocker)
        };
        if !updated.contains(&format!("[#{}]", blocker.id)) {
            anyhow::bail!(
                "prompt-blocker review #{} was not written into agent:review (#promptpressure)",
                blocker.id
            );
        }
        if updated != current {
            publish_document_update(file, &current, &updated)?;
        }
        if external {
            // A retained editor write alone cannot fence queue consumers.
            // Keep the exact worker attempt until the current authority
            // exposes the hold, then the ordinary fenced requeue frees it.
            let authoritative =
                agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
            anyhow::ensure!(
                agent_doc_queue::prompt_pressure::open_review_blocking_prompt(
                    &authoritative,
                    &claim.item_text,
                )
                .is_some_and(
                    |current| current.id == blocker.id && current.evidence == blocker.evidence
                ),
                "external blocker review #{} is awaiting editor authority; report this same generation when it is visible",
                blocker.id,
            );
        }
        let pointer = agent_doc_queue::prompt_pressure::exchange_pointer(&blocker);
        let claim = agent_doc_queue_io::goal_loop_handoff::requeue_with_prompt_pressure(
            file,
            &updated,
            &claim,
            Some(generation),
            &pointer,
            agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review,
        )?;
        return Ok(Some(PromptPressureRequeue {
            claim: claim.into(),
            kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review,
            review_id: Some(blocker.id),
            external_evidence: blocker.evidence,
            incomplete: blocker.incomplete,
            complete_by: blocker.complete_by,
            close_pressure: false,
        }));
    }
    // `#closepressure`: a delivered hand-back and execution evidence still
    // leave the row unfinished when the task required a pull request this
    // turn and the response only cites one that already existed.
    // `#attemptlineage`: a pull request an earlier attempt of this same row
    // opened is the row's own effect, not one that already existed. A
    // recorded receipt or verifiable provenance proves it; a pull request the
    // row never created still requeues.
    if agent_doc_queue::queue_response::hand_back_lacks_required_pull_request(
        &claim.item_text,
        response,
    ) {
        match agent_doc_queue_io::attempt_lineage_effect::earlier_attempt_pull_request_with(
            file, &claim, response, observe,
        )? {
            Some(proof) => {
                agent_doc_queue_io::attempt_lineage_effect::log_accepted(file, &claim, &proof);
            }
            None => {
                let claim = agent_doc_queue_io::goal_loop_handoff::requeue_with_prompt_pressure(
                    file,
                    &current,
                    &claim,
                    Some(generation),
                    response,
                    agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose,
                )?;
                return Ok(Some(PromptPressureRequeue {
                    claim: claim.into(),
                    kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose,
                    review_id: None,
                    external_evidence: None,
                    incomplete: String::new(),
                    complete_by: String::new(),
                    close_pressure: true,
                }));
            }
        }
    }
    if delivered || !agent_doc_queue::prompt_pressure::prose_blocker_needs_pressure(response) {
        reopen_close_pressure_requeue(file, &claim, response)?;
        return Ok(None);
    }
    let claim = agent_doc_queue_io::goal_loop_handoff::requeue_with_prompt_pressure(
        file,
        &current,
        &claim,
        Some(generation),
        response,
        agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose,
    )?;
    Ok(Some(PromptPressureRequeue {
        claim: claim.into(),
        kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose,
        review_id: None,
        external_evidence: None,
        incomplete: String::new(),
        complete_by: String::new(),
        close_pressure: false,
    }))
}

/// `#attemptlineage`: reopen the exact attempt a `#closepressure` verdict
/// requeued once its hand-back passes that gate.
///
/// A prompt-pressure requeue retains the started attempt abandoned
/// (`authorized_running_requeue`), and an abandoned attempt refuses its
/// report. When the recorded hand-off for this generation and worker is a
/// `#closepressure` verdict, that verdict is what abandoned the attempt, and a
/// hand-back that now shows the pull request (created this turn, or proven to
/// be an earlier attempt's) answers it. The coordinator can then report this
/// same generation instead of being left with a row no command closes. Any
/// other requeue (a prose blocker, a plain `release --requeue`) stays
/// abandoned.
fn reopen_close_pressure_requeue(
    file: &Path,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    response: &str,
) -> Result<()> {
    let reportable_loss = claim.state == agent_doc_queue::queue_claim::QueueClaimState::Lost
        || agent_doc_queue::lost_running_recovery::authorized_running_requeue(claim);
    if !reportable_loss {
        return Ok(());
    }
    let Some(worker_id) = claim
        .worker
        .as_ref()
        .map(|worker| worker.worker_id.as_str())
    else {
        return Ok(());
    };
    let handoffs = agent_doc_queue_io::goal_loop_handoff::load(file)?;
    let close_pressure_verdict = handoffs.handoffs.iter().rev().any(|handoff| {
        handoff.identity == claim.identity
            && handoff.generation == claim.generation
            && handoff.worker_id.as_deref() == Some(worker_id)
            && handoff.prompt_pressure
                == Some(agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose)
            && agent_doc_queue::queue_response::hand_back_lacks_required_pull_request(
                &handoff.item_text,
                &handoff.response,
            )
    });
    if !close_pressure_verdict {
        return Ok(());
    }
    if agent_doc_queue_io::queue_claim::reopen_lost_attempt_for_report(
        file,
        &claim.item_text,
        claim.generation,
        worker_id,
        true,
        response,
    )?
    .is_some()
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_close_pressure_reopened owner={} generation={} item_bytes={} (#attemptlineage)",
                claim.owner,
                claim.generation,
                claim.item_text.trim().len(),
            ),
        );
    }
    Ok(())
}

/// `#blockerreportterminal`: the same prompt-blocker hand-back, reported again
/// after it closed its attempt (a retry after a lost exit status). When the
/// hand-off ledger recorded a review hand-back for this exact attempt and the
/// same review still holds the row, the report is already recorded: answer it
/// the same way instead of failing with `#reportcompletenomatch`. An external
/// blocker whose evidence the review no longer carries is from a relinquished
/// attempt and is refused. Anything else keeps the ordinary path, which
/// refuses a report that matches no claim.
fn recorded_prompt_blocker_hand_back(
    file: &Path,
    current: &str,
    item: &str,
    generation: Option<u64>,
    response: &str,
) -> Result<Option<PromptPressureRequeue>> {
    let Some(mut blocker) = agent_doc_queue::prompt_pressure::parse_prompt_blocker(response) else {
        return Ok(None);
    };
    let identity = agent_doc_queue::queue_claim::claim_identity(item);
    let handoffs = agent_doc_queue_io::goal_loop_handoff::load(file)?;
    let Some(handoff) = handoffs.handoffs.iter().rev().find(|handoff| {
        handoff.identity == identity
            && handoff.prompt_pressure
                == Some(agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review)
            && generation.is_none_or(|generation| generation == handoff.generation)
    }) else {
        return Ok(None);
    };
    let external = blocker.evidence.is_some();
    if external {
        blocker.bind_external_hold(&handoff.item_text);
    }
    let held = agent_doc_queue::prompt_pressure::open_review_blocking_prompt(current, item)
        .is_some_and(|hold| hold.id == blocker.id && hold.evidence == blocker.evidence);
    if !held {
        anyhow::ensure!(
            !external,
            "external blocker for {:?} is from a relinquished attempt; current evidence superseded it",
            handoff.item_text
        );
        return Ok(None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_prompt_blocker_already_recorded owner={} generation={} review={} item_bytes={} (#blockerreportterminal)",
            handoff.owner,
            handoff.generation,
            blocker.id,
            handoff.item_text.trim().len(),
        ),
    );
    Ok(Some(PromptPressureRequeue {
        claim: PressuredAttempt {
            item_text: handoff.item_text.clone(),
            owner: handoff.owner.clone(),
            generation: handoff.generation,
        },
        kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review,
        review_id: Some(blocker.id),
        external_evidence: blocker.evidence,
        incomplete: blocker.incomplete,
        complete_by: blocker.complete_by,
        close_pressure: false,
    }))
}

/// `#harnessquotaexhausted`: record a worker that ran out of usage. The
/// harness is the explicit one, else the one the notice names, else the
/// claiming coordinator's: workers run on their coordinator's harness unless
/// told otherwise.
pub(crate) fn block_worker_on_quota(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    limit: &agent_doc_harness::usage_limit::HarnessUsageLimit,
    harness: Option<&str>,
    worktree: Option<&Path>,
    branch: Option<&str>,
) -> Result<agent_doc_queue::queue_claim::QueueClaim> {
    let _lock = acquire_completion_lock(
        file,
        "record a quota-blocked worker",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    let ledger = agent_doc_queue_io::queue_claim::load_ledger(file)?;
    let owner_harness = ledger
        .resolve_selector(item)?
        .and_then(|claim| claim.owner_session.as_ref())
        .and_then(|session| session.harness.clone());
    let harness = harness
        .map(str::to_string)
        .or_else(|| limit.harness.clone())
        .or(owner_harness)
        .map(|name| agent_doc_queue::worker_quota::quota_harness_key(&name))
        .filter(|name| !name.is_empty());
    let block = agent_doc_queue::worker_quota::WorkerQuotaBlock {
        harness,
        limit: limit.limit.clone(),
        evidence: limit.evidence.clone(),
        reset_hint: limit.reset_hint.clone(),
        worktree: worktree.map(|path| path.display().to_string()),
        branch: branch.map(str::to_string),
        blocked_at_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
    };
    agent_doc_queue_io::queue_claim::block_attempt_on_quota(file, item, generation, block)
}

pub(crate) fn report_complete(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    publication_state: Option<QueueReportState>,
    repository: Option<&RepositoryCompletion>,
    response: &str,
) -> Result<CompletionFeedbackOutcome> {
    anyhow::ensure!(
        !response.trim().is_empty(),
        "subagent completion response must not be empty"
    );
    report_complete_with_lock_wait(
        file,
        item,
        generation,
        publication_state,
        repository,
        response,
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )
}

/// `#completionlockload`: every completion-feedback owner waits on the same
/// progress-aware lock, so a long queue of reporters on a loaded machine keeps
/// waiting while owners hand off and fails fast only on a stuck owner.
fn acquire_completion_lock(
    file: &Path,
    action: &str,
    wait: ProgressLockWait,
) -> Result<CompletionFeedbackLock> {
    agent_doc_queue_io::completion_feedback_lock::acquire_completion_feedback_lock_with(
        file,
        action,
        wait,
        |round: &ProgressLockRound| {
            eprintln!(
                "[queue] still waiting to {action} for {}: {} owner handoff(s) in the last window, {} since the wait began ({}ms waited, ceiling {}ms)",
                file.display(),
                round.handoffs,
                round.total_handoffs,
                round.waited.as_millis(),
                wait.max_wait.as_millis(),
            );
        },
    )
}

fn report_complete_with_lock_wait(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    publication_state: Option<QueueReportState>,
    repository: Option<&RepositoryCompletion>,
    response: &str,
    lock_wait: ProgressLockWait,
) -> Result<CompletionFeedbackOutcome> {
    // `#subagentcompletionfeedback`: unlike SESSION_ACTOR_WRITE_QUEUE, this
    // lock serializes independent CLI processes. Re-resolve and rebuild inside
    // the owner so stale full-document successors cannot lose sibling notices.
    let _lock = acquire_completion_lock(file, "publish subagent completion feedback", lock_wait)?;
    // One apply capability spans the response receipt, lineage facts, reported
    // claim transition, completion-cell document successor, and projection
    // ledgers. Acquire it after the completion lock: concurrent reporters must
    // serialize here instead of racing two apply acquisitions and refusing the
    // loser before the completion handoff can make progress. No durable effect
    // occurs before both capabilities are held.
    let apply_guard = agent_doc_queue_io::queue_edit_owner::QueueEditGuard::acquire(file)?;
    let resolved =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    // A retained successor is the latest complete desired cut even while the
    // attached editor has not materialized it. Rebase that successor over the
    // fresh editor cut before composing another report: blindly preferring the
    // retained whole-document target would attribute later operator edits to
    // this completion write (or drop them).
    let current =
        agent_doc_document_realtime_io::deferred_document_write_reconnect_content(file, &resolved)?
            .unwrap_or(resolved);
    // `#reportcompletenomatch`: a report that matches no subagent claim is a
    // failure, never a silent success. The worker result it carried would
    // otherwise be lost while the coordinator reads exit 0 as "recorded".
    let Some(claim) = observed_claim(file, item, &current)? else {
        return Err(no_matching_claim_error(
            file, item, generation, &current, None,
        ));
    };
    if !claim.owner.starts_with("subagent:") {
        return Err(no_matching_claim_error(
            file,
            item,
            generation,
            &current,
            Some(&claim),
        ));
    }
    let structural_context =
        agent_doc_queue_io::queue_claim::bound_attempt_is_structural_context(&current, &claim);
    if structural_context {
        anyhow::ensure!(
            generation == Some(claim.generation),
            "structural parent completion requires the original worker's exact generation {}",
            claim.generation,
        );
    }
    let worker_id = claim
        .worker
        .as_ref()
        .map(|worker| worker.worker_id.clone())
        .context("queue completion requires the exact started worker binding")?;
    // The full response receipt is the monotonic first boundary. If the
    // process stops after this write, reconciliation can reconstruct the cell
    // and retry the exact state transition without consulting agent history.
    if structural_context {
        agent_doc_queue_io::claim_release_receipt::capture_structural_context_response(
            file, &claim, response,
        )?;
    } else {
        agent_doc_queue_io::claim_release_receipt::capture_response(file, &claim, response)?;
    }
    maybe_pause_report_complete_transaction_after_capture()?;
    // Join the result class to this exact claim before it can become
    // Reported. A crash can leave running repository debt, never a prose-only
    // completion that is eligible for closeout. The attempt is (re-)keyed from
    // the live claim first: a claim that followed an admission edit would
    // otherwise find no lineage attempt for its current text.
    agent_doc_queue_io::queue_lineage::record_claim(file, &claim)?;
    if repository.is_none() {
        // `--prose-only` is the worker's explicit, durable declaration
        // (`#reportedcloseoutfacts`); without it an attempt with no
        // repository facts is unknown, never prose.
        agent_doc_queue_io::queue_lineage::record_prose_only(
            file,
            &claim,
            "prose-only result declared by queue report-complete",
        )?;
    }
    if let Some(repository) = repository {
        agent_doc_queue_io::queue_lineage::record(
            file,
            &claim.item_text,
            claim.generation,
            agent_doc_queue_io::queue_lineage::RecordRequest {
                kind: if repository.pushed_ref.is_some() {
                    agent_doc_queue_io::queue_lineage::QueueLineageEventKind::Pushed
                } else {
                    agent_doc_queue_io::queue_lineage::QueueLineageEventKind::Committed
                },
                worker_id: Some(worker_id.clone()),
                source_repo_path: Some(repository.source_repo.clone()),
                sink_repo_path: Some(repository.sink_repo.clone()),
                worktree_path: repository.worktree.clone(),
                branch: repository.branch.clone(),
                base_sha: repository.base_sha.clone(),
                source_sha: Some(repository.source_sha.clone()),
                pushed_ref: repository.pushed_ref.clone(),
                sink_ref: Some(repository.sink_ref.clone()),
                detail: Some("repository result captured by queue report-complete".to_string()),
                ..agent_doc_queue_io::queue_lineage::RecordRequest::default()
            },
        )?;
    }
    let claim =
        agent_doc_queue_io::queue_claim::report_exact_worker_attempt_with_publication_state(
            file,
            &claim.item_text,
            generation.unwrap_or(claim.generation),
            &worker_id,
            publication_state,
        )?;
    if structural_context {
        // The response receipt and repository lineage already attest the
        // superseded attempt. No completion cell, strike or follow-up can be
        // attributed to the new leaf work.
        return Ok(CompletionFeedbackOutcome {
            claim,
            structural_context,
            appended: false,
            replaced: false,
            projection_pending: false,
            intent_id: None,
            followups: Default::default(),
        });
    }
    // Derive the reported marker and completion cell from one cut and publish
    // one successor. Two separate retained editor writes can conflict while
    // the first is still awaiting projection.
    let projection =
        agent_doc_queue_io::queue_cmd::derive_reported_claim_runtime_projection(file, &current)?;
    let (appended, replaced, intent_id, followups) =
        upsert_completion_feedback(file, &current, &projection.target_content, &claim, response)?;
    agent_doc_queue_io::queue_cmd::record_claim_runtime_projection_under_apply_guard(
        file,
        &projection,
        None,
        &apply_guard,
    )?;
    if !followups.inserted.is_empty() {
        // `#capturegatedispatched`: queued follow-ups are tracked work. The
        // rows already landed; a failed mark only leaves the gate armed.
        if let Err(err) = agent_doc_cycle_state_io::mark_queue_followups_added(file) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!("queue_followup_capture_mark_failed source=report_complete error={err:#}"),
            );
        }
    }
    Ok(CompletionFeedbackOutcome {
        claim,
        structural_context,
        appended,
        replaced,
        projection_pending: intent_id.is_some(),
        intent_id,
        followups,
    })
}

fn maybe_pause_report_complete_transaction_after_capture() -> Result<()> {
    let Ok(marker) = std::env::var("AGENT_DOC_TEST_REPORT_COMPLETE_AFTER_CAPTURE_MARKER") else {
        return Ok(());
    };
    let release = std::env::var("AGENT_DOC_TEST_REPORT_COMPLETE_AFTER_CAPTURE_RELEASE")
        .context("report-complete test barrier requires a release path")?;
    std::fs::write(&marker, b"ready")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !Path::new(&release).exists() {
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "timed out waiting for report-complete transaction test barrier release"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}

/// `#workerresponsestructure`: the report line's account of the hand-back's
/// items — action items queued as follow-up rows, already queued, or refused
/// with the exact `queue follow-up` command the coordinator runs instead, and
/// open items held for triage (`#handbackopenitemstriage`). Empty when the
/// hand-back lists neither.
///
/// `#handbackrefusedcommandexact`: the refused-placement command is its own
/// line. `--after` and each `--item` are the real origin and unplaced row
/// texts, shell-quoted, because those rows quote an id in backticks and a
/// surrounding code span would cut the command. The line can be run as printed.
pub(crate) fn describe_handback_followups(
    file: &Path,
    origin: &str,
    followups: &agent_doc_queue::handback_followups::HandbackFollowups,
) -> String {
    let queue = followups.queue_id.as_deref().unwrap_or("<queue-id>");
    let mut text = String::new();
    if !followups.inserted.is_empty() {
        text.push_str(&format!(
            " Queued {} action item(s) as follow-up row(s) after {origin:?} in queue id `{queue}`; do not add them again.",
            followups.inserted.len(),
        ));
        // `#humanfollowupowner`: say which rows no agent will dispatch.
        if followups.operator_owned > 0 {
            text.push_str(&format!(
                " {} of them are human-owned: they carry `[operator-verify]` and wait on the operator (⏳), so no agent dispatches them; the operator strikes each once done or answers it inline to hand the rest back.",
                followups.operator_owned,
            ));
        }
    }
    if !followups.self_repair_items.is_empty() {
        text.push_str(&format!(
            " {} operator cleanup item(s) about agent-doc's own document artifacts were NOT queued as operator rows: agent-doc repairs them itself on the next preflight or `queue sync` (#selfrepaircleanup); do not queue them.",
            followups.self_repair_items.len(),
        ));
    }
    if followups.already_present > 0 {
        text.push_str(&format!(
            " {} action item(s) already have a follow-up row in queue id `{queue}`.",
            followups.already_present,
        ));
    }
    if let Some(reason) = &followups.refused {
        if followups.refused_items.is_empty() {
            text.push_str(&format!(
                " The hand-back's action items were NOT queued ({reason})."
            ));
        } else {
            let command = agent_doc_queue::handback_followups::refused_placement_command(
                &file.display().to_string(),
                queue,
                followups.refused_origin.as_deref().unwrap_or(origin),
                &followups.refused_items,
            );
            text.push_str(&format!(
                " The hand-back's action items were NOT queued ({reason}); run this exact command as printed:\n{command}\n"
            ));
        }
    }
    if followups.held_for_triage > 0 {
        text.push_str(&format!(
            " {} open item(s) were held for triage and NOT queued, because Open items does not say which entries are work: read them in the response, and queue only the work with `agent-doc queue follow-up {} --id {queue} --after <origin> --item <text>...`.",
            followups.held_for_triage,
            file.display(),
        ));
    }
    text.push_str(&finish_agent_action_items_directive(file, followups));
    text
}

/// `#finishactionable`: the agent-owned action items of this hand-back are
/// work for this turn, not parked follow-ups.
///
/// A worker lists an `agent:` action item for work an agent can do but the
/// worker did not (an install the coordinator rules reserve for the
/// coordinator, a CI run still in progress at hand-back, a small follow-up
/// fix). The queued row tracks it, but a row that waits for a later turn
/// makes the operator ask for "complete the work" on each one. So the report
/// line tells the coordinator to do each of them now, under the originating
/// item's authorization, and to strike each row once its result is verified.
/// Only operator-owned rows (`[operator-verify]`) and a concrete blocker
/// leave work for later. Empty when the hand-back has no agent-owned item.
pub(crate) fn finish_agent_action_items_directive(
    file: &Path,
    followups: &agent_doc_queue::handback_followups::HandbackFollowups,
) -> String {
    let rows = followups
        .inserted
        .iter()
        .chain(followups.refused_items.iter())
        .filter(|row| agent_doc_queue::followup_owner::is_agent_followup_row(row))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return String::new();
    }
    let mut text = format!(
        "\nDo these {} agent-owned action item(s) NOW, in this turn, before closeout (#finishactionable): they are agent work under the originating item's authorization (its queue item and resolved presets), not operator questions and not work for a later turn. Do each one inline, or dispatch its row in this same turn when it is a substantial new task; verify its result, then strike its row with `agent-doc queue consume {} --ack-text '<row>' --summary '<what was done, and where>'` so it carries its response tree (#ackresponse). Leave a row open only with its concrete blocker recorded on it (an authorization the item does not grant, an external change that has not happened). Do not end the turn, and do not write \"More work remains\" for them, while one of these rows is open:",
        rows.len(),
        file.display(),
    );
    for row in rows {
        text.push_str("\n- ");
        text.push_str(row);
    }
    text.push('\n');
    text
}

/// Most rows/claims named in a no-match refusal; the rest are counted.
const NO_MATCH_LIST_LIMIT: usize = 20;

/// `#reportcompletenomatch`: the refusal for a `report-complete` whose item
/// resolves to no subagent claim. It names what the item could have meant —
/// the document's queue rows and the ledger's claims with owner, generation,
/// and state — so the coordinator can re-run against the exact attempt
/// instead of losing the worker result.
fn no_matching_claim_error(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    current: &str,
    non_subagent: Option<&QueueClaim>,
) -> anyhow::Error {
    let requested_generation = generation
        .map(|generation| format!(" generation {generation}"))
        .unwrap_or_default();
    let reason = match non_subagent {
        Some(claim) => format!(
            "the claim on {:?} is held by {} generation {}, not a `subagent:` owner",
            claim.item_text, claim.owner, claim.generation
        ),
        None => "no claim in the ledger matches it".to_string(),
    };
    let rows =
        agent_doc_queue::queue_continuation::all_queue_head_texts(current).unwrap_or_default();
    let ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, current);
    let mut message = format!(
        "queue report-complete matched no subagent claim for {item:?}{requested_generation} in {}: {reason}. No completion feedback was written; the worker result is NOT recorded (#reportcompletenomatch).",
        file.display(),
    );
    push_named_list(
        &mut message,
        "candidate rows",
        rows.iter().map(|row| format!("{row:?}")),
    );
    match &ledger {
        Ok(ledger) => {
            push_named_list(
                &mut message,
                "claims",
                ledger.claims.iter().map(describe_claim),
            );
            let identity = agent_doc_queue::queue_claim::claim_identity(item);
            push_named_list(
                &mut message,
                "cancelled attempts for this item",
                ledger
                    .cancellations
                    .iter()
                    .filter(|receipt| receipt.claim.identity == identity)
                    .map(|receipt| describe_claim(&receipt.claim)),
            );
        }
        Err(err) => message.push_str(&format!("\nclaims: unreadable ({err:#})")),
    }
    message.push_str(
        "\nNext: re-run `agent-doc queue report-complete` with --item set to the exact claimed row text and --generation from the claims above; if no claim fits, the attempt was released, cancelled, or never claimed, so keep the worker response and re-claim the row before reporting.",
    );
    anyhow::anyhow!(message)
}

fn describe_claim(claim: &QueueClaim) -> String {
    format!(
        "{:?} owner={} generation={} state={}{}",
        claim.item_text,
        claim.owner,
        claim.generation,
        format!("{:?}", claim.state).to_lowercase(),
        claim
            .worker
            .as_ref()
            .map(|worker| format!(" worker={}", worker.worker_id))
            .unwrap_or_default(),
    )
}

fn push_named_list(message: &mut String, label: &str, entries: impl Iterator<Item = String>) {
    let entries: Vec<String> = entries.collect();
    if entries.is_empty() {
        message.push_str(&format!("\n{label}: none"));
        return;
    }
    message.push_str(&format!("\n{label} ({}):", entries.len()));
    for entry in entries.iter().take(NO_MATCH_LIST_LIMIT) {
        message.push_str(&format!("\n  - {entry}"));
    }
    if entries.len() > NO_MATCH_LIST_LIMIT {
        message.push_str(&format!(
            "\n  - ... and {} more",
            entries.len() - NO_MATCH_LIST_LIMIT
        ));
    }
}

fn observed_claim(file: &Path, item: &str, current: &str) -> Result<Option<QueueClaim>> {
    let ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, current)?;
    // Use the same pure identity/continuation matching as release without
    // mutating the durable ledger; an id selector (`lazilyreleasedecision`,
    // `#id`, `do [#id]`) also resolves a row claimed by its full text, and an
    // id carried by several claims refuses with the candidates
    // (`#reportcompleteselector`).
    Ok(ledger.resolve_selector(item)?.cloned())
}

fn upsert_completion_feedback(
    file: &Path,
    current: &str,
    projected: &str,
    claim: &QueueClaim,
    response: &str,
) -> Result<(
    bool,
    bool,
    Option<String>,
    agent_doc_queue::handback_followups::HandbackFollowups,
)> {
    let key = agent_doc_queue_io::queue_claim::completion_feedback_key(claim)?;
    // Keep the heading, attested worker response, and marker contiguous. The existing N-1 diff
    // classifier treats a block beginning with `### Re:` as a recovery
    // artifact, while the structural/session-check gate requires substantive
    // body text after every response heading. An unquoted blank line here would split
    // the old controller's annotated diff block and make the body look like a
    // new operator prompt during a rolling upgrade. The contiguous
    // heading+body+marker shape is also what lets recovery recognize the cell
    // as binary-owned feedback rather than an agent response.
    //
    // The heading's topic names the worker and item (`### Re: topic —
    // agent-doc`), so several cells never collapse to one replay topic.
    let note = agent_doc_document::subagent_completion::cell_with_response(
        &claim.owner,
        &claim.item_text,
        &key,
        response,
    )?;
    let replacement = agent_doc_document::subagent_completion::replace_attested_completion_cell(
        projected, &key, &note,
    )?;
    let appended = replacement.is_none();
    let with_cell = match replacement {
        Some(updated) => updated,
        None => agent_doc_element_exchange::insert_prompt_line_before_boundary(projected, &note)?,
    };
    // `#workerresponsestructure`: each action item the hand-back reports
    // becomes a follow-up row after the originating head, in the same
    // successor as the cell, so the work is queued rather than buried prose.
    // `#followupshorttitle`: an id-less origin gets its `[#q-…]` in this same
    // write, so every row names its origin by id.
    let (updated, followups) = agent_doc_queue::handback_followups::plan_handback_followups_in(
        &with_cell,
        &agent_doc_hash::document_id_for_path(file),
        &claim.item_text,
        response,
    );
    // `#followupidatcreate`: each new row carries its `[#q-…]` id from the
    // write that creates it, and id-less operator follow-ups are backfilled,
    // so a recap can name them before any preflight runs (`#recapids`).
    let admitted = agent_doc_queue::prompt_identity::admit_followup_row_ids(
        &updated,
        &agent_doc_hash::document_id_for_path(file),
        &followups.inserted,
    );
    let updated = admitted.content;
    for item in &followups.self_repair_items {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_self_repair_cleanup_item_absorbed owner={} item=\"{}\" (#selfrepaircleanup)",
                claim.owner,
                item.chars()
                    .take(160)
                    .map(|c| if c == '"' || c == '\n' { '\'' } else { c })
                    .collect::<String>(),
            ),
        );
    }
    let mut followups = followups;
    // The report names each row as it now reads, id included, so the
    // coordinator's `--ack-text` and recap quote the row the operator sees.
    for row in &mut followups.inserted {
        if let Some(assignment) = admitted
            .assignments
            .iter()
            .find(|assignment| assignment.source_text == row.trim())
        {
            *row = assignment.target_text.clone();
        }
    }
    let replaced = !appended && with_cell != projected;
    if updated == current {
        let intent_id = agent_doc_document_realtime_io::pending_document_write(file)
            .filter(|pending| pending.target_content == current)
            .map(|pending| pending.intent_id);
        return Ok((false, false, intent_id, followups));
    }
    let scope = changed_component_scope(current, &updated)
        .context("subagent completion feedback requires a component-body-only write")?;
    let scope_pairs = scope.to_pairs();
    if (appended && !scope.contains("exchange", 0))
        || scope_pairs
            .iter()
            .any(|(name, _)| !matches!(name.as_str(), "exchange" | "queue" | "status" | "active"))
    {
        bail!("subagent completion feedback escaped its queue lifecycle components");
    }
    // An attached editor owns disk projection. Hand the target to the durable
    // retained-intent graph and return as soon as the controller accepts it;
    // its reactive projector applies the target without keeping this one-shot
    // CLI process alive through typing/native-save backpressure. Detached files
    // still use the ordinary synchronous CAS write.
    let retained_intent = publish_exchange_update(file, current, &updated, scope)?;
    // `#followuprootprovenance`: the rows serve the origin's root tracked
    // item (its id, or the root a follow-up origin already carries), so the
    // backlog mirror counts that item as queued while any of them is live.
    let origin_row =
        agent_doc_queue::queue_response::queue_row_text_for_head(&with_cell, &claim.item_text);
    agent_doc_queue_io::followup_provenance_ledger::record_followup_roots(
        file,
        &updated,
        &[
            origin_row.as_deref().unwrap_or(&claim.item_text),
            &claim.item_text,
        ],
        &followups.inserted,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "subagent_completion_feedback_accepted owner={} claimed_at={} key={} item_bytes={} followups={} followups_present={} delivery={}{}",
            claim.owner,
            claim.claimed_at_secs,
            key,
            claim.item_text.len(),
            followups.inserted.len(),
            followups.already_present,
            if retained_intent.is_some() {
                "retained_controller_projection"
            } else {
                "detached_materialized"
            },
            retained_intent
                .as_deref()
                .map(|intent| format!(" intent_id={intent}"))
                .unwrap_or_default(),
        ),
    );
    Ok((appended, replaced, retained_intent, followups))
}

/// Complete mechanical `☑️` closeouts now.
///
/// Prose, an already verified integration, and a sink that already contains
/// the source are verified, recorded, and consumed after the saved response is
/// in the document. A row whose visible marker has drifted is repaired once,
/// then retried. A merge, cherry-pick, push, missing fact, operator decision,
/// or a response that was never captured stays reported. One refusal does not
/// fail the pass: the caller reads each [`MechanicalCloseoutOutcome`].
pub(crate) fn close_mechanical_reported_rows(
    file: &Path,
    effects: &impl agent_doc_queue_io::queue_cmd::QueueCommandEffects,
) -> Result<Vec<agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutOutcome>> {
    use agent_doc_queue_io::reported_closeout_exec::{
        MechanicalCloseoutResult, close_mechanical_reported_attempts,
    };

    let close_pass = || {
        // Mechanical closeout takes the same queue apply guard as report
        // publication. Serialize both paths so another reporter cannot take
        // the completion lock and then run into the preceding report's
        // direct apply lease. Projection repair acquires the lock on its own.
        let _lock = acquire_completion_lock(
            file,
            "close reported subagent attempts",
            COMPLETION_FEEDBACK_LOCK_WAIT,
        )?;
        close_mechanical_reported_attempts(effects, file)
    };
    let first = close_pass()?;
    let outcomes = if first
        .outcomes
        .iter()
        .any(|outcome| outcome.projection_out_of_sync())
    {
        match repair_claim_projection(file, effects) {
            Ok(_) => {
                let second = close_pass()?;
                merge_closeout_passes(first.outcomes, second.outcomes)
            }
            Err(err) => {
                let reason = format!("projection repair refused: {err:#}").replace('\n', " ");
                first
                    .outcomes
                    .into_iter()
                    .map(|mut outcome| {
                        if outcome.projection_out_of_sync() {
                            outcome.result = MechanicalCloseoutResult::LeftReported {
                                reason: reason.clone(),
                            };
                        }
                        outcome
                    })
                    .collect()
            }
        }
    } else {
        first.outcomes
    };
    for outcome in &outcomes {
        let status = match &outcome.result {
            MechanicalCloseoutResult::Closed => "closed".to_string(),
            MechanicalCloseoutResult::ProjectionOutOfSync => "projection_out_of_sync".to_string(),
            MechanicalCloseoutResult::LeftReported { reason } => {
                format!("left_reported reason={reason}")
            }
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "mechanical_reported_closeout file={} owner={} generation={} item_bytes={} {status}",
                file.display(),
                outcome.owner,
                outcome.generation,
                outcome.item.len(),
            ),
        );
    }
    Ok(outcomes)
}

fn merge_closeout_passes(
    first: Vec<agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutOutcome>,
    second: Vec<agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutOutcome>,
) -> Vec<agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutOutcome> {
    let mut merged: Vec<_> = first
        .into_iter()
        .filter(|outcome| outcome.closed())
        .collect();
    for outcome in second {
        let already = merged.iter().any(|existing| {
            existing.owner == outcome.owner && existing.generation == outcome.generation
        });
        if !already {
            merged.push(outcome);
        }
    }
    merged
}

pub(crate) fn print_mechanical_closeout(
    file: &Path,
    outcomes: &[agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutOutcome],
) {
    use agent_doc_queue_io::reported_closeout_exec::MechanicalCloseoutResult;

    if outcomes.is_empty() {
        eprintln!("[queue] no reported rows to close for {}", file.display());
        return;
    }
    let closed = outcomes.iter().filter(|outcome| outcome.closed()).count();
    eprintln!(
        "[queue] mechanical reported closeout for {}: {closed} closed, {} still reported",
        file.display(),
        outcomes.len().saturating_sub(closed),
    );
    for outcome in outcomes {
        let handle = agent_doc_queue::queue_claim::claim_item_handle(&outcome.item);
        match &outcome.result {
            MechanicalCloseoutResult::Closed => eprintln!(
                "[queue] closed {handle} ({}#{})",
                outcome.owner, outcome.generation
            ),
            MechanicalCloseoutResult::LeftReported { reason } => eprintln!(
                "[queue] still reported {handle} ({}#{}): {reason}",
                outcome.owner, outcome.generation
            ),
            MechanicalCloseoutResult::ProjectionOutOfSync => eprintln!(
                "[queue] still reported {handle} ({}#{}): visible marker does not match the claim",
                outcome.owner, outcome.generation
            ),
        }
    }
}

/// Settle any retained completion successor, then rederive the queue marker
/// from the durable claim ledger. This never releases a claim or writes behind
/// an editor-owned buffer.
pub(crate) fn repair_claim_projection(
    file: &Path,
    effects: &impl agent_doc_queue_io::queue_cmd::QueueCommandEffects,
) -> Result<bool> {
    let _lock = acquire_completion_lock(
        file,
        "repair queue projection",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )
    .map_err(|err| {
        anyhow::anyhow!("{err:#}; retry `agent-doc queue repair-projection {file:?}`")
    })?;

    if agent_doc_document_realtime_io::pending_document_write(file).is_some() {
        // Land a retained write when the editor can accept it. A missing
        // native-save receipt must not defer the marker reconcile
        // (`#markdrift`). The generation write below CAS-rebases onto current
        // authority and returns without waiting for that receipt and without
        // `--force-disk`.
        agent_doc_document_realtime_io::recover_retained_document_write_before_new_cycle(
            file,
            agent_doc_document_realtime_io::RetainedWriteCycleBoundary::QueueProjectionRepair,
        )?;
    }

    agent_doc_queue_io::queue_cmd::project_claim_runtime_state_with_effects(effects, file, None)
}

/// Repair the short-lived v0.34 rolling-upgrade artifact that put all visible
/// status in a `### Re:` heading and only an HTML marker beneath it. The
/// integrity gate correctly ignores comments as response bodies, so those
/// artifacts strand session-check even though the closeout itself committed.
///
/// This migration rewrites only the exact binary-owned heading+marker pair. It
/// neither replays a response nor changes cycle/queue state.
pub(crate) fn repair_legacy_completion_feedback(
    file: &Path,
) -> Result<CompletionFeedbackRepairOutcome> {
    let _lock = acquire_completion_lock(
        file,
        "repair subagent completion feedback",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let (updated, repaired) = repair_legacy_completion_feedback_content(&current);
    if repaired == 0 {
        return Ok(CompletionFeedbackRepairOutcome {
            repaired,
            accepted: false,
        });
    }
    let scope = changed_component_scope(&current, &updated)
        .context("completion feedback repair must change only agent:exchange")?;
    if !scope.contains("exchange", 0) || scope.components().len() != 1 {
        bail!("completion feedback repair escaped the first agent:exchange component");
    }
    let retained_intent = publish_exchange_update(file, &current, &updated, scope)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "subagent_completion_feedback_repair_accepted repaired={} delivery={}{}",
            repaired,
            if retained_intent.is_some() {
                "retained_controller_projection"
            } else {
                "detached_materialized"
            },
            retained_intent
                .as_deref()
                .map(|intent| format!(" intent_id={intent}"))
                .unwrap_or_default(),
        ),
    );
    Ok(CompletionFeedbackRepairOutcome {
        repaired,
        accepted: true,
    })
}

/// `#stalecompletionrepair`: serialize with `report-complete` (whose cells
/// this edits) and refuse while a retained editor write is still unsettled, so
/// the successor is never composed over a cut the editor has not materialized.
pub(crate) fn repair_stale_completion(
    file: &Path,
    apply: bool,
    keys: &std::collections::HashSet<String>,
) -> Result<agent_doc_queue_io::stale_completion_repair::StaleCompletionRepairReport> {
    if !apply {
        return agent_doc_queue_io::stale_completion_repair::repair(
            file,
            false,
            keys,
            &crate::CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        );
    }
    let _lock = acquire_completion_lock(
        file,
        "repair stale completion evidence",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file) {
        bail!(
            "{} has a retained editor write (intent {}) that has not landed; open it in the active editor/project and run `agent-doc queue repair-projection {:?}`, then rerun; nothing was changed",
            file.display(),
            pending.intent_id,
            file,
        );
    }
    agent_doc_queue_io::stale_completion_repair::repair(
        file,
        true,
        keys,
        &crate::CLI_QUEUE_CONSUME_WRITE_EFFECTS,
    )
}

/// `#completioncellrestore`: rebuild the completion cells a closeout deleted,
/// from the worker responses saved in the attempts' receipts, plus the closed
/// attempts named in `keys`. Dry run unless `apply`. `--apply` serializes with
/// `report-complete` (whose cells this adds), refuses while a retained editor
/// write is unsettled, may change only `agent:exchange`, and publishes through
/// the same editor/CRDT path as `report-complete`. Idempotent.
pub(crate) fn restore_completion_cells(
    file: &Path,
    apply: bool,
    keys: &std::collections::HashSet<String>,
) -> Result<agent_doc_queue_io::completion_cell_restore::CompletionCellRestoreReport> {
    use agent_doc_queue_io::completion_cell_restore::{compose, inspect};
    const RESTORE_SOURCE: &str = "queue_repair_completion_cells";
    if !apply {
        let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            RESTORE_SOURCE,
        )?;
        return inspect(file, &current, keys);
    }
    let _lock = acquire_completion_lock(
        file,
        "restore subagent completion cells",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file) {
        bail!(
            "{} has a retained editor write (intent {}) that has not landed; open it in the active editor/project and run `agent-doc queue repair-projection {:?}`, then rerun; nothing was changed",
            file.display(),
            pending.intent_id,
            file,
        );
    }
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, RESTORE_SOURCE)?;
    let mut report = inspect(file, &current, keys)?;
    if report.is_clean() {
        return Ok(report);
    }
    let updated = compose(&current, &report)?;
    let scope = changed_component_scope(&current, &updated)
        .context("restored completion cells must change only agent:exchange")?;
    if !scope.contains("exchange", 0) || scope.components().len() != 1 {
        bail!("restored completion cells escaped the first agent:exchange component");
    }
    let retained_intent = publish_exchange_update(file, &current, &updated, scope)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_completion_cells_restored file={} cells={} keys={} present={} skipped={} delivery={}{} (#completioncellrestore)",
            file.display(),
            report.restored.len(),
            report
                .restored
                .iter()
                .map(|cell| cell.key.as_str())
                .collect::<Vec<_>>()
                .join(","),
            report.present,
            report.skipped.len(),
            if retained_intent.is_some() {
                "retained_controller_projection"
            } else {
                "detached_materialized"
            },
            retained_intent
                .as_deref()
                .map(|intent| format!(" intent_id={intent}"))
                .unwrap_or_default(),
        ),
    );
    report.applied = true;
    report.intent_id = retained_intent;
    Ok(report)
}

pub(crate) fn print_completion_cell_restore_report(
    report: &agent_doc_queue_io::completion_cell_restore::CompletionCellRestoreReport,
) {
    use agent_doc_queue_io::stale_completion_repair::one_line;
    let verb = if report.applied {
        "restored"
    } else {
        "would restore"
    };
    for cell in &report.restored {
        println!(
            "{verb} completion cell key={} owner={} generation={} item=\"{}\"",
            cell.key,
            cell.owner,
            cell.generation,
            one_line(&cell.item, 100)
        );
    }
    for skipped in &report.skipped {
        println!(
            "skipped saved response owner={} generation={} key={} item=\"{}\": {}",
            skipped.owner,
            skipped.generation,
            skipped.key.as_deref().unwrap_or("<none>"),
            one_line(&skipped.item, 100),
            skipped.reason
        );
    }
    for key in &report.missing_keys {
        println!("--key {key}: no receipt carries it");
    }
    eprintln!(
        "[queue] {}: {} completion cell(s) {}; {} already present, {} saved response(s) skipped{}",
        report.file.display(),
        report.restored.len(),
        if report.applied {
            "restored"
        } else if report.is_clean() {
            "to restore"
        } else {
            "to restore (dry run; rerun with --apply to restore)"
        },
        report.present,
        report.skipped.len(),
        report
            .intent_id
            .as_deref()
            .map(|intent| format!("; retained editor intent {intent}"))
            .unwrap_or_default(),
    );
}

/// Combined, evidence-based audit/repair for visible managed queue state.
///
/// The stale-evidence planner proves which completion artifacts no longer
/// belong to live or claimed work. Runtime projection then derives every
/// visible lifecycle marker from the remaining durable claim/receipt/cell
/// evidence. Neither phase treats elapsed time as proof of completion.
#[derive(Debug)]
pub(crate) struct ManagedStateRepairReport {
    pub stale: agent_doc_queue_io::stale_completion_repair::StaleCompletionRepairReport,
    pub projection_drift: bool,
    pub projection_applied: bool,
    /// `#managedlistnodrift`: the tracked-list and todo reconcile preflight
    /// runs, run here through the same functions.
    pub lists: ManagedListRepair,
}

/// The tracked-list (`agent:backlog`/`review`/`icebox`) and `agent:todo`
/// reconcile of one repair run.
#[derive(Debug, Default)]
pub(crate) struct ManagedListRepair {
    pub actions: Vec<String>,
    pub drift: bool,
    pub applied: bool,
}

/// Render the tracked lists and the todo list from their durable state, the
/// same projections preflight applies.
fn project_managed_lists(file: &Path, current: &str) -> Result<(Option<String>, Vec<String>)> {
    let mut actions = Vec::new();
    let tracked = agent_doc_work_projection::tracked_lists::project_tracked_lists(current)?;
    actions.extend(tracked.actions.iter().map(ToString::to_string));
    let after_tracked = tracked.content.unwrap_or_else(|| current.to_string());
    let links = agent_doc_element_backlog_io::backlog_cmd::todo_link_index_in_content(
        file,
        &after_tracked,
        true,
        &[],
    )?;
    let todo = agent_doc_element::todo::project_todo(&after_tracked, &links)?;
    actions.extend(
        todo.completed
            .iter()
            .map(|id| format!("agent:todo: closed [#{id}] (its tracked work is resolved)")),
    );
    actions.extend(todo.duplicate_ids.iter().map(|(id, rows)| {
        agent_doc_work_projection::managed_list::ReconcileAction::DuplicateId {
            list: agent_doc_work_projection::managed_list::ManagedList::Todo,
            id: id.clone(),
            rows: *rows,
        }
        .to_string()
    }));
    let after_todo = todo.content.unwrap_or(after_tracked);
    let worktrees = agent_doc_queue_io::worktree_inventory::reconcile(file, &after_todo)?;
    if worktrees.is_some() {
        actions.push("agent:worktrees: re-projected the live worktree inventory".to_string());
    }
    let target = worktrees.unwrap_or(after_todo);
    Ok(((target != current).then_some(target), actions))
}

pub(crate) fn repair_managed_state(
    file: &Path,
    apply: bool,
    keys: &std::collections::HashSet<String>,
) -> Result<ManagedStateRepairReport> {
    let _lock = acquire_completion_lock(
        file,
        "audit managed queue state",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;

    if apply && let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file) {
        bail!(
            "{} has a retained editor write (intent {}) that has not landed; open it in the active editor/project and run `agent-doc queue repair-projection {:?}`, then rerun; nothing was changed",
            file.display(),
            pending.intent_id,
            file,
        );
    }

    let stale = agent_doc_queue_io::stale_completion_repair::repair(
        file,
        apply,
        keys,
        &crate::CLI_QUEUE_CONSUME_WRITE_EFFECTS,
    )?;
    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "queue_repair_managed_state",
    )?;
    let preview_keys = stale
        .stale_cells
        .iter()
        .map(|cell| cell.key.clone())
        .collect::<std::collections::HashSet<_>>();
    let projected_from = if apply {
        current.clone()
    } else {
        agent_doc_queue::stale_completion_evidence::retire_cells_by_key(&current, &preview_keys)
    };
    let projection = agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection(
        file,
        &projected_from,
        None,
    )?;
    let projection_drift = projection.changed;
    let projection_applied = if apply {
        agent_doc_queue_io::queue_cmd::project_claim_runtime_state_with_effects(
            &crate::CliQueueCommandEffects,
            file,
            None,
        )?
    } else {
        false
    };

    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "queue_repair_managed_lists",
    )?;
    let worktrees = if apply {
        agent_doc_queue_io::worktree_inventory::maintain(file, &current)?
    } else {
        agent_doc_queue_io::worktree_inventory::inspect(file, &current)?
    };
    let (target, mut actions) = project_managed_lists(file, &current)?;
    let verb = if apply { "removed" } else { "would remove" };
    for removal in &worktrees.removed {
        actions.push(format!(
            "agent:worktrees: {verb} `{}` ({})",
            removal.directory, removal.reason
        ));
    }
    for refusal in &worktrees.refused {
        actions.push(format!(
            "agent:worktrees: left `{}` in place ({})",
            refusal.directory, refusal.reason
        ));
    }
    // `#managedlistnodrift`: name every `☑️` that no durable record backs.
    actions.extend(
        agent_doc_queue_io::queue_marker_audit::unbacked_reported_rows(file, &current)?
            .iter()
            .map(ToString::to_string),
    );
    let mut lists = ManagedListRepair {
        drift: target.is_some(),
        actions,
        applied: false,
    };
    if apply && let Some(target) = target {
        use agent_doc_queue_io::queue_consume::QueueConsumeWriteEffects as _;
        crate::CLI_QUEUE_CONSUME_WRITE_EFFECTS.converge_document_or_disk(
            file,
            &target,
            &current,
            "queue_repair_managed_lists",
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_repair_managed_lists_applied file={} actions={} (#managedlistnodrift)",
                file.display(),
                lists.actions.len()
            ),
        );
        lists.applied = true;
    }

    Ok(ManagedStateRepairReport {
        stale,
        projection_drift,
        projection_applied,
        lists,
    })
}

pub(crate) fn print_managed_state_report(report: &ManagedStateRepairReport) {
    print_stale_completion_report(&report.stale);
    match (report.projection_drift, report.projection_applied) {
        (_, true) => println!(
            "repaired lifecycle projection from durable evidence in {}",
            report.stale.file.display()
        ),
        (true, false) => println!(
            "would repair lifecycle projection from durable evidence in {} (dry run; rerun with --apply)",
            report.stale.file.display()
        ),
        (false, false) => eprintln!(
            "[queue] {}: lifecycle projection already synchronized",
            report.stale.file.display()
        ),
    }
    for action in &report.lists.actions {
        println!("{}: {action}", report.stale.file.display());
    }
    match (report.lists.drift, report.lists.applied) {
        (_, true) => println!(
            "repaired tracked lists and agent:todo from durable state in {}",
            report.stale.file.display()
        ),
        (true, false) => println!(
            "would repair tracked lists and agent:todo from durable state in {} (dry run; rerun with --apply)",
            report.stale.file.display()
        ),
        (false, false) => eprintln!(
            "[queue] {}: tracked lists and agent:todo already synchronized",
            report.stale.file.display()
        ),
    }
}

pub(crate) fn print_stale_completion_report(
    report: &agent_doc_queue_io::stale_completion_repair::StaleCompletionRepairReport,
) {
    use agent_doc_queue_io::stale_completion_repair::one_line;
    let verb = if report.applied {
        "retired"
    } else {
        "would retire"
    };
    for cell in &report.stale_cells {
        println!(
            "{verb} completion cell key={} owner={} item=\"{}\"",
            cell.key,
            cell.owner,
            one_line(&cell.item, 100)
        );
    }
    for receipt in &report.stale_receipts {
        println!(
            "{verb} release receipt owner={} generation={} item=\"{}\"",
            receipt.owner,
            receipt.generation,
            one_line(&receipt.item_text, 100)
        );
    }
    for cell in &report.untracked_live_cells {
        println!(
            "kept untracked live-row completion cell key={} owner={} item=\"{}\" (its row is live but no claim or receipt tracks this attempt; if the row is a NEW request, retire it with --key {})",
            cell.key,
            cell.owner,
            one_line(&cell.item, 100),
            cell.key
        );
    }
    for key in &report.missing_keys {
        println!("--key {key}: no completion cell carries it (already retired?)");
    }
    eprintln!(
        "[queue] {}: {} stale completion cell(s), {} stale receipt(s) {}; kept {} live-row cell(s), {} untracked live-row cell(s), {} claimed cell(s){}",
        report.file.display(),
        report.stale_cells.len(),
        report.stale_receipts.len(),
        if report.applied {
            "retired"
        } else if report.is_clean() {
            "found"
        } else {
            "found (dry run; rerun with --apply to retire)"
        },
        report.kept_live_cells,
        report.untracked_live_cells.len(),
        report.kept_claimed_cells,
        if report.applied {
            format!("; snapshot cells retired {}", report.snapshot_cells_retired)
        } else {
            String::new()
        },
    );
}

pub(crate) fn print_stale_completion_scan(root: &Path) {
    let scan = agent_doc_queue_io::stale_completion_repair::scan_project(root);
    let mut stale_documents = 0;
    for (file, report) in &scan {
        match report {
            Ok(report) if report.is_clean() => {}
            Ok(report) => {
                stale_documents += 1;
                println!(
                    "{}: {} stale completion cell(s), {} stale receipt(s)",
                    file.display(),
                    report.stale_cells.len(),
                    report.stale_receipts.len()
                );
            }
            Err(err) => println!("{}: not judged: {err:#}", file.display()),
        }
    }
    eprintln!(
        "[queue] {stale_documents} session document(s) under {} carry stale completion evidence (read from disk; nothing changed). Repair each with `agent-doc queue repair-stale-completion <FILE> --apply`.",
        root.display()
    );
}

fn publish_document_update(file: &Path, current: &str, updated: &str) -> Result<Option<String>> {
    // GH #40 / `#ghrecvpolicy`: hoisting a prompt blocker may create
    // `agent:review`; that follows the recipient's `[receive]` policy for
    // non-owner senders.
    agent_doc_project_config_io::workspace_access::enforce_sender_created_components(
        file, current, updated,
    )?;
    match changed_component_scope(current, updated) {
        Some(scope) => publish_exchange_update(file, current, updated, scope),
        None => {
            // A new `agent:review` changes the component count, so the write
            // is not a body edit. Keep the unscoped path.
            if agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached() {
                Ok(Some(
                    agent_doc_document_realtime_io::retain_deferred_document_write_target(
                        file,
                        current,
                        updated,
                        SOURCE,
                        agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
                    )?,
                ))
            } else {
                agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
                    file, current, updated, SOURCE,
                )?;
                Ok(None)
            }
        }
    }
}

/// Write the blocker-first response for a quota-blocked attempt
/// (`#blockerfront`). This is not a completion report: the attempt stays
/// `QuotaBlocked`, and the raw harness notice is not the exchange body.
pub(crate) fn publish_quota_blocked_response(file: &Path, claim: &QueueClaim) -> Result<()> {
    let _lock = acquire_completion_lock(
        file,
        "record a quota-blocked response",
        COMPLETION_FEEDBACK_LOCK_WAIT,
    )?;
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let response = agent_doc_queue::worker_quota::quota_blocked_row_response(
        &file.display().to_string(),
        claim,
    );
    let updated =
        agent_doc_document::blocked_response::project_blocked_row_response(&current, &response)?;
    if updated == current {
        return Ok(());
    }
    let scope = changed_component_scope(&current, &updated)
        .context("quota-blocked response must change only agent:exchange and agent:queue")?;
    if scope
        .to_pairs()
        .iter()
        .any(|(name, _)| name != "exchange" && name != "queue")
    {
        bail!("quota-blocked response escaped agent:exchange and agent:queue");
    }
    publish_exchange_update(file, &current, &updated, scope)?;
    Ok(())
}

fn publish_exchange_update(
    file: &Path,
    current: &str,
    updated: &str,
    scope: agent_doc_element::component_scope::ComponentWriteScope,
) -> Result<Option<String>> {
    if agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached() {
        Ok(Some(with_component_write_scope(scope, || {
            agent_doc_document_realtime_io::retain_deferred_document_write_target(
                file,
                current,
                updated,
                SOURCE,
                agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
            )
        })?))
    } else {
        with_component_write_scope(scope, || {
            agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
                file, current, updated, SOURCE,
            )
        })?;
        Ok(None)
    }
}

fn repair_legacy_completion_feedback_content(content: &str) -> (String, usize) {
    let mut output = String::with_capacity(content.len());
    let mut lines = content.split_inclusive('\n').peekable();
    let mut repaired = 0;

    while let Some(line) = lines.next() {
        let bare = line.trim_end_matches(['\r', '\n']);
        let Some(_detail) = bare
            .strip_prefix(LEGACY_COMPLETION_HEADING_PREFIX)
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
        else {
            output.push_str(line);
            continue;
        };
        let Some(next) = lines.peek() else {
            output.push_str(line);
            continue;
        };
        if !is_completion_marker_line(next) {
            output.push_str(line);
            continue;
        }

        let newline = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
        output.push_str(bare);
        output.push_str(newline);
        output.push_str(COMPLETION_BODY);
        output.push_str(newline);
        repaired += 1;
    }
    (output, repaired)
}

fn is_completion_marker_line(line: &str) -> bool {
    agent_doc_document::subagent_completion::is_marker_line(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_element_queue::QueueItemIdentity;
    use std::fs;
    use std::time::Duration;

    fn test_claim() -> QueueClaim {
        QueueClaim {
            identity: QueueItemIdentity::FreeText("finish-sdk".into()),
            item_text: "finish sdk.md\nwith deterministic tests".into(),
            owner: "subagent:sdk-feedback".into(),
            generation: 1,
            state: agent_doc_queue::queue_claim::QueueClaimState::Running,
            lost_from: None,
            completion_recovered_after_loss: false,
            publication_state: None,
            native_spawn_pending: Some(false),
            worker: Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
                worker_id: "worker:sdk-feedback".into(),
                started_at_secs: 10,
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: 10,
            expires_at_secs: 20,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            stage: None,
            reported_at_secs: None,
            heartbeat_at_secs: None,
            frozen: None,
            quota_block: None,
            lost_continue: None,
        }
    }

    #[test]
    fn completion_notice_is_deduped_and_not_prompt_bearing() {
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk.md with deterministic tests\n",
            "<!-- /agent:queue -->\n",
        );
        let claim = test_claim();
        let key = agent_doc_queue_io::queue_claim::completion_feedback_key(&claim).unwrap();
        let marker = format!("<!-- agent-doc-subagent-completion key={key} -->");
        let note = agent_doc_document::subagent_completion::cell_with_response(
            &claim.owner,
            &claim.item_text,
            &key,
            "Implemented the SDK fix.\n\n- 12 focused tests passed\n- branch pushed",
        )
        .unwrap();
        assert!(note.ends_with(&marker));
        assert!(note.contains("“finish sdk.md with deterministic tests” — agent-doc\n"));
        let after =
            agent_doc_element_exchange::insert_prompt_line_before_boundary(before, &note).unwrap();

        assert_eq!(after.matches("### Re: Subagent completed").count(), 1);
        assert!(after.find(&note).unwrap() < after.find("<!-- agent:boundary:keep -->").unwrap());
        assert!(after.contains("- finish sdk.md with deterministic tests"));
        assert!(after.contains(&marker));
        assert!(after.contains("> Implemented the SDK fix."));
        assert!(after.contains("> - 12 focused tests passed"));
        let diff = agent_doc_diff::unified_diff_from_contents(before, &after).unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(
            changes.iter().all(|change| matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
                    | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact
            )),
            "{changes:?}"
        );
        assert!(agent_doc_diff::prompt_prefix_normalization_targets(&diff).is_empty());
        let exchange_subagent_dispatch = agent_doc_preflight_io::exchange_subagent_dispatch_entries(
            &changes
                .iter()
                .filter(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                })
                .map(|change| change.text.clone())
                .collect::<Vec<_>>(),
        );
        assert!(
            exchange_subagent_dispatch.is_empty(),
            "an N-1 controller must not route the completion artifact as an exchange subagent prompt"
        );
        assert_eq!(
            agent_doc_document_realtime::write_policy::classify_safe_out_of_band_agent_doc_mutation(
                before, &after,
            ),
            Some("exchange"),
            "an open coordinator cycle must accept the binary completion artifact"
        );
        agent_doc_lint_io::validate_structure_on_content(Path::new("sdk.md"), &after)
            .expect("the session-check integrity classifier must see a non-empty response body");
    }

    #[test]
    fn legacy_bodyless_completion_feedback_repair_is_narrow_idempotent_and_integrity_safe() {
        let legacy_one = concat!(
            "### Re: Subagent completed — agent-doc: `subagent:sdk` returned work for queue item “finish sdk”; coordinator integration and closeout are pending.\n",
            "<!-- agent-doc-subagent-completion key=one -->\n",
        );
        let legacy_two = concat!(
            "### Re: Subagent completed — agent-doc: `subagent:dev` returned work for queue item “finish dev”; coordinator integration and closeout are pending.\n",
            "<!-- agent-doc-subagent-completion key=two -->\n",
        );
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append subagents -->\n",
                "{}",
                "{}",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
            ),
            legacy_one, legacy_two,
        );
        let error = agent_doc_lint_io::validate_structure_on_content(Path::new("sdk.md"), &before)
            .expect_err(
                "the historical bodyless form must reproduce the live session-check failure",
            );
        assert!(error.to_string().contains("has no response body"));

        let (repaired, count) = repair_legacy_completion_feedback_content(&before);
        assert_eq!(count, 2);
        assert_eq!(
            repaired.matches(LEGACY_COMPLETION_HEADING_PREFIX).count(),
            2
        );
        assert_eq!(repaired.matches(COMPLETION_BODY).count(), 2);
        assert!(repaired.contains("`subagent:sdk` returned work"));
        assert!(repaired.contains("`subagent:dev` returned work"));
        agent_doc_lint_io::validate_structure_on_content(Path::new("sdk.md"), &repaired)
            .expect("one or more repaired artifacts must pass the actual integrity classifier");

        let diff = agent_doc_diff::unified_diff_from_contents(
            concat!(
                "<!-- agent:exchange patch=append subagents -->\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
            ),
            &repaired,
        )
        .unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(
            changes.iter().all(|change| matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
                    | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact
            )),
            "the pre-existing N-1 `### Re:` classifier must keep migrated artifacts inert: {changes:?}"
        );

        let (again, second_count) = repair_legacy_completion_feedback_content(&repaired);
        assert_eq!(second_count, 0);
        assert_eq!(again, repaired);
    }

    #[test]
    fn repair_does_not_reclassify_or_hide_a_mixed_operator_prompt() {
        let content = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "Please investigate the completion failure.\n",
            "<!-- agent-doc-subagent-completion key=not-an-artifact-pair -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (after, count) = repair_legacy_completion_feedback_content(content);
        assert_eq!(count, 0);
        assert_eq!(after, content);
        let before = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let diff = agent_doc_diff::unified_diff_from_contents(before, &after).unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(changes.iter().any(|change| {
            change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                && change.text.contains("Please investigate")
        }));
    }

    #[test]
    fn completion_marker_does_not_hide_a_mixed_operator_prompt() {
        let before = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "Please review this before integrating.\n",
            "<!-- agent-doc-subagent-completion key=not-a-binary-block -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let diff = agent_doc_diff::unified_diff_from_contents(before, after).unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(changes.iter().any(|change| {
            change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                && change.text.contains("Please review this")
        }));
    }

    /// `#goalpresetterminal` regression (anonymized): a project-preset `/goal`
    /// review loop's worker handed back after posting one review with an
    /// actionable P2. The hand-back did not open on a declared final state, so
    /// it must keep the SAME row open, requeue the attempt, and carry the
    /// worker's worktree and remaining findings to the next worker's brief,
    /// without adding a duplicate row. A later hand-back that opens on
    /// `REVIEW-CLEAN` takes the ordinary report path.
    #[test]
    fn goal_loop_hand_back_without_final_state_requeues_the_same_row() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head. Start a separate fixer using #address, then a fresh\n",
                "    independent reviewer of the new exact head.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr";
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review1", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review1",
            "worker:pr12-review1",
            600,
            1,
        )
        .unwrap();

        let interrupted = concat!(
            "Posted one plain COMMENTED review at ",
            "https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000001 ",
            "against head aaaa111.\n\n",
            "Verdict: changes needed. One P2 finding remains: reconnect drops the retained outbox.\n",
            "Delivery worktree: /work/sample-repo/pr12-loop\n",
            "INLINE-THREADS-LOADED 0 head=aaaa111\n",
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), interrupted)
            .unwrap()
            .expect("a hand-back without a final state is an interrupted loop");
        assert_eq!(requeue.final_states, vec!["REVIEW-CLEAN", "BLOCKED"]);
        assert_eq!(requeue.claim.generation, 1);
        assert!(!requeue.answered_blocked_review);
        assert!(!requeue.suppress_immediate_refill);

        // The same row is open, undisturbed, and not duplicated; no completion
        // cell claims the loop finished.
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            content.matches("pull/12: #adversarial-pr").count(),
            1,
            "{content}"
        );
        assert!(!content.contains("~~"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");

        // The attempt no longer blocks dispatch: the same row can be claimed
        // again, and its next brief carries the interrupted hand-back.
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger
                .claims
                .iter()
                .all(|claim| !claim.blocks_dispatch(claim.claimed_at_secs)),
            "{ledger:?}"
        );
        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review2", 600)
                .expect("the requeued row is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned,
            "the same row gets a new attempt generation"
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        let prior = brief
            .prior_goal_attempt
            .as_ref()
            .expect("the brief carries the interrupted attempt");
        assert_eq!(prior.owner, "subagent:pr12-review1");
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("Prior goal-loop attempt")
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("> Delivery worktree: /work/sample-repo/pr12-loop")
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("> Verdict: changes needed. One P2 finding remains")
        );

        // A final-state hand-back takes the ordinary report path and retires
        // the recorded hand-off.
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-review2")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review2",
            "worker:pr12-review2",
            600,
            generation,
        )
        .unwrap();
        let clean = concat!(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000002 ",
            "base=cccc333 head=bbbb222 cycles=2\n\nFresh independent review: zero surviving findings.\n",
            "INLINE-THREADS-LOADED 0 head=bbbb222\n",
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), clean)
                .unwrap()
                .is_none()
        );
        assert!(
            agent_doc_queue_io::goal_loop_handoff::load(&doc)
                .unwrap()
                .handoffs
                .is_empty()
        );
        let reported = report_complete(&doc, ROW, Some(generation), None, None, clean)
            .expect("the final-state hand-back is reported normally");
        assert!(reported.appended);
    }

    /// `#crossagenthandoff`: `#address then #adversarial-pr` with a landed
    /// `FIX-ON-HEAD` is still an open goal loop when the hand-back begins with
    /// `**Summary**`. The row is not `prose_consume`. Resume uses the agent-doc
    /// key. A Grok conversation id does not continue the Claude worker.
    #[test]
    fn prose_summary_on_address_and_adversarial_pr_returns_to_the_inbox() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("sdk-dev.md");
        const SESSION: &str = "78c70863-fed7-40a8-b703-8261eeb8da9c";
        const ROW: &str =
            "https://github.com/haiven-dev/haiven-sdk/pull/286: #address then #adversarial-pr";
        const OWNER: &str = "subagent:addr-286";
        const WORKER: &str = "a7e8d9d85c28f8da8";
        const GROK_CONVERSATION: &str = "01a1e0e1-6a34-7a12-bd15-5fa98f34c9e5";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: {SESSION}\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        let owner_session = Some(agent_doc_queue::queue_claim::ClaimOwnerSession {
            harness: Some("grok".to_string()),
            session_id: Some(GROK_CONVERSATION.to_string()),
            pane: None,
            pid: None,
        });
        agent_doc_queue_io::queue_claim::claim_with_session_for_worker_harness(
            &doc,
            ROW,
            OWNER,
            600,
            owner_session.clone(),
            Some("claude"),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt_with_owner_session(
            &doc,
            ROW,
            OWNER,
            WORKER,
            600,
            1,
            owner_session,
        )
        .unwrap();
        let prose = concat!(
            "**Summary**\n\n",
            "The address run landed. The adversarial loop needs a reviewer this subagent cannot spawn. ",
            "The owning session must spawn the reviewer.\n\n",
            "Delivery worktree: `/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/addr-286-g2`\n",
            "Head branch: `feat/demo-upstream-overlay`\n",
            "Head: `177f46b753f55f7b37c22706a615ad687544a041`\n",
            "FIX-ON-HEAD https://github.com/haiven-dev/haiven-sdk/pull/286#pullrequestreview-5912901233 head=177f46b753f55f7b37c22706a615ad687544a041\n",
            "INLINE-THREADS-LOADED 0 head=177f46b753f55f7b37c22706a615ad687544a041\n",
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), prose)
            .unwrap()
            .expect("a **Summary** hand-back stays open when the goal final state is unmet");
        assert_eq!(requeue.final_states, vec!["REVIEW-CLEAN", "BLOCKED"]);
        assert!(!requeue.address_fix_pending);
        assert!(!requeue.inline_threads_pending);
        assert_ne!(
            requeue.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches(ROW).count(), 1, "{content}");
        assert!(!content.contains("~~"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger
                .claims
                .iter()
                .all(|claim| !claim.blocks_dispatch(claim.claimed_at_secs)),
            "{ledger:?}"
        );
        let attempts = agent_doc_queue_io::queue_lineage::load(&doc).unwrap();
        assert!(
            attempts.attempts.iter().all(|attempt| {
                attempt.events.iter().all(|event| {
                    event.kind
                        != agent_doc_queue_io::queue_lineage::QueueLineageEventKind::ProseOnly
                })
            }),
            "a prose hand-back is not prose_only"
        );

        let resumed = agent_doc_queue_io::cross_harness_handoff::resume_by_key(
            &doc, &content, ROW, OWNER, 1, WORKER,
        )
        .unwrap();
        match resumed {
            agent_doc_queue_io::cross_harness_handoff::StoredResume::Recovered {
                worktree,
                branch,
                head,
                record,
            } => {
                assert_eq!(
                    worktree,
                    "/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/addr-286-g2"
                );
                assert_eq!(branch, "feat/demo-upstream-overlay");
                assert_eq!(head, "177f46b753f55f7b37c22706a615ad687544a041");
                assert_eq!(record.key.session_id, SESSION);
                assert_eq!(record.key.worker_id, WORKER);
            }
            other => panic!("the agent-doc key recovers the worktree: {other:?}"),
        }
        match agent_doc_queue_io::cross_harness_handoff::refuse_conversation(
            &doc,
            "grok",
            GROK_CONVERSATION,
        )
        .unwrap()
        {
            agent_doc_queue_io::cross_harness_handoff::StoredResume::ConversationRefused {
                conversation_id,
                others,
                ..
            } => {
                assert_eq!(conversation_id, GROK_CONVERSATION);
                assert_eq!(others[0].key.worker_id, WORKER);
                assert_eq!(others[0].worker_harness.as_deref(), Some("claude"));
            }
            other => panic!("the grok conversation does not continue the worker: {other:?}"),
        }
        let inbox = agent_doc_queue_io::cross_harness_handoff::list_inbox(&doc).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].from_harness, "claude");
        assert_eq!(inbox[0].owner_harness, "grok");
        assert!(inbox[0].message.contains("Spawn the next worker"));

        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:addr-286-next", 600)
                .expect("the requeued row is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:addr-286-next")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:addr-286-next",
            "worker:addr-286-next",
            600,
            generation,
        )
        .unwrap();
        let clean = concat!(
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-sdk/pull/286#pullrequestreview-5912901233 ",
            "base=b6cfa91701ff0b482bf8d404e24b6a742d8cd0ca head=177f46b753f55f7b37c22706a615ad687544a041 cycles=2\n",
            "FIX-ON-HEAD https://github.com/haiven-dev/haiven-sdk/pull/286#pullrequestreview-5912901233 head=177f46b753f55f7b37c22706a615ad687544a041\n",
            "INLINE-THREADS-LOADED 0 head=177f46b753f55f7b37c22706a615ad687544a041\n",
        );
        let _proof = proved_address_observation();
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), clean)
                .unwrap()
                .is_none(),
            "REVIEW-CLEAN still takes the ordinary report path once the address effect is observed"
        );
    }

    /// `#operatororderclosed`: BLOCKED on a human review the preset already
    /// refuses to approve or dismiss stays the same open row. It is not an
    /// operator question, and it does not post another review. A later
    /// hand-back that already wrote the pull-request explanation suppresses
    /// the immediate refill. A runner-budget BLOCKED and REVIEW-CLEAN still
    /// take the ordinary report path.
    #[test]
    fn blocked_human_review_the_preset_already_answers_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head. Do not approve or dismiss a human review. A required\n",
                "    human approval this preset cannot submit stays BLOCKED and does not\n",
                "    start another address cycle.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr";
        start_subagent(&doc, ROW, "subagent:pr12-review1");
        let approval_blocker = format!(
            "<!-- agent-doc:prompt-blocker -->\n\
             - [/] The current head is independently review-clean and awaits merge approval.\n\
             \x20 - **Complete by:** Receive an eligible GitHub review approving the current PR head.\n\
             \x20 - **Prompt:** {ROW}\n\
             \x20 - **Evidence:** GitHub reports REVIEW_REQUIRED and BLOCKED on bbbb222.\n\
             <!-- /agent-doc:prompt-blocker -->"
        );
        let runner_blocker = approval_blocker.replace(
            "Receive an eligible GitHub review approving the current PR head.",
            "Restore the hosted runner budget before the independent review.",
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &runner_blocker)
                .unwrap()
                .is_none(),
            "evidence about approval does not override an independent external gate"
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, None, &approval_blocker)
                .unwrap_err()
                .to_string()
                .contains("requires --generation")
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &approval_blocker)
            .unwrap()
            .expect("an already-answered human-review blocker returns to the goal loop");
        assert!(requeue.answered_blocked_review);
        assert!(!requeue.suppress_immediate_refill);
        assert_eq!(requeue.claim.generation, 1);
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            content.matches("pull/12: #adversarial-pr").count(),
            1,
            "{content}"
        );
        assert!(!content.contains("~~"), "{content}");
        assert!(!content.contains("agent:review"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");

        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review2", 600)
                .expect("the requeued order is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        let preamble = brief.subagent_prompt_preamble.to_ascii_lowercase();
        assert!(
            preamble.contains("(#operatororderclosed)"),
            "{}",
            brief.subagent_prompt_preamble
        );
        assert!(
            preamble.contains("do not dismiss"),
            "{}",
            brief.subagent_prompt_preamble
        );

        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-review2")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review2",
            "worker:pr12-review2",
            600,
            generation,
        )
        .unwrap();
        let explained = concat!(
            "BLOCKED reviewer clears or replaces review 1000000001 ",
            "base=cccc333 head=bbbb222 cycles=2\n\n",
            "Posted the pull-request comment ",
            "https://github.com/sample-org/sample-repo/pull/12#issuecomment-100 ",
            "explaining why the work is not progressing. The findings are already ",
            "addressed and this loop does not dismiss the review.\n",
            "INLINE-THREADS-LOADED 0 head=bbbb222\n",
        );
        let explained_requeue =
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), explained)
                .unwrap()
                .expect("the explained hand-back still does not close the order");
        assert!(explained_requeue.answered_blocked_review);
        assert!(explained_requeue.suppress_immediate_refill);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("~~"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");

        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review3", 600)
                .unwrap();
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-review3")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review3",
            "worker:pr12-review3",
            600,
            generation,
        )
        .unwrap();
        let budget = concat!(
            "BLOCKED restore the hosted runner budget base=cccc333 head=bbbb222 cycles=2\n\n",
            "Review URLs: https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-100\n",
            "INLINE-THREADS-LOADED 0 head=bbbb222\n",
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), budget)
                .unwrap()
                .is_none(),
            "a runner-budget BLOCKED still completes"
        );
        let clean = concat!(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-200 ",
            "base=cccc333 head=bbbb222 cycles=3\n",
            "INLINE-THREADS-LOADED 0 head=bbbb222\n",
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), clean)
                .unwrap()
                .is_none(),
            "REVIEW-CLEAN still completes"
        );
    }

    /// `#reviewfixonhead`: an address hand-back that leaves the review's written
    /// fix off the head is not completion. The same row stays open, and a later
    /// hand-back that names the head containing the written fix reports normally.
    #[test]
    fn address_hand_back_without_the_written_fix_requeues_the_same_row() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/12: #address";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address",
            "worker:pr12-address",
            600,
            1,
        )
        .unwrap();
        let skipped = concat!(
            "Every actionable item is already fixed on head 2a507fba. No new commit.\n\n",
            "Earlier reviews that asked for the shadow apply were superseded and were not reopened.",
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), skipped)
            .unwrap()
            .expect("an address run without the written fix stays open");
        assert!(requeue.address_fix_pending);
        assert_eq!(requeue.claim.generation, 1);
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches(ROW).count(), 1, "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");

        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address2", 600)
                .expect("the requeued address row is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned,
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("(#reviewfixonhead)")
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("were superseded and were not reopened")
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-address2")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address2",
            "worker:pr12-address2",
            600,
            generation,
        )
        .unwrap();
        let landed = concat!(
            "The retry applies under shadow. The stored state, the audit applied_state, and the acknowledgement are allowed.\n",
            "FIX-ON-HEAD https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-5837752992 head=fa3a3f449986d565f21aeb6e0f792323bc8d1d58\n",
            "INLINE-THREADS-LOADED 0 head=fa3a3f449986d565f21aeb6e0f792323bc8d1d58\n",
        );
        let unproven = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), landed)
            .unwrap()
            .expect("FIX-ON-HEAD without an observed head change or review reply stays open");
        assert!(!unproven.address_fix_pending);
        assert!(unproven.address_effect_unproven);
        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address3", 600)
                .expect("the unproven address row is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned,
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-address3")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address3",
            "worker:pr12-address3",
            600,
            generation,
        )
        .unwrap();
        let _proof = proved_address_observation();
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), landed)
                .unwrap()
                .is_none(),
            "an observed head change completes the address run"
        );
        let reported = report_complete(&doc, ROW, Some(generation), None, None, landed)
            .expect("the written fix on the head completes the address run");
        assert!(reported.appended);
    }

    /// `#reviewfixonhead`: `#address then #adversarial-pr` whose reviews state
    /// no code change is not another fix pass. `NO-WRITTEN-FIX` plus a
    /// `THREAD-NO-FIX` receipt settles the address half and leaves the goal
    /// loop open for the independent review.
    #[test]
    fn no_written_fix_on_address_then_adversarial_pr_continues_the_review() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("sdk-dev.md");
        const ROW: &str =
            "https://github.com/sample-org/sample-repo/pull/12: #address then #adversarial-pr";
        const HEAD: &str = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-sdk\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address",
            "worker:pr12-address",
            600,
            1,
        )
        .unwrap();
        // The address half's receipt is not the observation. A posted reply is.
        let _proof = proved_address_reply();
        let prose = format!(
            "**Summary**\n\n\
             The approval states no code change, so nothing was committed.\n\
             NO-WRITTEN-FIX head={HEAD}\n\
             THREAD-NO-FIX https://github.com/sample-org/sample-repo/pull/12#discussion_r4603884112 head={HEAD} resolved\n"
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &prose)
            .unwrap()
            .expect("a no-fix address half stays open until REVIEW-CLEAN or BLOCKED");
        assert!(!requeue.address_fix_pending);
        assert!(!requeue.inline_threads_pending);
        assert_eq!(requeue.final_states, vec!["REVIEW-CLEAN", "BLOCKED"]);
        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review", 600)
                .expect("the requeued row is dispatchable for the independent review");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(brief.subagent_prompt_preamble.contains("NO-WRITTEN-FIX"));
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("Do not land a code change and do not start another address pass")
        );
        assert!(
            !brief
                .subagent_prompt_preamble
                .contains("Prior address attempt"),
            "a settled no-fix half must not brief another fix pass"
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("not permission to skip"),
            "{}",
            brief.subagent_prompt_preamble
        );
    }

    /// `#presetorder`: a no-change address half does not finish a later preset
    /// that declares no goal final states. `PRESET-DONE` for that preset, after
    /// the address receipt and an observed review reply, takes the ordinary
    /// report path. A pure `#address` row does not complete on `NO-WRITTEN-FIX`
    /// alone (`#addressproof`).
    #[test]
    fn no_written_fix_does_not_skip_a_later_preset() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "  '#adversarial-pr': 'Post one independent COMMENTED review on the current head.'\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("sdk-dev.md");
        const ROW: &str =
            "https://github.com/sample-org/sample-repo/pull/12: #address then #adversarial-pr";
        const HEAD: &str = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-sdk\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address",
            "worker:pr12-address",
            600,
            1,
        )
        .unwrap();
        // The address half's receipt is not the observation. A posted reply is.
        let _proof = proved_address_reply();
        let prose = format!(
            "**Summary**\n\n\
             The approval states no code change, so nothing was committed.\n\
             NO-WRITTEN-FIX head={HEAD}\n\
             INLINE-THREADS-LOADED 0 head={HEAD}\n"
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &prose)
            .unwrap()
            .expect("a no-change address half does not skip the later preset");
        assert!(!requeue.address_fix_pending);
        assert!(!requeue.inline_threads_pending);
        assert!(requeue.final_states.is_empty());
        assert_eq!(
            requeue.next_ordered_preset.as_deref(),
            Some("#adversarial-pr")
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        let preamble = &brief.subagent_prompt_preamble;
        assert!(preamble.contains("not permission to skip"), "{preamble}");
        assert!(preamble.contains("#adversarial-pr"), "{preamble}");
        assert!(
            preamble.contains("Do not land a code change and do not start another address pass"),
            "{preamble}"
        );
        assert!(!preamble.contains("Prior address attempt"), "{preamble}");
        assert!(!preamble.contains("Land that written fix"), "{preamble}");

        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review", 600)
                .expect("the requeued row is dispatchable for the later preset");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-review")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review",
            "worker:pr12-review",
            600,
            generation,
        )
        .unwrap();
        let done = format!("{prose}PRESET-DONE #adversarial-pr\n");
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &done)
                .unwrap()
                .is_none(),
            "PRESET-DONE after the address receipt finishes the ordered row"
        );
    }

    /// `#addressproof`: `NO-WRITTEN-FIX` does not complete a pure address run.
    /// An observed head change or review reply does.
    #[test]
    fn no_written_fix_without_a_head_change_or_reply_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/12: #address";
        const HEAD: &str = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address",
            "worker:pr12-address",
            600,
            1,
        )
        .unwrap();
        let prose = format!(
            "Every review states no code change.\n\
             NO-WRITTEN-FIX head={HEAD}\n\
             INLINE-THREADS-LOADED 0 head={HEAD}\n"
        );
        let unproven = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &prose)
            .unwrap()
            .expect("no commit and no review reply does not complete the address");
        assert!(!unproven.address_fix_pending);
        assert!(unproven.address_effect_unproven);
        let _proof = proved_address_reply();
        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-address2", 600)
                .unwrap();
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:pr12-address2")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-address2",
            "worker:pr12-address2",
            600,
            generation,
        )
        .unwrap();
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &prose)
                .unwrap()
                .is_none(),
            "an observed review reply completes the address run"
        );
    }

    /// `#noopaddress`: sdk-dev `#address` under haiven-sdk PR #389
    /// (2026-11-23) had nothing to address. Its `NO-WRITTEN-FIX` hand-back
    /// requeued every generation, and the coordinator told the operator to
    /// delete the row. A remote read showing the head has no unresolved
    /// thread, no changes-requested review, and no failing or unfinished
    /// required check is the address effect, so the no-op hand-back settles.
    #[test]
    fn no_written_fix_on_a_head_with_nothing_to_address_completes() {
        use agent_doc_queue::address_completion_proof::{
            AddressObservation, AddressReviewState, AddressSnapshot, ForcedObservation,
        };
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/389: #address";
        const HEAD: &str = "a1379d91362c2defe0e45870af5a279320b058c8";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:s389address", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:s389address",
            "worker:s389address",
            600,
            1,
        )
        .unwrap();
        let hand_back = format!(
            "PR #389 has nothing to address: no unresolved thread, no changes requested, required checks green.\n\
             NO-WRITTEN-FIX head={HEAD}\n\
             INLINE-THREADS-LOADED 0 head={HEAD}\n"
        );
        let snapshot = |review_state: Option<AddressReviewState>| AddressSnapshot {
            head_sha: HEAD.to_string(),
            reply_ids: Vec::new(),
            comment_commit: None,
            thread_named: false,
            review_state: review_state.map(Box::new),
        };
        {
            let _unresolved = ForcedObservation::install(AddressObservation::Seen {
                baseline: Some(snapshot(None)),
                current: snapshot(Some(AddressReviewState {
                    unresolved_threads: 1,
                    ..AddressReviewState::default()
                })),
            });
            let requeued = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &hand_back)
                .unwrap()
                .expect("an unresolved thread still asks for work");
            assert!(requeued.address_effect_unproven);
        }
        let (_, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:s389address2", 600)
                .unwrap();
        let generation = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == "subagent:s389address2")
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:s389address2",
            "worker:s389address2",
            600,
            generation,
        )
        .unwrap();
        let _clean = ForcedObservation::install(AddressObservation::Seen {
            baseline: Some(snapshot(None)),
            current: snapshot(Some(AddressReviewState::default())),
        });
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &hand_back)
                .unwrap()
                .is_none(),
            "a head with nothing to address settles the no-op address hand-back"
        );
    }

    fn proved_address_observation() -> agent_doc_queue::address_completion_proof::ForcedObservation
    {
        use agent_doc_queue::address_completion_proof::{
            AddressObservation, AddressSnapshot, ForcedObservation,
        };
        let snap = |head: &str| AddressSnapshot {
            head_sha: head.to_string(),
            reply_ids: Vec::new(),
            comment_commit: None,
            thread_named: false,
            review_state: None,
        };
        ForcedObservation::install(AddressObservation::Seen {
            baseline: Some(snap("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
            current: snap("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        })
    }

    fn proved_address_reply() -> agent_doc_queue::address_completion_proof::ForcedObservation {
        use agent_doc_queue::address_completion_proof::{
            AddressObservation, AddressSnapshot, ForcedObservation,
        };
        let head = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        let snap = |replies: Vec<u64>| AddressSnapshot {
            head_sha: head.to_string(),
            reply_ids: replies,
            comment_commit: Some(head.to_string()),
            thread_named: true,
            review_state: None,
        };
        ForcedObservation::install(AddressObservation::Seen {
            baseline: Some(snap(Vec::new())),
            current: snap(vec![4_606_896_300]),
        })
    }

    /// `#presetorder`: an ungated earlier preset does not add a receipt.
    /// `#rebase then #adversarial-pr` still completes on REVIEW-CLEAN.
    #[test]
    fn preset_order_review_clean_completes_rebase_then_goal() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#rebase': 'Rebase onto the base branch.'\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        const ROW: &str =
            "https://github.com/sample-org/sample-repo/pull/12: #rebase then #adversarial-pr";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n\
                 ### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - {ROW}\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:pr12-review", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:pr12-review",
            "worker:pr12-review",
            600,
            1,
        )
        .unwrap();
        let clean = concat!(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-200 ",
            "base=cccc333 head=bbbb222 cycles=1\n",
            "INLINE-THREADS-LOADED 0 head=bbbb222\n",
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), clean)
                .unwrap()
                .is_none(),
            "REVIEW-CLEAN completes #rebase then #adversarial-pr"
        );
    }

    const INLINE_MUST: &str =
        "https://github.com/haiven-dev/haiven-api/pull/869#discussion_r4532159601";
    const INLINE_SHOULD: &str =
        "https://github.com/haiven-dev/haiven-api/pull/869#discussion_r4532069052";
    const INLINE_HEAD: &str = "87afcacde5eaed85bc87372fad897903afdebacf";

    fn inline_project(dir: &Path) {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        fs::write(
            dir.join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#address': 'Address every actionable review item on the exact current head.'\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head. Start a separate fixer using #address.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
    }

    fn inline_doc(dir: &Path, row: &str) -> std::path::PathBuf {
        let doc = dir.join("prs.md");
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n- {row}\n<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    fn restart_subagent(doc: &Path, row: &str, owner: &str) -> u64 {
        let (outcome, _) = agent_doc_queue_io::queue_claim::claim(doc, row, owner, 600).unwrap();
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let generation = agent_doc_queue_io::queue_claim::load_ledger(doc)
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.owner == owner)
            .map(|claim| claim.generation)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            doc,
            row,
            owner,
            &format!("worker:{owner}"),
            600,
            generation,
        )
        .unwrap();
        generation
    }

    /// `#addressinline`: an `#address` run that only reads the review body, or
    /// that cites a later reply, stays the same open row. Receipts for every
    /// named root thread take the ordinary report path. Dismissing a thread
    /// does not.
    #[test]
    fn address_hand_back_without_inline_threads_requeues_the_same_row() {
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        const ROW: &str = "https://github.com/haiven-dev/haiven-api/pull/869: #address";
        let doc = inline_doc(dir.path(), ROW);
        start_subagent(&doc, ROW, "subagent:address1");
        let review_body = concat!(
            "Addressed the review body at ",
            "https://github.com/haiven-dev/haiven-api/pull/869#pullrequestreview-5837752992.",
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), review_body)
            .unwrap()
            .expect("a review body does not load inline discussion comments");
        assert!(requeue.inline_threads_pending);
        assert!(requeue.final_states.is_empty());
        assert!(!requeue.answered_blocked_review);
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches(ROW).count(), 1, "{content}");
        assert!(!content.contains("~~"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");

        let generation = restart_subagent(&doc, ROW, "subagent:address2");
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        let preamble = &brief.subagent_prompt_preamble;
        assert!(preamble.contains("(#addressinline)"), "{preamble}");
        assert!(
            preamble.contains("gh api repos/<owner>/<repo>/pulls/<number>/comments"),
            "{preamble}"
        );
        assert!(
            preamble.contains("Prior inline-thread attempt"),
            "{preamble}"
        );
        assert!(
            preamble.contains("> Addressed the review body at "),
            "{preamble}"
        );

        let later =
            format!("Later replies on {INLINE_MUST} and {INLINE_SHOULD} are marked resolved.");
        let later_requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &later)
            .unwrap()
            .expect("a later reply does not load the root comment");
        assert!(later_requeue.inline_threads_pending);

        let generation = restart_subagent(&doc, ROW, "subagent:address3");
        let dismissed = format!(
            "THREAD-FIX-ON-HEAD {INLINE_MUST} head={INLINE_HEAD} resolved\n\
             THREAD-FIX-ON-HEAD {INLINE_SHOULD} head={INLINE_HEAD} resolved\n\
             I dismissed the thread.\n"
        );
        let dismissed_requeue =
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &dismissed)
                .unwrap()
                .expect("dismissing a thread does not finish the run");
        assert!(dismissed_requeue.inline_threads_pending);

        let generation = restart_subagent(&doc, ROW, "subagent:address4");
        let settled = format!(
            "FIX-ON-HEAD https://github.com/haiven-dev/haiven-api/pull/869#pullrequestreview-5837752992 head={INLINE_HEAD}\n\
             THREAD-FIX-ON-HEAD {INLINE_MUST} head={INLINE_HEAD} resolved\n\
             THREAD-FIX-ON-HEAD {INLINE_SHOULD} head={INLINE_HEAD} resolved\n"
        );
        let unproven = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &settled)
            .unwrap()
            .expect("thread receipts without an observed head change or reply stay open");
        assert!(unproven.address_effect_unproven);
        let generation = restart_subagent(&doc, ROW, "subagent:address5");
        let _proof = proved_address_observation();
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &settled)
                .unwrap()
                .is_none(),
            "receipts plus an observed head change take the ordinary report path"
        );
    }

    /// `#addressinline`: `REVIEW-CLEAN` does not finish `#adversarial-pr` while
    /// a root thread's written fix is absent from the hand-back. Receipts, or
    /// an explicit zero-comment query, do.
    #[test]
    fn adversarial_review_clean_without_inline_threads_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        const ROW: &str = "https://github.com/haiven-dev/haiven-api/pull/869: #adversarial-pr";
        let doc = inline_doc(dir.path(), ROW);
        start_subagent(&doc, ROW, "subagent:adversarial1");
        let clean = format!(
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-api/pull/869#pullrequestreview-5837752992 \
             base=cccc333 head={INLINE_HEAD} cycles=1\n\nThe review body is clean."
        );
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &clean)
            .unwrap()
            .expect("REVIEW-CLEAN without inline threads stays open");
        assert!(requeue.inline_threads_pending);
        assert_eq!(requeue.final_states, vec!["REVIEW-CLEAN", "BLOCKED"]);
        assert!(!requeue.answered_blocked_review);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("### Re: Subagent completed"), "{content}");
        assert!(!content.contains("~~"), "{content}");

        let generation = restart_subagent(&doc, ROW, "subagent:adversarial2");
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(brief.subagent_prompt_preamble.contains("(#addressinline)"));
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("Prior inline-thread attempt")
        );
        let with_receipts = format!(
            "{clean}\n\
             THREAD-FIX-ON-HEAD {INLINE_MUST} head={INLINE_HEAD} resolved\n\
             THREAD-FIX-ON-HEAD {INLINE_SHOULD} head={INLINE_HEAD} resolved\n"
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &with_receipts)
                .unwrap()
                .is_none(),
            "REVIEW-CLEAN plus a receipt for every named thread completes"
        );
        let zero = format!(
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-api/pull/869#pullrequestreview-5837752992 \
             base=cccc333 head={INLINE_HEAD} cycles=1\n\
             INLINE-THREADS-LOADED 0 head={INLINE_HEAD}\nQueried pulls/869/comments.\n"
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(generation), &zero)
                .unwrap()
                .is_none(),
            "REVIEW-CLEAN plus an explicit empty comments query completes"
        );
    }

    /// `#inlinegatefalsereject`: the PR 412 `#adversarial-pr` hand-back, as a
    /// nested child under its `### <PR url>` heading. The gate once refused it
    /// because the hand-back cited the reply comment `discussion_r4568355508`
    /// beside the receipted root thread `discussion_r4568099735`. That refusal
    /// requeued generation 1 as an abandoned `Lost` attempt, and the
    /// coordinator then ran `queue release --requeue` too. The same hand-back
    /// must now report from that generation, so the row can close, instead of
    /// the row waiting forever on `⚠️🚧` for a new generation.
    #[test]
    fn inlinegatefalsereject_pr412_hand_back_reports_after_the_false_requeue() {
        const HAND_BACK: &str =
            include_str!("../agent-doc-queue/tests/fixtures/pr412_adversarial_handback.md");
        const ROW: &str =
            "https://github.com/btakita/agent-doc/pull/412: #adversarial-pr [#q-23c8d4f70342]";
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        let doc = dir.path().join("prs.md");
        fs::write(
            &doc,
            "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n\
             <!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue subagents -->\n\
             - ### https://github.com/btakita/agent-doc/pull/412\n\
             \x20 - #adversarial-pr [#q-23c8d4f70342]\n\
             <!-- /agent:queue -->\n",
        )
        .unwrap();
        start_subagent(&doc, ROW, "subagent:adv412");

        // The verdict the old gate reached on this hand-back: the reply citation
        // counted as a second root thread. Reproduce that requeue with the
        // receipt removed, which still fails the gate.
        let unreceipted = HAND_BACK
            .lines()
            .filter(|line| !line.starts_with("THREAD-FIX-ON-HEAD"))
            .collect::<Vec<_>>()
            .join("\n");
        let requeue = requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &unreceipted)
            .unwrap()
            .expect("without its receipt the root thread is still unsettled");
        assert!(requeue.inline_threads_pending);
        let lost = |doc: &Path| {
            agent_doc_queue_io::queue_claim::load_ledger(doc)
                .unwrap()
                .claims
                .into_iter()
                .find(|claim| claim.owner == "subagent:adv412")
                .unwrap()
        };
        let requeued = lost(&doc);
        assert_eq!(
            requeued.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert!(requeued.lost_from.is_none());
        assert_ne!(
            requeued.projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
        );
        assert!(
            requeued
                .frozen
                .as_ref()
                .is_some_and(|freeze| freeze.is_abandon()),
            "{requeued:#?}"
        );
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();

        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), HAND_BACK)
                .unwrap()
                .is_none(),
            "a cited reply comment is not a second root thread"
        );
        let outcome = report_complete(&doc, ROW, Some(1), None, None, HAND_BACK)
            .expect("the gate-requeued generation reports its corrected hand-back");
        assert_eq!(
            outcome.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert_eq!(outcome.claim.generation, 1);
        assert!(
            outcome.claim.completion_recovered_after_loss,
            "the report keeps the loss boundary visible"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: Subagent completed"), "{content}");
    }

    /// `#inlinegatefalsereject`: only a gate's own requeue reopens. A plain
    /// `queue release --requeue` with no gate hand-off, or a reclaimed
    /// generation, stays refused.
    #[test]
    fn inlinegatefalsereject_a_plain_requeue_does_not_reopen_the_attempt() {
        const ROW: &str = "https://github.com/btakita/agent-doc/pull/412: #adversarial-pr";
        const HAND_BACK: &str =
            include_str!("../agent-doc-queue/tests/fixtures/pr412_adversarial_handback.md");
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        let doc = inline_doc(dir.path(), ROW);
        start_subagent(&doc, ROW, "subagent:plain");
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), HAND_BACK)
                .unwrap()
                .is_none()
        );
        assert!(
            report_complete(&doc, ROW, Some(1), None, None, HAND_BACK).is_err(),
            "an explicit requeue without a gate verdict stays abandoned"
        );
    }

    // ---- `#lostrowreportreopen` -------------------------------------------

    const PR412_ROW: &str =
        "https://github.com/btakita/agent-doc/pull/412: #adversarial-pr [#q-23c8d4f70342]";
    const PR412_HAND_BACK: &str =
        include_str!("../agent-doc-queue/tests/fixtures/pr412_adversarial_handback.md");

    /// The PR 412 session document: the row nested under its `### <PR url>`
    /// heading. `marker` is the row's visible lifecycle prefix.
    fn pr412_doc(dir: &Path, marker: &str) -> std::path::PathBuf {
        let doc = dir.join("prs.md");
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n\
                 <!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n\
                 <!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents -->\n\
                 - ### https://github.com/btakita/agent-doc/pull/412\n\
                 \x20 - {marker}#adversarial-pr [#q-23c8d4f70342]\n\
                 <!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    fn pr412_claim(doc: &Path) -> QueueClaim {
        agent_doc_queue_io::queue_claim::load_ledger(doc)
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.owner == "subagent:adv412")
            .unwrap()
    }

    /// The PR 412 hand-back with its root-thread receipt removed: the verdict
    /// the old `#addressinline` gate reached on the real hand-back, which
    /// counted the cited reply as a second root thread.
    fn pr412_false_verdict() -> String {
        PR412_HAND_BACK
            .lines()
            .filter(|line| !line.starts_with("THREAD-FIX-ON-HEAD"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The worker ended (the SubagentStop hook froze it with its hand-back).
    /// Since `#finishedheadrefill` the same pass's lost-running recovery
    /// leaves it alone: a recorded hand-back is report-owed, so continue
    /// never runs, never fails, and authorizes no new generation.
    fn pr412_worker_ended(doc: &Path) {
        agent_doc_queue_io::queue_claim::freeze_attempt(
            doc,
            PR412_ROW,
            Some(1),
            &agent_doc_queue::queue_claim::subagent_end_freeze_reason(Some(PR412_HAND_BACK)),
        )
        .unwrap();
        agent_doc_queue_io::lost_running::recover_lost_running_automatically(doc, true).unwrap();
        let ended = pr412_claim(doc);
        assert_eq!(
            ended.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert!(ended.lost_continue.is_none(), "{ended:#?}");
        assert!(ended.ended_hand_back_owed().is_some(), "{ended:#?}");
        assert!(
            !agent_doc_queue::lost_running_recovery::retry_authorized(&ended),
            "{ended:#?}"
        );
    }

    fn assert_reported_after_loss(outcome: &CompletionFeedbackOutcome, doc: &Path) {
        assert_eq!(
            outcome.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert_eq!(outcome.claim.generation, 1);
        assert!(
            outcome.claim.completion_recovered_after_loss,
            "the report keeps the loss boundary visible (⚠️☑️)"
        );
        let content = fs::read_to_string(doc).unwrap();
        assert!(content.contains("### Re: Subagent completed"), "{content}");
    }

    /// `#lostrowreportreopen`: the live PR 412 sequence, end to end. The
    /// worker ended and the harness froze generation 1 BEFORE the coordinator
    /// reported, so the gate's two false `#addressinline` requeues and the
    /// coordinator's `queue release --requeue` all landed on an attempt that
    /// already carried a harness freeze. PR 422's test skipped the freeze, so
    /// its gate requeue wrote the abandon its reopen looked for; here the old
    /// abandon never replaced the harness freeze and the reopen refused, and
    /// report-complete then failed `generation 1 is terminal (Lost)`.
    #[test]
    fn lostrowreportreopen_pr412_sequence_reports_after_freeze_two_gate_requeues_and_release() {
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        let doc = pr412_doc(dir.path(), "");
        start_subagent(&doc, PR412_ROW, "subagent:adv412");
        pr412_worker_ended(&doc);

        let false_verdict = pr412_false_verdict();
        for _ in 0..2 {
            let requeue =
                requeue_nonterminal_goal_hand_back(&doc, PR412_ROW, Some(1), &false_verdict)
                    .unwrap()
                    .expect("the false verdict requeues the attempt");
            assert!(requeue.inline_threads_pending);
            let lost = pr412_claim(&doc);
            assert_eq!(
                lost.state,
                agent_doc_queue::queue_claim::QueueClaimState::Running
            );
            assert!(lost.lost_from.is_none());
            assert_ne!(
                lost.projection_marker(),
                agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
            );
            assert!(
                lost.frozen
                    .as_ref()
                    .is_some_and(|freeze| freeze.is_abandon()),
                "a requeue of a harness-frozen attempt is still an abandon: {lost:#?}"
            );
        }
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            PR412_ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();

        assert!(
            requeue_nonterminal_goal_hand_back(&doc, PR412_ROW, Some(1), PR412_HAND_BACK)
                .unwrap()
                .is_none()
        );
        let outcome = report_complete(&doc, PR412_ROW, Some(1), None, None, PR412_HAND_BACK)
            .expect("the gate-requeued generation reports its hand-back");
        assert_reported_after_loss(&outcome, &doc);
    }

    /// `#lostrowreportreopen`: the state the live ledger actually holds after
    /// the binaries before this fix ran that sequence. Generation 1 is `Lost`
    /// from running, its freeze is still the harness end-freeze recording the
    /// hand-back (the old requeue never marked it abandoned), its continue
    /// failed, and no gate hand-off survives (the first post-422 report retired
    /// it before refusing). The Stop hook demanded continue/retry for it.
    ///
    /// The row is closable: the freeze recorded the hand-back the coordinator
    /// now reports. The Stop hook owes that report, and after it the `⚠️☑️`
    /// row owes integration, never continue or retry.
    #[test]
    fn lostrowreportreopen_legacy_lost_row_with_recorded_hand_back_reports_and_stop_hook_agrees() {
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        let doc = pr412_doc(dir.path(), "");
        start_subagent(&doc, PR412_ROW, "subagent:adv412");
        pr412_worker_ended(&doc);
        let mut legacy = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        for claim in &mut legacy.claims {
            // The binaries before `#finishedheadrefill` failed continue on it.
            claim.lost_continue = Some(agent_doc_queue::queue_claim::LostContinueRecord {
                attempted_at_secs: claim.claimed_at_secs,
                failed: true,
                reason: "harness freeze worker is gone; continue refused".to_string(),
                during_turn: true,
            });
            claim.mark_lost();
        }
        let root = agent_doc_fs::find_project_root(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(&doc).unwrap(),
                state_kind: agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND.to_string(),
                canonical_path: doc.canonicalize().unwrap().display().to_string(),
                payload_json: serde_json::to_string(&legacy).unwrap(),
                updated_at_secs: 0,
            },
        )
        .unwrap();
        drop(conn);
        let lost = pr412_claim(&doc);
        assert_eq!(
            lost.state,
            agent_doc_queue::queue_claim::QueueClaimState::Lost
        );
        assert!(lost.lost_end_hand_back().is_some(), "{lost:#?}");
        assert!(
            agent_doc_queue_io::goal_loop_handoff::load(&doc)
                .unwrap()
                .handoffs
                .is_empty()
        );

        // The Stop hook: report the recorded hand-back, not continue/retry.
        pr412_doc(dir.path(), "⚠️🚧 [#subagent:adv412#1] ");
        let content = fs::read_to_string(&doc).unwrap();
        let evidence = agent_doc_queue_io::transient_closeout::closeout_evidence(&doc, &content);
        assert_eq!(evidence.reportable_lost.len(), 1, "{evidence:#?}");
        let failure =
            agent_doc_queue::transient_closeout::normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .expect("the lost row still owes its report");
        assert!(
            failure.contains("report the hand-back its ended worker left"),
            "{failure}"
        );
        assert!(
            !failure.contains("continue the same generation"),
            "{failure}"
        );

        // A different hand-back is not the one the freeze recorded, even when
        // it passes the gate.
        let other =
            PR412_HAND_BACK.replacen("pullrequestreview-5897902287", "pullrequestreview-1", 1);
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, PR412_ROW, Some(1), &other)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            pr412_claim(&doc).state,
            agent_doc_queue::queue_claim::QueueClaimState::Lost,
            "only the recorded hand-back reopens the attempt"
        );
        assert!(report_complete(&doc, PR412_ROW, Some(1), None, None, &other).is_err());

        assert!(
            requeue_nonterminal_goal_hand_back(&doc, PR412_ROW, Some(1), PR412_HAND_BACK)
                .unwrap()
                .is_none()
        );
        let outcome = report_complete(&doc, PR412_ROW, Some(1), None, None, PR412_HAND_BACK)
            .expect("the recorded hand-back reports the lost generation");
        assert_reported_after_loss(&outcome, &doc);

        let reported = fs::read_to_string(&doc).unwrap();
        let reported = reported.replace(
            "⚠️🚧 [#subagent:adv412#1] #adversarial-pr",
            "⚠️☑️ [#subagent:adv412#1] #adversarial-pr",
        );
        let content = reported;
        let evidence = agent_doc_queue_io::transient_closeout::closeout_evidence(&doc, &content);
        assert!(evidence.reportable_lost.is_empty(), "{evidence:#?}");
        let failure =
            agent_doc_queue::transient_closeout::normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .expect("a recovered report still owes integration");
        assert!(
            failure.contains("integrate the recovered report"),
            "{failure}"
        );
        assert!(
            !failure.contains("continue the same generation"),
            "{failure}"
        );
    }

    /// `#lostrowreportreopen`: a plain `queue release --requeue` of an attempt
    /// the harness froze when its worker ended is a deliberate abandon. With
    /// no gate verdict, its report stays refused even with the very hand-back
    /// the freeze recorded.
    #[test]
    fn lostrowreportreopen_a_plain_requeue_after_the_end_freeze_does_not_reopen() {
        let dir = tempfile::tempdir().unwrap();
        inline_project(dir.path());
        let doc = pr412_doc(dir.path(), "");
        start_subagent(&doc, PR412_ROW, "subagent:adv412");
        pr412_worker_ended(&doc);
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            PR412_ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();
        let abandoned = pr412_claim(&doc);
        assert!(abandoned.lost_end_hand_back().is_none(), "{abandoned:#?}");
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, PR412_ROW, Some(1), PR412_HAND_BACK)
                .unwrap()
                .is_none()
        );
        let err = report_complete(&doc, PR412_ROW, Some(1), None, None, PR412_HAND_BACK)
            .expect_err("an explicit abandon without a gate verdict stays refused");
        assert!(err.to_string().contains("is terminal"), "{err:#}");
    }

    fn start_subagent(doc: &Path, row: &str, owner: &str) {
        agent_doc_queue_io::queue_claim::claim(doc, row, owner, 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            doc,
            row,
            owner,
            &format!("worker:{owner}"),
            600,
            1,
        )
        .unwrap();
    }

    #[test]
    fn unchanged_external_blocker_keeps_one_generation_until_evidence_changes() {
        use agent_doc_queue::subagent_pool::PoolRefillTrigger;
        use agent_doc_queue_io::{queue_claim, subagent_pool};

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("work.md");
        const ROW: &str = "finish the widget [#q-111111111111]";
        const SIBLING: &str = "finish the widget [#q-222222222222]";
        const OWNER: &str = "subagent:widget";
        let handback = format!(
            "<!-- agent-doc:prompt-blocker -->\n\
             - [/] The export is waiting for a signing key\n  \
             - **Complete by:** Provide the signing key reference\n  \
             - **Prompt:** {ROW}\n  \
             - **Evidence:** secret status: absent\n\
             <!-- /agent-doc:prompt-blocker -->"
        );
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_format: template\nqueue: go\n---\n\n\
                 <!-- agent:active -->\n\n<!-- /agent:active -->\n\n\
                 <!-- agent:run id=\"main\" subagents go -->\n- {ROW}\n- {SIBLING}\n\
                 <!-- /agent:run -->\n"
            ),
        )
        .unwrap();
        start_subagent(&doc, ROW, OWNER);
        let project = || {
            agent_doc_queue_io::queue_cmd::project_claim_runtime_state_with_effects(
                &crate::CliQueueCommandEffects,
                &doc,
                Some(ROW),
            )
            .unwrap();
        };
        let active_body = |content: &str| {
            agent_doc_element::element::parse(content)
                .unwrap()
                .into_iter()
                .find(|component| component.name == "active")
                .unwrap()
                .content(content)
                .to_string()
        };
        project();
        let running = fs::read_to_string(&doc).unwrap();
        assert!(
            active_body(&running).contains("finish the widget"),
            "{running}"
        );
        assert!(
            requeue_nonterminal_goal_hand_back(&doc, ROW, Some(1), &handback)
                .unwrap()
                .is_none()
        );

        let report = || requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), &handback, false);
        let first = report().unwrap().unwrap();
        project();
        let held = fs::read_to_string(&doc).unwrap();
        assert!(!active_body(&held).contains("finish the widget"), "{held}");
        assert!(
            held.contains("blocked_until_evidence_hash_changes="),
            "{held}"
        );
        assert!(
            agent_doc_queue::prompt_pressure::review_blocks_prompt(&held, ROW),
            "{held}"
        );
        assert!(
            held.lines()
                .any(|line| line.contains("⛔") && line.contains(ROW)),
            "{held}"
        );
        let reformatted = held.replace("secret status: absent", "secret   status: absent");
        assert!(agent_doc_queue::prompt_pressure::review_blocks_prompt(
            &reformatted,
            ROW
        ));
        let refill =
            subagent_pool::pool_refill_for_content(&doc, &held, PoolRefillTrigger::Plan).unwrap();
        assert!(
            refill.next_heads.iter().all(|head| head.item != ROW)
                && refill.next_heads.iter().any(|head| head.item == SIBLING),
            "{refill:#?}"
        );
        assert!(refill.refresh_due.is_empty(), "{refill:#?}");

        for _ in 0..3 {
            let replay = report().unwrap().unwrap();
            assert_eq!(replay.review_id, first.review_id);
            assert_eq!(replay.claim.generation, 1);
            assert_eq!(fs::read_to_string(&doc).unwrap(), held);
            let claim_err = queue_claim::claim(&doc, ROW, "subagent:retry", 600).unwrap_err();
            assert!(claim_err.to_string().contains("held by external blocker"));
            assert!(
                queue_claim::refresh_attempt(&doc, ROW, OWNER, 600, Some(1))
                    .unwrap_err()
                    .to_string()
                    .contains("held by external blocker")
            );
        }
        assert_eq!(
            agent_doc_queue_io::queue_lineage::max_generation(&doc, ROW).unwrap(),
            1
        );

        let changed = held.replace("secret status: absent", "secret status: available");
        fs::write(&doc, &changed).unwrap();
        assert!(!agent_doc_queue::prompt_pressure::review_blocks_prompt(
            &changed, ROW
        ));
        assert!(report().unwrap_err().to_string().contains("relinquished"));
        let refill =
            subagent_pool::pool_refill_for_content(&doc, &changed, PoolRefillTrigger::Plan)
                .unwrap();
        assert!(
            refill.next_heads.iter().any(|head| head.item == ROW),
            "{refill:#?}"
        );
        let (_, next) = queue_claim::claim(&doc, ROW, "subagent:retry", 600).unwrap();
        assert_eq!(next.generation, 2);
    }

    const BRT_GH: &str = "do [#ghrecvpolicy]: #advance whatever is actionable.";
    const BRT_CHROMIUM: &str =
        "[#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.";
    const BRT_GH_HAND_BACK: &str = concat!(
        "<!-- agent-doc:prompt-blocker -->\n",
        "- [/] [#prompt-blocker-01eda3b6d7de] Recipient-side policy awaits multi-user workspace support.\n",
        "  - **Complete by:** Enforce the recipient policy once multi-user workspace access control lands.\n",
        "  - **Prompt:** do [#ghrecvpolicy]\n",
        "<!-- /agent-doc:prompt-blocker -->\n",
    );

    fn brt_chromium_hand_back() -> String {
        format!(
            concat!(
                "<!-- agent-doc:prompt-blocker -->\n",
                "- [/] Production activation awaits a published signed JetBrains frontend release.\n",
                "  - **Complete by:** Verify the signed frontend package, then confirm live activation receipts.\n",
                "  - **Prompt:** {row}\n",
                "  - **Evidence:** GitHub release v0.35.585 has no signed frontend assets.\n",
                "<!-- /agent-doc:prompt-blocker -->\n",
            ),
            row = BRT_CHROMIUM
        )
    }

    fn brt_doc(dir: &Path) -> std::path::PathBuf {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("agent-doc-bugs.md");
        fs::write(
            &doc,
            format!(
                concat!(
                    "---\nagent_doc_format: template\nqueue: go\n---\n\n",
                    "<!-- agent:active -->\n\n<!-- /agent:active -->\n\n",
                    "<!-- agent:run id=\"main\" subagents go -->\n",
                    "- ### {gh}\n",
                    "- ### {chromium}\n",
                    "<!-- /agent:run -->\n\n",
                    "<!-- agent:backlog -->\n",
                    "- [ ] [#ghrecvpolicy] Recipient-side auto_create_components policy. (stays open: workspace access control and session isolation must land before enforcing the recipient policy)\n",
                    "<!-- /agent:backlog -->\n",
                ),
                gh = BRT_GH,
                chromium = BRT_CHROMIUM,
            ),
        )
        .unwrap();
        doc
    }

    fn brt_row<'a>(content: &'a str, needle: &str) -> &'a str {
        content
            .lines()
            .find(|line| line.contains(needle) && line.trim_start().starts_with("- ###"))
            .unwrap_or_else(|| panic!("no row {needle:?} in {content}"))
    }

    fn brt_project(doc: &Path) {
        agent_doc_queue_io::queue_cmd::project_claim_runtime_state_with_effects(
            &crate::CliQueueCommandEffects,
            doc,
            None,
        )
        .unwrap();
    }

    fn brt_live_claims(doc: &Path) -> Vec<QueueClaim> {
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(doc).unwrap();
        ledger
            .claims
            .into_iter()
            .filter(|claim| {
                [BRT_GH, BRT_CHROMIUM]
                    .iter()
                    .any(|row| claim.identity == agent_doc_queue::queue_claim::claim_identity(row))
            })
            .collect()
    }

    /// `#blockerreportterminal` (agent-doc-bugs.md rows `#ghrecvpolicy` and
    /// `#prompt-blocker-b0ddfbae2f94`, 2026-11-21): two workers each handed
    /// back only a prompt-blocker review, one a legacy review naming the
    /// short prompt `do [#ghrecvpolicy]`, one an external hold. Recording the
    /// hand-back used to requeue the attempt; the dispatcher's continue then
    /// failed, the claim read `running / replacement_pending` with no worker,
    /// the rows repainted `📥 [#subagent:…#1]`, and the Stop hook refused the
    /// turn forever. Recording a blocker-only hand-back now closes the claim,
    /// files the review, and rests the row `⛔` citing it. Nothing owes a
    /// replacement until the review resolves; the next claim is generation 2.
    #[test]
    fn blockerreportterminal_a_blocker_only_hand_back_closes_its_claim_and_rests_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let doc = brt_doc(dir.path());
        start_subagent(&doc, BRT_GH, "subagent:ghrecvpolicy");
        start_subagent(&doc, BRT_CHROMIUM, "subagent:chromium_steps");
        brt_project(&doc);
        assert_eq!(brt_live_claims(&doc).len(), 2);

        let chromium_hand_back = brt_chromium_hand_back();
        let gh = requeue_prompt_pressure_hand_back(&doc, BRT_GH, Some(1), BRT_GH_HAND_BACK, false)
            .unwrap()
            .expect("a review hand-back is prompt pressure");
        assert_eq!(
            gh.kind,
            agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review
        );
        assert_eq!(gh.review_id.as_deref(), Some("prompt-blocker-01eda3b6d7de"));
        let chromium = requeue_prompt_pressure_hand_back(
            &doc,
            BRT_CHROMIUM,
            Some(1),
            &chromium_hand_back,
            false,
        )
        .unwrap()
        .expect("an external hold is prompt pressure");
        assert!(chromium.external_evidence.is_some());
        brt_project(&doc);

        // (1) The claim is closed, not retained as a replacement-pending attempt.
        assert!(
            brt_live_claims(&doc).is_empty(),
            "{:#?}",
            brt_live_claims(&doc)
        );
        assert!(
            agent_doc_queue_io::lost_running::recover_lost_running_automatically(&doc, true)
                .unwrap()
                .is_empty()
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            agent_doc_queue_io::queue_claim::claim_reports_for_content(&doc, &content)
                .unwrap()
                .iter()
                .all(|report| report.assessment != "replacement_pending"),
        );
        // The row rests `⛔` citing the review, with no stale owner tag.
        let gh_row = brt_row(&content, BRT_GH);
        assert!(gh_row.contains("⛔"), "{content}");
        assert!(gh_row.contains("prompt-blocker-01eda3b6d7de"), "{content}");
        assert!(!gh_row.contains("[#subagent:"), "{content}");
        let chromium_row = brt_row(&content, BRT_CHROMIUM);
        assert!(chromium_row.contains("⛔"), "{content}");
        assert!(!chromium_row.contains("[#subagent:"), "{content}");
        assert!(!content.contains("📥"), "{content}");
        let evidence = agent_doc_queue_io::transient_closeout::closeout_evidence(&doc, &content);
        assert!(
            agent_doc_queue::transient_closeout::normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .is_none(),
            "{content}"
        );

        // (2)+(3) An older projector's stale `📥 [#subagent:…#1]` paint is
        // quiescent blocked, and the next projection rests it `⛔` again.
        let stale = content
            .replace(
                gh_row,
                &format!("- ### 📥 [#subagent:ghrecvpolicy#1] {BRT_GH}"),
            )
            .replace(
                chromium_row,
                &format!("- ### 📥 [#subagent:chromium_steps#1] {BRT_CHROMIUM}"),
            );
        let evidence = agent_doc_queue_io::transient_closeout::closeout_evidence(&doc, &stale);
        let assessment =
            agent_doc_queue::transient_closeout::assess_transient_closeout(&stale, &evidence)
                .unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:#?}");
        assert_eq!(assessment.quiescent_state(), Some("blocked"));
        fs::write(&doc, &stale).unwrap();
        brt_project(&doc);
        let repainted = fs::read_to_string(&doc).unwrap();
        assert!(!repainted.contains("[#subagent:"), "{repainted}");
        assert!(brt_row(&repainted, BRT_GH).contains("⛔"), "{repainted}");

        // A retried report of the same hand-back is already recorded.
        let replay =
            requeue_prompt_pressure_hand_back(&doc, BRT_GH, Some(1), BRT_GH_HAND_BACK, false)
                .unwrap()
                .expect("the recorded review hand-back answers again");
        assert_eq!(replay.claim.generation, 1);
        assert_eq!(replay.claim.owner, "subagent:ghrecvpolicy");
        let replay = requeue_prompt_pressure_hand_back(
            &doc,
            BRT_CHROMIUM,
            Some(1),
            &chromium_hand_back,
            false,
        )
        .unwrap()
        .expect("the recorded external hold answers again");
        assert_eq!(replay.review_id, chromium.review_id);
        assert_eq!(fs::read_to_string(&doc).unwrap(), repainted);

        // The review still holds the row; resolving it makes it claimable
        // again at a fenced new generation.
        assert!(
            agent_doc_queue_io::queue_claim::claim(&doc, BRT_GH, "subagent:retry", 600).is_err()
        );
        assert_eq!(
            agent_doc_queue_io::queue_lineage::max_generation(&doc, BRT_GH).unwrap(),
            1
        );
        let resolved = repainted.replace(
            "- [/] [#prompt-blocker-01eda3b6d7de]",
            "- [x] [#prompt-blocker-01eda3b6d7de]",
        );
        assert_ne!(resolved, repainted);
        fs::write(&doc, &resolved).unwrap();
        let (_, next) =
            agent_doc_queue_io::queue_claim::claim(&doc, BRT_GH, "subagent:retry", 600).unwrap();
        assert_eq!(next.generation, 2);
    }

    /// `#blockerreportterminal`: a ledger an older binary wrote still holds the
    /// abandoned, replacement-pending attempt, and the row shows the stale
    /// `📥 [#subagent:ghrecvpolicy#1]` tag. The marker reconcile closes that
    /// orphan attempt and repaints the row `⛔` citing the review.
    #[test]
    fn blockerreportterminal_reconcile_closes_a_legacy_orphan_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let doc = brt_doc(dir.path());
        start_subagent(&doc, BRT_GH, "subagent:ghrecvpolicy");
        // The old path: requeue retains the started attempt abandoned, then
        // the dispatcher's continue fails and authorizes a replacement.
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            BRT_GH,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();
        agent_doc_queue_io::lost_running::recover_lost_running_automatically(&doc, true).unwrap();
        let orphan = brt_live_claims(&doc);
        assert_eq!(orphan.len(), 1, "{orphan:#?}");
        assert!(orphan[0].replacement_pending(), "{orphan:#?}");

        let blocker =
            agent_doc_queue::prompt_pressure::parse_prompt_blocker(BRT_GH_HAND_BACK).unwrap();
        let content = fs::read_to_string(&doc).unwrap();
        let stale = agent_doc_queue::prompt_pressure::hoist_prompt_blocker(&content, &blocker)
            .replace(
                &format!("- ### {BRT_GH}"),
                &format!("- ### 📥 [#subagent:ghrecvpolicy#1] {BRT_GH}"),
            );
        assert!(stale.contains("📥 [#subagent:ghrecvpolicy#1]"), "{stale}");
        fs::write(&doc, &stale).unwrap();
        assert!(
            agent_doc_queue_io::queue_claim::claim_reports_for_content(&doc, &stale)
                .unwrap()
                .iter()
                .any(|report| report.assessment == "replacement_pending")
        );

        assert!(crate::plan::reconcile_visible_claim_markers(&doc).unwrap());
        assert!(
            brt_live_claims(&doc).is_empty(),
            "{:#?}",
            brt_live_claims(&doc)
        );
        let healed = fs::read_to_string(&doc).unwrap();
        let row = brt_row(&healed, BRT_GH);
        assert!(row.contains("⛔"), "{healed}");
        assert!(row.contains("prompt-blocker-01eda3b6d7de"), "{healed}");
        assert!(!row.contains("[#subagent:"), "{healed}");
        assert!(!crate::plan::reconcile_visible_claim_markers(&doc).unwrap());
    }

    /// `#promptpressure`: a prose blocker is not a completion. The same row
    /// stays open, no exchange essay is written, and the next brief quotes the
    /// hand-back as data.
    #[test]
    fn prose_blocker_hand_back_requeues_without_an_exchange_essay() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("work.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sample-work\nagent_doc_format: template\n---\n\n",
                "<!-- agent:queue -->\n",
                "- finish the widget\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
            ),
        )
        .unwrap();
        const ROW: &str = "finish the widget";
        start_subagent(&doc, ROW, "subagent:widget");
        let blocked = "Blocked. The widget export is unfinished because the signing key is absent.";
        let pressure = requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), blocked, false)
            .unwrap()
            .expect("a prose blocker does not complete the prompt");
        assert_eq!(
            pressure.kind,
            agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("signing key"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");
        assert!(!content.contains("~~"), "{content}");
        assert_eq!(
            content.matches("- finish the widget").count(),
            1,
            "{content}"
        );
        let (outcome, _) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:widget-2", 600)
                .expect("the requeued row is dispatchable again");
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("Prior prompt attempt")
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("> Blocked. The widget export is unfinished")
        );
        assert!(brief.subagent_prompt_preamble.contains("#promptpressure"));
    }

    /// `#closepressure`: a structured done hand-back that cites an already
    /// merged pull request does not report a setup head complete, even when
    /// the repository result was delivered. A creation receipt does.
    #[test]
    fn closepressure_setup_hand_back_requeues_until_a_pull_request_is_created() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk-dev.md");
        const ROW: &str = "haiven-sdk: Setup Release Please workflow to replace the run that opened https://github.com/haiven-dev/haiven-sdk/pull/228 [#q-bac390dcf354]";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sdk-dev\nagent_doc_format: template\n---\n\n<!-- agent:queue -->\n- {ROW}\n<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        start_subagent(&doc, ROW, "subagent:closepressure");
        let cited = format!(
            "> **Queue prompt:**\n> {ROW}\n\n\
**Summary**\n\n\
This leaf is answered and is safe to delete.\n\n\
**What changed**\n\n\
The setup pull request is the merged https://github.com/haiven-dev/haiven-sdk/pull/228. The promotion that workflow opened is https://github.com/haiven-dev/haiven-sdk/pull/227.\n\n\
**Verification**\n\n\
- `cargo test`\n"
        );
        let pressure = requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), &cited, true)
            .unwrap()
            .expect("citing an existing pull request does not complete a setup head");
        assert_eq!(
            pressure.kind,
            agent_doc_queue::goal_preset_terminal::PromptPressureKind::Prose
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("### Re: Subagent completed"), "{content}");
        assert!(!content.contains("~~"), "{content}");
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(
            brief.subagent_prompt_preamble.contains("#closepressure"),
            "{}",
            brief.subagent_prompt_preamble
        );
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:closepressure-2", 600)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            ROW,
            "subagent:closepressure-2",
            "worker:closepressure-2",
            600,
            claim.generation,
        )
        .unwrap();
        let opened = "Opened https://github.com/haiven-dev/haiven-sdk/pull/229\n";
        assert!(
            requeue_prompt_pressure_hand_back(&doc, ROW, Some(claim.generation), opened, true)
                .unwrap()
                .is_none(),
            "a pull request this turn opened takes the ordinary report path"
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger.claims.iter().any(|claim| {
                claim.owner == "subagent:closepressure-2" && claim.state.blocks_dispatch()
            }),
            "{ledger:?}"
        );
    }

    const STGSDKMOD_ROW: &str = "[#q-7256e3fa62d9] follow-up: agent: in `/home/brian/work/btakita/agent-loop/src/haiven-dev/src/haiven-sdk`, open a PR to `sdk-dev` that handles `chat.message.moderated` in the SDK chat client, so a held message moves to accepted or removed. Mirror how web chat (haiven-frontend) shows pending, held and moderated messages, add a test, and re-vendor Unity. [#q-fbf061cf35e4]";

    /// The generation-2..4 hand-back, abridged: #402 is finished, every
    /// commit is generation 1's, and this generation opened nothing.
    const STGSDKMOD_HAND_BACK: &str = "**Summary**\n\
PR https://github.com/haiven-dev/haiven-sdk/pull/402 is finished, and CI passed on head `7913f3cb7190a2bca86b1fc6f4bcd505c91c2a51`: every check completed, none pending, none failed, and GitHub reports it ready to merge with no conflicts.\n\n\
**What changed**\n\
- Branch: `fix/sdk-chat-moderated-event`, targeting `sdk-dev`. It is an open PR, not a draft.\n\
- Commits, both by generation 1. This generation added none.\n\n\
**Verification**\n\
- `gh pr view 402`: open, head `7913f3cb7`, ready to merge, no conflicts.\n\n\
**Action items**\n\
- human (haiven-sdk reviewer): review and approve https://github.com/haiven-dev/haiven-sdk/pull/402.\n";

    fn stgsdkmod_doc(dir: &Path) -> std::path::PathBuf {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("sdk-dev.md");
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sdk-dev\nagent_doc_format: template\n---\n\n<!-- agent:queue -->\n- {STGSDKMOD_ROW}\n<!-- /agent:queue -->\n\n<!-- agent:exchange patch=append -->\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n"
            ),
        )
        .unwrap();
        doc
    }

    /// Start generation `generation` of the row on `worker`. A later
    /// generation follows a requeue of the one before, as the work pool does.
    fn stgsdkmod_start(doc: &Path, generation: u64, worker: &str) {
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(doc, STGSDKMOD_ROW, "subagent:stgsdkmod", 600)
                .unwrap();
        assert_eq!(claim.generation, generation);
        agent_doc_queue_io::queue_claim::start_attempt(
            doc,
            STGSDKMOD_ROW,
            "subagent:stgsdkmod",
            worker,
            600,
            generation,
        )
        .unwrap();
    }

    /// Set each attempt's `Created` receipt to the live ledger's times:
    /// generation 1 at 21:59:49Z, generation 2 at 00:52:22Z.
    fn stgsdkmod_backdate_lineage(doc: &Path) -> u64 {
        const GEN1_CREATED: u64 = 1_795_471_189;
        const GEN2_CREATED: u64 = 1_795_481_542;
        let mut ledger = agent_doc_queue_io::queue_lineage::load(doc).unwrap();
        for attempt in &mut ledger.attempts {
            let at = match attempt.generation {
                1 => GEN1_CREATED,
                2 => GEN2_CREATED,
                other => GEN2_CREATED + other * 30,
            };
            for event in &mut attempt.events {
                event.at_secs = at;
            }
        }
        let root = agent_doc_fs::find_project_root(doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(doc).unwrap(),
                state_kind: agent_doc_queue_io::queue_lineage::QUEUE_LINEAGE_STATE_KIND.to_string(),
                canonical_path: doc.canonicalize().unwrap().display().to_string(),
                payload_json: serde_json::to_string(&ledger).unwrap(),
                updated_at_secs: 0,
            },
        )
        .unwrap();
        GEN1_CREATED
    }

    fn pr402_facts(
        created_at_secs: u64,
    ) -> impl FnMut(
        &agent_doc_queue::attempt_lineage_effect::CitedPullRequest,
    ) -> Option<agent_doc_queue::attempt_lineage_effect::PullRequestFacts> {
        move |pull_request| {
            (pull_request.number == 402).then(|| {
                agent_doc_queue::attempt_lineage_effect::PullRequestFacts {
                    created_at_secs,
                    head_ref: "fix/sdk-chat-moderated-event".to_string(),
                    head_sha: "7913f3cb7190a2bca86b1fc6f4bcd505c91c2a51".to_string(),
                    state: agent_doc_queue::attempt_lineage_effect::PullRequestLifecycle::Open,
                }
            })
        }
    }

    /// `#attemptlineage`: the `stgsdkmod` sequence. Generation 1 opened
    /// haiven-sdk #402 at 22:14:28Z and was then frozen; generation 2 found
    /// it finished and cited it. `#closepressure` requeued that hand-back
    /// (and generations 3 and 4) because THIS attempt had not opened the pull
    /// request. A pull request an earlier attempt of the same row opened is
    /// the row's effect: the report lands.
    #[test]
    fn attemptlineage_stgsdkmod_cited_pr_from_an_earlier_generation_reports_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let doc = stgsdkmod_doc(dir.path());
        stgsdkmod_start(&doc, 1, "a1f8f4c4f759dc682");
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            STGSDKMOD_ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();
        stgsdkmod_start(&doc, 2, "ae465212a5b46b9c1");
        let gen1_created = stgsdkmod_backdate_lineage(&doc);
        let pr402_created = 1_795_475_668; // 2026-11-23T22:14:28Z

        // A pull request that predates the row is still refused.
        let predates = requeue_prompt_pressure_hand_back_with(
            &doc,
            STGSDKMOD_ROW,
            Some(2),
            STGSDKMOD_HAND_BACK,
            true,
            pr402_facts(gen1_created - 1),
        )
        .unwrap()
        .expect("a pull request the row never created does not complete it");
        assert!(predates.close_pressure);
        stgsdkmod_start(&doc, 3, "ae465212a5b46b9c1");

        // Without a receipt or readable facts the citation fails closed.
        let unknown = requeue_prompt_pressure_hand_back_with(
            &doc,
            STGSDKMOD_ROW,
            Some(3),
            STGSDKMOD_HAND_BACK,
            true,
            |_| None,
        )
        .unwrap()
        .expect("unproven provenance stays refused");
        assert!(unknown.close_pressure);
        stgsdkmod_start(&doc, 4, "ae465212a5b46b9c1");
        stgsdkmod_backdate_lineage(&doc);

        assert!(
            requeue_prompt_pressure_hand_back_with(
                &doc,
                STGSDKMOD_ROW,
                Some(4),
                STGSDKMOD_HAND_BACK,
                true,
                pr402_facts(pr402_created),
            )
            .unwrap()
            .is_none(),
            "generation 1 opened #402 during its own attempt"
        );
        let outcome = report_complete(
            &doc,
            STGSDKMOD_ROW,
            Some(4),
            None,
            None,
            STGSDKMOD_HAND_BACK,
        )
        .expect("the row's own pull request reports the row");
        assert_eq!(
            outcome.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
    }

    /// `#attemptlineage`: the ledger the old binary left. Generation 4's
    /// hand-back was requeued by `#closepressure`, which retains the started
    /// attempt abandoned, and an abandoned attempt refuses its report. Once
    /// the same hand-back passes the gate, the coordinator reports that same
    /// generation: the verdict that abandoned it is answered.
    #[test]
    fn attemptlineage_a_close_pressure_requeue_reopens_when_its_hand_back_now_passes() {
        let dir = tempfile::tempdir().unwrap();
        let doc = stgsdkmod_doc(dir.path());
        stgsdkmod_start(&doc, 1, "a1f8f4c4f759dc682");
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            STGSDKMOD_ROW,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(1),
        )
        .unwrap();
        stgsdkmod_start(&doc, 2, "ae465212a5b46b9c1");
        stgsdkmod_backdate_lineage(&doc);
        // The old binary's verdict.
        requeue_prompt_pressure_hand_back_with(
            &doc,
            STGSDKMOD_ROW,
            Some(2),
            STGSDKMOD_HAND_BACK,
            true,
            |_| None,
        )
        .unwrap()
        .expect("the old verdict requeued generation 2");
        let abandoned = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.generation == 2)
            .unwrap();
        assert!(
            agent_doc_queue::lost_running_recovery::authorized_running_requeue(&abandoned),
            "{abandoned:#?}"
        );
        assert!(
            report_complete(
                &doc,
                STGSDKMOD_ROW,
                Some(2),
                None,
                None,
                STGSDKMOD_HAND_BACK
            )
            .is_err(),
            "an abandoned attempt refuses its report until the verdict is answered"
        );

        assert!(
            requeue_prompt_pressure_hand_back_with(
                &doc,
                STGSDKMOD_ROW,
                Some(2),
                STGSDKMOD_HAND_BACK,
                true,
                pr402_facts(1_795_475_668),
            )
            .unwrap()
            .is_none()
        );
        let outcome = report_complete(
            &doc,
            STGSDKMOD_ROW,
            Some(2),
            None,
            None,
            STGSDKMOD_HAND_BACK,
        )
        .expect("the answered verdict reopens the same generation");
        assert_eq!(
            outcome.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
    }

    /// A repository delivery is an ordinary report even when the prose leads
    /// with Blocked. A goal `BLOCKED` line is a final state, not prompt pressure.
    #[test]
    fn prompt_pressure_leaves_a_delivery_and_a_goal_final_state_alone() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\npresets:\n",
                "  '#adversarial-pr': >-\n",
                "    /goal Bring the pull request to an independently review-clean exact\n",
                "    remote head.\n\n",
                "    Final states. Your final message must begin with exactly one of these\n",
                "    lines:\n",
                "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
                "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`.\n",
                "---\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("prs.md");
        const ROW: &str = "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr";
        fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: sample-prs\nagent_doc_format: template\n---\n\n<!-- agent:queue -->\n- {ROW}\n<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        start_subagent(&doc, ROW, "subagent:pr12");
        let blocked = "BLOCKED the signing key is absent base=aaaa111 head=bbbb222 cycles=1";
        assert!(
            requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), blocked, false)
                .unwrap()
                .is_none(),
            "a goal BLOCKED line is a final state"
        );
        let prose = "Blocked. The export is unfinished because the signing key is absent.";
        assert!(
            requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), prose, true)
                .unwrap()
                .is_none(),
            "a delivered repository result stays on the ordinary report path"
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger
                .claims
                .iter()
                .any(|claim| claim.owner == "subagent:pr12" && claim.state.blocks_dispatch()),
            "{ledger:?}"
        );
    }

    /// `#promptpressure`: the review structure is hoisted to the front of
    /// `agent:review`. The essay stays out of exchange, and claim refuses the
    /// named row until that review is resolved.
    #[test]
    fn review_structure_hand_back_is_hoisted_and_blocks_claim() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("work.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sample-work\nagent_doc_format: template\n---\n\n",
                "<!-- agent:queue -->\n",
                "- finish the widget\n",
                "- ship the other crate\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:review -->\n",
                "- [/] [#other] Keep me\n",
                "<!-- /agent:review -->\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
            ),
        )
        .unwrap();
        const ROW: &str = "finish the widget";
        start_subagent(&doc, ROW, "subagent:widget");
        let response = concat!(
            "The export cannot land until the signing key is present.\n\n",
            "<!-- agent-doc:prompt-blocker -->\n",
            "- [/] [#prompt-blocker-widget] The widget export is unfinished\n",
            "  - **Complete by:** land the export commit on fix/widget\n",
            "  - **Prompt:** finish the widget\n",
            "<!-- /agent-doc:prompt-blocker -->\n",
        );
        let pressure = requeue_prompt_pressure_hand_back(&doc, ROW, Some(1), response, true)
            .unwrap()
            .expect("a review structure is hoisted even when a repository result was delivered");
        assert_eq!(
            pressure.kind,
            agent_doc_queue::goal_preset_terminal::PromptPressureKind::Review
        );
        assert_eq!(pressure.review_id.as_deref(), Some("prompt-blocker-widget"));
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("<!-- agent-doc:prompt-blocker -->"),
            "{content}"
        );
        assert!(
            content.find("[#prompt-blocker-widget]").unwrap() < content.find("[#other]").unwrap(),
            "{content}"
        );
        assert!(
            content.find("<!-- agent:review -->").unwrap()
                < content.find("<!-- agent:exchange").unwrap(),
            "{content}"
        );
        assert!(!content.contains("signing key"), "{content}");
        assert!(!content.contains("### Re: Subagent completed"), "{content}");
        assert!(!content.contains("~~"), "{content}");
        assert_eq!(
            content.matches("- finish the widget\n").count(),
            1,
            "{content}"
        );
        let refused = agent_doc_queue_io::queue_claim::claim(&doc, ROW, "subagent:widget-2", 600)
            .expect_err("the open review blocks the claim");
        let refused = refused.to_string();
        assert!(refused.contains("#promptpressure"), "{refused}");
        assert!(refused.contains("prompt-blocker-widget"), "{refused}");
        assert!(
            refused.contains("land the export commit on fix/widget"),
            "{refused}"
        );
        agent_doc_queue_io::queue_claim::claim(&doc, "ship the other crate", "subagent:other", 600)
            .expect("a different row stays claimable");
        let brief =
            agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_item(&doc, ROW)
                .unwrap();
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("Prior prompt attempt")
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("review #prompt-blocker-widget")
        );
        assert!(!brief.subagent_prompt_preamble.contains("signing key"));
    }

    /// `#humanfollowupowner`: the report line names how many queued rows wait
    /// on the operator, so the coordinator does not dispatch them.
    #[test]
    fn report_line_names_human_owned_followups() {
        let followups = agent_doc_queue::handback_followups::HandbackFollowups {
            queue_id: Some("main".to_string()),
            inserted: vec![
                "follow up on an action item from `alpha`: agent: fix the typo".to_string(),
                "follow up on an action item from `alpha`: human (tmstorm): approve PR 9 [operator-verify]"
                    .to_string(),
            ],
            operator_owned: 1,
            ..agent_doc_queue::handback_followups::HandbackFollowups::default()
        };
        let text =
            describe_handback_followups(Path::new("tasks/bugs.md"), "do [#alpha]", &followups);
        assert!(text.contains("Queued 2 action item(s)"), "{text}");
        assert!(
            text.contains("1 of them are human-owned: they carry `[operator-verify]`"),
            "{text}"
        );
        let none = agent_doc_queue::handback_followups::HandbackFollowups {
            operator_owned: 0,
            ..followups
        };
        let text = describe_handback_followups(Path::new("tasks/bugs.md"), "do [#alpha]", &none);
        assert!(!text.contains("human-owned"), "{text}");
    }

    /// `#finishactionable`: the report line tells the coordinator to do the
    /// agent-owned action items now, in this turn, and lists exactly those
    /// rows (placed or refused); human-owned rows are not listed, and a
    /// hand-back with only human-owned items prints no such instruction.
    #[test]
    fn report_line_directs_agent_action_items_to_be_done_this_turn() {
        let install = "follow up on an action item from queue item “complete the work for this tree”: agent: install `a12e285c55` (`make install` in the agent-doc submodule) and verify `agent-doc --version`.";
        let ci = "follow up on an action item from queue item “complete the work for this tree”: agent: check that agent-doc CI run 44946906648 for `a12e285c55` on `main` finished green.";
        let operator = "follow up on an action item from queue item “complete the work for this tree”: operator: apply edits 1–3 above in the editor. [operator-verify]";
        let followups = agent_doc_queue::handback_followups::HandbackFollowups {
            queue_id: Some("main".to_string()),
            inserted: vec![install.to_string(), operator.to_string()],
            operator_owned: 1,
            refused: Some("queue follow-up origin is ambiguous".to_string()),
            refused_origin: Some("complete the work for this tree".to_string()),
            refused_items: vec![ci.to_string()],
            ..agent_doc_queue::handback_followups::HandbackFollowups::default()
        };
        let text = describe_handback_followups(
            Path::new("tasks/bugs.md"),
            "complete the work for this tree",
            &followups,
        );
        assert!(
            text.contains(
                "Do these 2 agent-owned action item(s) NOW, in this turn, before closeout (#finishactionable)"
            ),
            "{text}"
        );
        assert!(
            text.contains("agent-doc queue consume tasks/bugs.md --ack-text"),
            "{text}"
        );
        assert!(text.contains(&format!("\n- {install}")), "{text}");
        assert!(text.contains(&format!("\n- {ci}")), "{text}");
        assert!(!text.contains(&format!("\n- {operator}")), "{text}");

        let human_only = agent_doc_queue::handback_followups::HandbackFollowups {
            queue_id: Some("main".to_string()),
            inserted: vec![operator.to_string()],
            operator_owned: 1,
            ..agent_doc_queue::handback_followups::HandbackFollowups::default()
        };
        let text = describe_handback_followups(
            Path::new("tasks/bugs.md"),
            "complete the work for this tree",
            &human_only,
        );
        assert!(!text.contains("#finishactionable"), "{text}");
    }

    /// `#handbackrefusedcommandexact`: the report line's refused-placement
    /// command names the real origin and each unplaced row, and a marker-only
    /// refusal does not offer a command that would insert the marker.
    #[test]
    fn refused_placement_report_prints_a_command_that_can_be_run_as_printed() {
        let items = vec![
            "follow up on an action item from `alpha`: >512-commit documents can miss answers"
                .to_string(),
            "follow up on an action item from `alpha`: the gutter does not show the stamp"
                .to_string(),
        ];
        let followups = agent_doc_queue::handback_followups::HandbackFollowups {
            queue_id: Some("main".to_string()),
            refused: Some(
                "queue follow-up origin \"do [#alpha]\" is ambiguous in queue id `main`"
                    .to_string(),
            ),
            refused_origin: Some("do [#alpha]".to_string()),
            refused_items: items.clone(),
            ..agent_doc_queue::handback_followups::HandbackFollowups::default()
        };
        let text =
            describe_handback_followups(Path::new("tasks/bugs.md"), "do [#alpha]", &followups);
        let command = text
            .lines()
            .find(|line| line.starts_with("agent-doc queue follow-up"))
            .unwrap_or_else(|| panic!("no runnable command:\n{text}"));
        assert_eq!(
            command,
            agent_doc_queue::handback_followups::refused_placement_command(
                "tasks/bugs.md",
                "main",
                "do [#alpha]",
                &items,
            )
        );
        assert!(
            !command.contains("<origin>") && !command.contains("<text>"),
            "{command}"
        );
        assert!(command.contains("--after 'do [#alpha]'"), "{command}");
        assert!(command.contains("--item "), "{command}");

        let withheld = agent_doc_queue::handback_followups::HandbackFollowups {
            queue_id: Some("main".to_string()),
            refused: Some(
                "1 action item(s) carry an HTML comment or component marker, which a live queue row must not hold"
                    .to_string(),
            ),
            ..agent_doc_queue::handback_followups::HandbackFollowups::default()
        };
        let text =
            describe_handback_followups(Path::new("tasks/bugs.md"), "do [#alpha]", &withheld);
        assert!(text.contains("NOT queued"), "{text}");
        assert!(!text.contains("queue follow-up"), "{text}");
    }

    #[test]
    fn report_is_immediate_but_keeps_claim_until_release() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sdk-feedback\nagent_doc_format: template\n---\n\n",
                "## Status <!-- agent:status -->\nOperator status.\n<!-- /agent:status -->\n",
                "## Active <!-- agent:active priority -->\n\n<!-- /agent:active -->\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- finish sdk.md with deterministic tests\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            "finish sdk.md with deterministic tests",
            "subagent:sdk-feedback",
            600,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "finish sdk.md with deterministic tests",
            "subagent:sdk-feedback",
            "worker:sdk-feedback",
            600,
            1,
        )
        .unwrap();

        let first = report_complete(
            &doc,
            "finish sdk.md with deterministic tests",
            Some(1),
            Some(QueueReportState::PublishedLocal),
            None,
            "Implemented SDK support.\n\nTests: 12 passed.",
        )
        .unwrap();
        assert!(first.appended);
        assert!(!first.projection_pending);
        assert_eq!(
            first.claim.publication_state,
            Some(QueueReportState::PublishedLocal)
        );
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims[0]
                .publication_state,
            Some(QueueReportState::PublishedLocal)
        );
        let reported_content = fs::read_to_string(&doc).unwrap();
        assert!(
            reported_content.contains("📤 Queue handoff: 1 reported item")
                && reported_content.contains("- **Needs integration** (1)")
                && reported_content.contains("Operator status."),
            "{reported_content}"
        );
        assert!(
            reported_content.contains("finish sdk.md with deterministic tests ⟦published+local⟧"),
            "{reported_content}"
        );
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .len(),
            1,
            "completion feedback must not release the integration guard"
        );
        let refreshed = agent_doc_queue_io::queue_claim::refresh(
            &doc,
            "finish sdk.md with deterministic tests",
            "subagent:sdk-feedback",
            1_200,
        )
        .unwrap();
        assert_ne!(refreshed.expires_at_secs, first.claim.expires_at_secs);
        let second = report_complete(
            &doc,
            "finish sdk.md with deterministic tests",
            Some(1),
            None,
            None,
            "Implemented SDK support.\n\nTests: 12 passed.",
        )
        .unwrap();
        assert!(
            !second.appended,
            "the same completion must dedupe across TTL refresh"
        );
        assert!(!second.replaced);

        let updated = report_complete(
            &doc,
            "finish sdk.md with deterministic tests",
            Some(1),
            None,
            None,
            "Implemented SDK support.\n\nTests: 18 passed.",
        )
        .unwrap();
        assert!(!updated.appended);
        assert!(
            updated.replaced,
            "a changed report for the same attempt must replace the visible cell"
        );
        let updated_content = fs::read_to_string(&doc).unwrap();
        assert!(updated_content.contains("> Tests: 18 passed."));
        assert!(!updated_content.contains("> Tests: 12 passed."));
        assert_eq!(
            updated_content
                .matches("### Re: Subagent completed")
                .count(),
            1
        );
        let receipt = agent_doc_queue_io::claim_release_receipt::load(&doc)
            .unwrap()
            .receipts
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            receipt.response.unwrap().body,
            "Implemented SDK support.\n\nTests: 18 passed."
        );

        let released = agent_doc_queue_io::queue_claim::release(
            &doc,
            "finish sdk.md with deterministic tests",
        )
        .unwrap()
        .unwrap();
        assert_eq!(released.owner, "subagent:sdk-feedback");
        assert!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches("### Re: Subagent completed").count(), 1);
        assert!(
            content.find("### Re: Subagent completed").unwrap()
                < content.find("<!-- agent:boundary:keep -->").unwrap()
        );
    }

    #[test]
    fn projection_repair_changes_only_visible_marker_and_keeps_reported_claim() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue subagents -->\n",
                "- 🚧 [#subagent:sdk#1] finish sdk\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "finish sdk", "subagent:sdk", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "finish sdk",
            "subagent:sdk",
            "worker:sdk",
            600,
            1,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::report_attempt(&doc, "finish sdk", Some(1)).unwrap();

        assert!(
            repair_claim_projection(&doc, &crate::CliQueueCommandEffects).unwrap(),
            "the stale running marker should be repaired",
        );
        let repaired = fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("- ☑️ [#subagent:sdk#1] finish sdk"));
        let claims = agent_doc_queue_io::queue_claim::load_ledger(&doc)
            .unwrap()
            .claims;
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported,
        );
    }

    /// `#markdrift`: a retained write with no editor delivery receipt must not
    /// bail before the marker reconcile, and must not require `--force-disk`.
    #[test]
    fn projection_repair_paints_reported_while_a_save_receipt_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        let original = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue subagents -->\n",
            "- 🚧 [#subagent:sdk#1] finish sdk\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, original).unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "finish sdk", "subagent:sdk", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "finish sdk",
            "subagent:sdk",
            "worker:sdk",
            600,
            1,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::report_attempt(&doc, "finish sdk", Some(1)).unwrap();
        let retained = format!("{original}<!-- retained without a save receipt -->\n");
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &doc,
            original,
            &retained,
            "queue_claim_marker_drift",
            agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .expect("stage a retained write with no delivery receipt");
        assert!(
            agent_doc_document_realtime_io::pending_document_write(&doc).is_some(),
            "the receipt gap is the condition that used to refuse repair"
        );

        assert!(
            repair_claim_projection(&doc, &crate::CliQueueCommandEffects).unwrap(),
            "marker reconcile must proceed without a native-save receipt",
        );
        let repaired = fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("- ☑️ [#subagent:sdk#1] finish sdk"),
            "{repaired}"
        );
        assert!(!repaired.contains('🚧'), "{repaired}");
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims[0]
                .state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported,
        );
    }

    #[test]
    fn managed_state_repair_reaps_only_proven_orphans_and_repaints_a_stale_reported_marker() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        let doc = dir.path().join("sdk.md");
        let stale = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:old",
            "already integrated",
            "old-key",
            "Done in an earlier cycle.",
        )
        .unwrap();
        let original = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{stale}\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:run subagents -->\n",
                "- ☑️ [#subagent:live#1] still running\n",
                "<!-- /agent:run -->\n",
            ),
            stale = stale,
        );
        fs::write(&doc, &original).unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "still running", "subagent:live", 600)
            .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "still running",
            "subagent:live",
            "worker:live",
            600,
            1,
        )
        .unwrap();

        let keys = std::collections::HashSet::new();
        let audit = repair_managed_state(&doc, false, &keys).unwrap();
        assert_eq!(audit.stale.stale_cells.len(), 1);
        assert_eq!(audit.stale.stale_cells[0].key, "old-key");
        // `#managedlistnodrift`: the visible `☑️` is output; the durable claim
        // says the attempt is still running, so the audit reports drift.
        assert!(audit.projection_drift);
        assert!(!audit.projection_applied);
        assert_eq!(fs::read_to_string(&doc).unwrap(), original);

        let repaired = repair_managed_state(&doc, true, &keys).unwrap();
        assert_eq!(repaired.stale.stale_cells.len(), 1);
        assert!(repaired.stale.applied);
        assert!(repaired.projection_applied);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("old-key"), "{content}");
        assert!(
            content.contains("- 🚧 [#subagent:live#1] still running"),
            "{content}"
        );

        let repeated = repair_managed_state(&doc, true, &keys).unwrap();
        assert!(repeated.stale.stale_cells.is_empty());
        assert!(!repeated.projection_drift);
        assert!(!repeated.projection_applied);
        assert_eq!(fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn cancelled_generation_cannot_report_or_attach_to_same_text_readd() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        let item = "finish sdk";
        fs::write(
            &doc,
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue subagents -->\n",
                "- finish sdk\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        let (_, first) =
            agent_doc_queue_io::queue_claim::claim(&doc, item, "subagent:first", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:first",
            "worker:first",
            600,
            first.generation,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::cancel_removed_attempt(&doc, item, Some(first.generation))
            .unwrap();
        let cancelled = report_complete(
            &doc,
            item,
            Some(first.generation),
            None,
            None,
            "worker one completed",
        )
        .expect_err("a tombstoned attempt cannot manufacture completion feedback")
        .to_string();
        // `#reportcompletenomatch`: the refusal names the cancelled attempt
        // instead of exiting quietly.
        assert!(
            cancelled.contains("matched no subagent claim for \"finish sdk\" generation 1"),
            "{cancelled}"
        );
        assert!(
            cancelled.contains("cancelled attempts for this item (1):")
                && cancelled.contains("owner=subagent:first generation=1"),
            "{cancelled}"
        );

        let (_, replacement) =
            agent_doc_queue_io::queue_claim::claim(&doc, item, "subagent:replacement", 600)
                .unwrap();
        assert_eq!(replacement.generation, first.generation + 1);
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:replacement",
            "worker:replacement",
            600,
            replacement.generation,
        )
        .unwrap();
        let stale = report_complete(
            &doc,
            item,
            Some(first.generation),
            None,
            None,
            "late stale result",
        )
        .unwrap_err();
        assert!(stale.to_string().contains("stale queue claim generation"));
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("agent-doc-subagent-completion"),
            "stale completion must not enter the outbox"
        );
    }

    #[test]
    fn attached_editor_reports_are_durable_bounded_and_concurrency_safe() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        let items = ["finish sdk", "finish dev", "finish pane", "finish api"];
        let queue = items
            .iter()
            .map(|item| format!("- {item}\n"))
            .collect::<String>();
        fs::write(
            &doc,
            format!(
                concat!(
                    "---\nagent_doc_session: sdk-feedback-live\nagent_doc_format: template\n---\n\n",
                    "<!-- agent:exchange patch=append subagents -->\n",
                    "### Re: prior — gpt-5\n\nDone.\n",
                    "Operator edit that must survive.\n",
                    "<!-- agent:boundary:keep -->\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue subagents -->\n",
                    "{}",
                    "<!-- /agent:queue -->\n",
                ),
                queue,
            ),
        )
        .unwrap();
        for item in items {
            let owner = format!("subagent:{}", item.replace(' ', "-"));
            agent_doc_queue_io::queue_claim::claim(&doc, item, &owner, 600).unwrap();
            agent_doc_queue_io::queue_claim::start_attempt(
                &doc,
                item,
                &owner,
                &format!("worker:{}", item.replace(' ', "-")),
                600,
                1,
            )
            .unwrap();
        }

        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: "completion-feedback-editor".to_string(),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "completion-feedback-editor")
            .unwrap();
        assert!(
            agent_doc_crdt_relay_io::crdt_authority_for_file(&doc).editor_attached(),
            "fixture must exercise the retained active-editor branch"
        );

        let threads = items
            .into_iter()
            .map(|item| {
                let doc = doc.clone();
                std::thread::spawn(move || {
                    report_complete(
                        &doc,
                        item,
                        Some(1),
                        None,
                        None,
                        "concurrent worker completed",
                    )
                    .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.iter().all(|outcome| outcome.appended));
        // Found by the repositoryreportcell check run: the fixture's editor
        // never acknowledges, so a report that waited for projection could
        // only return after the projection landed. Every report returning with
        // its intent still pending, and the composed target below still
        // unprojected, proves they returned on durable acceptance. A
        // wall-clock bound measured machine load instead (13.6 s at load
        // average ~124 on 2026-11-05).
        assert!(outcomes.iter().all(|outcome| outcome.projection_pending));

        let pending = agent_doc_document_realtime_io::pending_document_write(&doc)
            .expect("active-editor feedback must have one durable composed target");
        assert!(
            pending
                .target_content
                .contains("Operator edit that must survive.")
        );
        assert_eq!(
            pending
                .target_content
                .matches("### Re: Subagent completed")
                .count(),
            items.len(),
            "the retained successor must contain every concurrent completion exactly once"
        );
        assert_eq!(
            pending
                .target_content
                .matches("<!-- agent-doc-subagent-completion key=")
                .count(),
            items.len()
        );
        agent_doc_lint_io::validate_structure_on_content(&doc, &pending.target_content)
            .expect("multiple concurrent completion artifacts must pass session-check integrity");
        assert_eq!(
            fs::read_to_string(&doc)
                .unwrap()
                .matches("### Re: Subagent completed")
                .count(),
            0,
            "the one-shot command must not force disk while the editor owns projection"
        );
        agent_doc_crdt_relay_io::deregister_replica_for_file(&doc, "completion-feedback-editor")
            .unwrap();
    }

    #[test]
    fn later_report_rebases_retained_feedback_over_concurrent_editor_queue_edit() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        let initial = concat!(
            "---\nagent_doc_session: sdk-feedback-rebase\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- first worker\n",
            "- second worker\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, initial).unwrap();
        for (item, owner, worker) in [
            ("first worker", "subagent:first", "worker:first"),
            ("second worker", "subagent:second", "worker:second"),
        ] {
            agent_doc_queue_io::queue_claim::claim(&doc, item, owner, 600).unwrap();
            agent_doc_queue_io::queue_claim::start_attempt(&doc, item, owner, worker, 600, 1)
                .unwrap();
        }

        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: "completion-feedback-rebase-editor".to_string(),
            }]);
        let (client_id, _bootstrap) = agent_doc_crdt_relay_io::register_replica_for_file(
            &doc,
            "completion-feedback-rebase-editor",
        )
        .unwrap()
        .expect("editor replica attaches");

        let first =
            report_complete(&doc, "first worker", Some(1), None, None, "first result").unwrap();
        assert!(first.projection_pending);

        agent_doc_crdt_relay_io::with_hub(&doc, |hub| {
            let before_edit = hub.canonical_text();
            let insertion_byte = before_edit
                .find("<!-- /agent:queue -->")
                .expect("queue closing marker");
            let insertion_char = before_edit[..insertion_byte].chars().count() as u32;
            hub.local_edit(
                client_id,
                insertion_char,
                0,
                "- operator added while reports were arriving\n",
            )
            .unwrap();
        })
        .unwrap();
        let authoritative = agent_doc_document_realtime_io::try_resolve_current_document_content(
            &doc,
            "completion_feedback_concurrent_edit_test",
        )
        .unwrap();
        assert!(
            authoritative.contains("operator added while reports were arriving"),
            "fixture must expose the concurrent editor cut:\n{authoritative}",
        );

        let second =
            report_complete(&doc, "second worker", Some(1), None, None, "second result").unwrap();
        assert!(second.projection_pending);
        let pending = agent_doc_document_realtime_io::pending_document_write(&doc)
            .expect("both reports remain in one retained successor");
        assert!(
            pending
                .target_content
                .contains("operator added while reports were arriving"),
            "the concurrent queue edit must survive:\n{}",
            pending.target_content,
        );
        assert_eq!(
            pending
                .target_content
                .matches("### Re: Subagent completed")
                .count(),
            2,
        );
        assert!(
            pending
                .target_content
                .contains("- ☑️ [#subagent:first#1] first worker")
        );
        assert!(
            pending
                .target_content
                .contains("- ☑️ [#subagent:second#1] second worker")
        );

        agent_doc_crdt_relay_io::deregister_replica_for_file(
            &doc,
            "completion-feedback-rebase-editor",
        )
        .unwrap();
    }

    #[test]
    fn contended_cross_process_owner_is_strictly_bounded() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue subagents -->\n",
                "- finish sdk\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "finish sdk", "subagent:sdk-feedback", 600)
            .unwrap();
        let _held = acquire_completion_lock(
            &doc,
            "hold the fixture lock",
            ProgressLockWait {
                stall_window: Duration::ZERO,
                max_wait: Duration::ZERO,
            },
        )
        .expect("fixture must acquire completion lock");
        // `#completionlockload`: the bound is structural, not a wall-clock
        // assertion. The fixture owner never hands off, so the waiter refuses
        // after exactly one stall window however slow the machine is, and the
        // hour-long ceiling proves the refusal came from the stall, not the cap.
        let error = report_complete_with_lock_wait(
            &doc,
            "finish sdk",
            Some(1),
            None,
            None,
            "worker completed",
            ProgressLockWait {
                stall_window: Duration::from_millis(50),
                max_wait: Duration::from_secs(3600),
            },
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("waiting to publish subagent completion feedback")
                && message.contains("made no progress for 50ms")
                && message.contains("over 1 window(s), 0 owner handoff(s)")
                && message.contains("no document mutation was attempted"),
            "{message}"
        );
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("Subagent completed")
        );
    }
}
