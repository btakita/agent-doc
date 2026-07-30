use std::path::Path;

use anyhow::Result;

pub use agent_doc_document_realtime_io::{
    RUNTIME_PIPELINE_FRONTMATTER_EFFECTS as PIPELINE_FRONTMATTER_EFFECTS,
    RuntimePipelineFrontmatterEffects as PipelineFrontmatterEffects,
};

pub struct RuntimeRepairIoEffects;

pub static REPAIR_IO_EFFECTS: RuntimeRepairIoEffects = RuntimeRepairIoEffects;

struct RecoveryCloseoutOwnerGuard {
    file: std::path::PathBuf,
    cycle_id: String,
    owner_id: String,
}

impl Drop for RecoveryCloseoutOwnerGuard {
    fn drop(&mut self) {
        if let Err(err) =
            agent_doc_controller_io::project_controller::release_closeout_owner_for_file(
                &self.file,
                &self.cycle_id,
                &self.owner_id,
                "session_check_recovery_finished",
            )
        {
            agent_doc_ops_log_io::log_op(
                &self.file,
                &format!(
                    "closeout_owner_release_failed file={} cycle_id={} owner_id={} err={err}",
                    self.file.display(),
                    self.cycle_id,
                    self.owner_id,
                ),
            );
        }
    }
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub use agent_doc_controller_io::project_controller::CloseoutCycleWaitOutcome;

/// Subscribe to the controller-owned closeout projection for one bounded
/// recovery interval. The controller returns immediately when the cycle closes
/// or its request-scoped owner guard releases.
pub fn await_closeout_cycle_progress(
    file: &Path,
    cycle_id: &str,
) -> anyhow::Result<CloseoutCycleWaitOutcome> {
    agent_doc_controller_io::project_controller::await_closeout_cycle_progress_for_file(
        file,
        cycle_id,
        std::time::Duration::from_secs(30),
    )
}

fn try_claim_recovery_closeout_owner(
    file: &std::path::Path,
    cycle_id: &str,
) -> anyhow::Result<Result<RecoveryCloseoutOwnerGuard, String>> {
    use agent_doc_controller_io::project_controller as controller;

    let now_secs = current_epoch_secs();
    let owner_id = controller::new_closeout_owner_id("session-check-recovery");
    match controller::claim_closeout_owner_for_file(
        file,
        controller::CloseoutOwnerClaimRequest {
            expected_cycle_id: Some(cycle_id.to_string()),
            owner_id: owner_id.clone(),
            owner_pid: std::process::id(),
            role: controller::CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY.to_string(),
            now_secs,
            // `#closeoutwaitchurn`: a status-only probe must not hold the
            // write-closeout lease. See `closeout_owner_lease_secs`.
            lease_secs: controller::closeout_owner_lease_secs(
                controller::CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY,
            ),
            allow_dead_owner_takeover: true,
        },
    )? {
        controller::CloseoutOwnerClaimOutcome::Acquired(_) => Ok(Ok(RecoveryCloseoutOwnerGuard {
            file: file.to_path_buf(),
            cycle_id: cycle_id.to_string(),
            owner_id,
        })),
        controller::CloseoutOwnerClaimOutcome::HeldByOther(owner) => Ok(Err(format!(
            "foreground closeout operation is already in progress: owner {} pid={} role={}; \
             recovery follows the cycle's terminal state (lease stopgap expires at {})",
            owner.owner_id, owner.owner_pid, owner.role, owner.expires_secs
        ))),
        controller::CloseoutOwnerClaimOutcome::CycleSuperseded => {
            Ok(Err("captured closeout cycle was superseded".to_string()))
        }
    }
}

impl agent_doc_repair_io::RepairIoEffects for RuntimeRepairIoEffects {
    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String> {
        repair_atomic_write_if_current(file, content, expected_current, source)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn apply_closeout_recovery_mutation(
        &self,
        file: &Path,
        mutation: agent_doc_flow_io::closeout::CloseoutRecoveryMutation<'_>,
    ) -> Result<()> {
        agent_doc_flow_io::closeout::apply_closeout_recovery_mutation(
            file,
            mutation,
            &closeout_effects(),
        )
    }
}

impl agent_doc_repair_io::RepairTemplateWriteEffects for RuntimeRepairIoEffects {
    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String> {
        repair_atomic_write_if_current(file, content, expected_current, source)
    }

    fn repair_response_prompt_order_for_file(
        &self,
        content: &str,
        known_response: Option<&str>,
        file: &Path,
        fallback_snapshot: Option<&str>,
    ) -> Result<Option<String>> {
        agent_doc_template_io::repair_response_prompt_order_for_file(
            content,
            known_response,
            file,
            fallback_snapshot,
        )
    }

    fn normalize_template_structure_or_fail_preserving(
        &self,
        content: &str,
        file: &Path,
        prompt_input: Option<&str>,
    ) -> Result<String> {
        agent_doc_template_io::normalize_template_structure_or_fail_preserving(
            content,
            file,
            prompt_input,
        )
    }
}

fn repair_atomic_write_if_current(
    file: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<String> {
    let authoritative =
        agent_doc_document_realtime_io::atomic_repair_write_if_current_through_authority(
            file,
            content,
            expected_current,
            source,
        )?;
    let disk = agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)?;
    anyhow::ensure!(
        disk == authoritative,
        "{source}: repair write for {} left a stale disk projection (expected_hash={}, disk_hash={}); refusing snapshot/save closeout",
        file.display(),
        agent_doc_hash::content_hash(&authoritative),
        agent_doc_hash::content_hash(&disk),
    );
    Ok(authoritative)
}

pub struct RuntimeSessionCheckEffects;

/// Closed transition identities emitted while resuming one durable captured
/// response.
///
/// These values select authority observations and durable effects; they are not
/// prose log labels. Keep the string encoding at the existing cross-crate
/// boundary, while making the policy owner choose from a finite set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturedFinalizeSource {
    FalseStaleRetirementSettlement,
    CommittedEffectSinkCurrent,
    CommittedEffectSinkSettlement,
    RetainedProjectionSettlement,
    ReconciliationCurrent,
    ReconciliationDisk,
    NativeSaveWithoutRetainedIntent,
    NativeSaveWithoutRetainedIntentSettled,
    RetainedWriteAppliedCurrent,
    RetainedWriteAppliedDisk,
}

impl CapturedFinalizeSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::FalseStaleRetirementSettlement => {
                "session_check_false_stale_capture_retirement_settlement"
            }
            Self::CommittedEffectSinkCurrent => {
                "session_check_committed_capture_effect_sink_current"
            }
            Self::CommittedEffectSinkSettlement => {
                "session_check_committed_capture_effect_sink_settlement"
            }
            Self::RetainedProjectionSettlement => {
                "session_check_retained_captured_projection_settlement"
            }
            Self::ReconciliationCurrent => {
                "session_check_write_applied_capture_reconciliation_current"
            }
            Self::ReconciliationDisk => "session_check_write_applied_capture_reconciliation_disk",
            Self::NativeSaveWithoutRetainedIntent => {
                "session_check_capture_without_retained_intent_native_save"
            }
            Self::NativeSaveWithoutRetainedIntentSettled => {
                "session_check_capture_without_retained_intent_native_save_settled"
            }
            Self::RetainedWriteAppliedCurrent => "session_check_retained_capture_write_applied",
            Self::RetainedWriteAppliedDisk => "session_check_retained_capture_disk",
        }
    }
}

fn cycle_event_is(state_event: &str, expected: &str) -> bool {
    state_event == expected
        || state_event
            .split_ascii_whitespace()
            .any(|field| field.strip_prefix("reason=") == Some(expected))
}

fn exact_legacy_pending_only_reap(
    head_content: &str,
    current_content: &str,
    pending_done_ids: &[String],
) -> Result<bool> {
    if pending_done_ids.is_empty() || head_content == current_content {
        return Ok(false);
    }
    let mut remaining = pending_done_ids
        .iter()
        .map(|id| id.trim().trim_start_matches('#').to_string())
        .filter(|id| !id.is_empty())
        .collect::<std::collections::HashSet<_>>();
    if remaining.is_empty() {
        return Ok(false);
    }
    let mut projected = String::with_capacity(head_content.len());
    for line in head_content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let matched = trimmed
            .starts_with("- [ ]")
            .then(|| {
                remaining
                    .iter()
                    .find(|id| trimmed.contains(&format!("[#{id}]")))
                    .cloned()
            })
            .flatten();
        if let Some(id) = matched {
            remaining.remove(&id);
        } else {
            projected.push_str(line);
        }
    }
    if !remaining.is_empty() {
        return Ok(false);
    }
    let (projected, _) =
        agent_doc_queue::queue_consume::mark_queue_prompts_completed_by_done_ids_in_content(
            &projected,
            pending_done_ids,
        )?;
    Ok(projected == current_content)
}

fn infer_pending_only_commit_target_from_retained_projection(
    head_content: &str,
    phase: agent_doc_turn::CyclePhase,
    recorded_target_hash: Option<&str>,
    pending_done_ids: &[String],
    pending: Option<(&str, &str)>,
) -> Result<Option<String>> {
    if recorded_target_hash.is_some()
        || phase != agent_doc_turn::CyclePhase::Committed
        || pending_done_ids.is_empty()
    {
        return Ok(None);
    }
    let Some((retained_target_hash, retained_target_content)) = pending else {
        return Ok(None);
    };
    let target_hash = agent_doc_hash::content_hash(retained_target_content);
    anyhow::ensure!(
        retained_target_hash.eq_ignore_ascii_case(&target_hash),
        "retained pending-only projection target hash does not match its content"
    );
    if !exact_legacy_pending_only_reap(head_content, retained_target_content, pending_done_ids)? {
        return Ok(None);
    }
    Ok(Some(target_hash))
}

enum PendingOnlyCommitDecision {
    NotReady,
    Eligible {
        target_hash: String,
        migrated_legacy_intent: bool,
    },
    Superseded {
        target_hash: String,
    },
    InvalidAtomicCompletion {
        target_hash: String,
    },
}

fn pending_only_commit_target(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    authority_content: &str,
    disk_content: &str,
) -> Result<PendingOnlyCommitDecision> {
    if authority_content != disk_content {
        return Ok(PendingOnlyCommitDecision::NotReady);
    }
    let authority_hash = agent_doc_hash::content_hash(authority_content);
    if let Some(target_hash) = state.pending_only_commit_target_hash.as_deref() {
        if authority_hash != target_hash {
            return Ok(PendingOnlyCommitDecision::Superseded {
                target_hash: target_hash.to_string(),
            });
        }
        if !state.pending_done_ids.is_empty() {
            let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
                return Ok(PendingOnlyCommitDecision::NotReady);
            };
            if !agent_doc_queue::queue_consume::done_ids_have_atomic_queue_completion(
                &head_content,
                authority_content,
                &state.pending_done_ids,
            )? {
                return Ok(PendingOnlyCommitDecision::InvalidAtomicCompletion {
                    target_hash: target_hash.to_string(),
                });
            }
        }
        return Ok(PendingOnlyCommitDecision::Eligible {
            target_hash: target_hash.to_string(),
            migrated_legacy_intent: false,
        });
    }
    // 0.35.94/0.35.95 could settle a retained `--backlog-only --done`
    // projection without persisting the downstream commit identity. Migrate
    // only the exact historical shape: a committed cycle records the done ids,
    // HEAD differs from the converged editor/disk cut solely by deleting those
    // unchecked rows, and no additions or other removals are present.
    if state.phase != agent_doc_turn::CyclePhase::Committed || state.pending_done_ids.is_empty() {
        return Ok(PendingOnlyCommitDecision::NotReady);
    }
    let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(PendingOnlyCommitDecision::NotReady);
    };
    if exact_legacy_pending_only_reap(&head_content, authority_content, &state.pending_done_ids)? {
        Ok(PendingOnlyCommitDecision::Eligible {
            target_hash: authority_hash,
            migrated_legacy_intent: true,
        })
    } else {
        Ok(PendingOnlyCommitDecision::NotReady)
    }
}

pub fn session_check_effects() -> RuntimeSessionCheckEffects {
    RuntimeSessionCheckEffects
}

impl agent_doc_session_check_io::SessionCheckEffects for RuntimeSessionCheckEffects {
    fn closeout_recovery_hint(&self, file: &Path) -> String {
        closeout_recovery_hint(file)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn atomic_repair_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String> {
        repair_atomic_write_if_current(file, content, expected_current, source)
    }

    fn settle_committed_projection(
        &self,
        file: &Path,
        committed_content: &str,
        expected_current: &str,
    ) -> Result<()> {
        agent_doc_document_realtime_io::settle_committed_projection_if_current_through_authority(
            file,
            committed_content,
            expected_current,
            "session_check_committed_projection_settlement",
        )
    }

    fn settle_retained_committed_projection(
        &self,
        file: &Path,
        committed_content: &str,
        expected_disk: &str,
    ) -> Result<bool> {
        agent_doc_document_realtime_io::settle_retained_committed_projection_through_authority(
            file,
            committed_content,
            expected_disk,
            "session_check_retained_committed_projection_settlement",
        )
    }

    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &Path,
    ) -> Result<Option<&'static str>> {
        agent_doc_repair_io::repair_committed_historical_snapshot_drift(file)
    }

    fn recover_missing_commit_boundary(
        &self,
        file: &Path,
        event: &str,
    ) -> Result<Option<&'static str>> {
        agent_doc_repair_io::recover_missing_commit_boundary(&REPAIR_IO_EFFECTS, file, event)
    }

    fn recover_retained_document_write(&self, file: &Path) -> Result<bool> {
        agent_doc_document_realtime_io::recover_retained_document_write_before_new_cycle(
            file,
            agent_doc_document_realtime_io::RetainedWriteCycleBoundary::SessionCheck,
        )
    }

    fn retained_document_write_blocks(&self, file: &Path) -> bool {
        agent_doc_document_realtime_io::retained_write_blocks_session_closeout(
            file,
            agent_doc_document_realtime_io::RetainedWriteCycleBoundary::SessionCheck.gate_source(),
        )
    }

    fn resume_retained_pending_only_commit(&self, file: &Path) -> Result<bool> {
        let Some(mut state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
            return Ok(false);
        };
        if state.pending_only_commit_target_hash.is_none()
            && state.phase == agent_doc_turn::CyclePhase::Committed
            && !state.pending_done_ids.is_empty()
            && let Some(head_content) = agent_doc_git_io::revision::show_head(file)?
        {
            let pending = agent_doc_document_realtime_io::pending_document_write(file);
            if let Some(target_hash) = infer_pending_only_commit_target_from_retained_projection(
                &head_content,
                state.phase,
                state.pending_only_commit_target_hash.as_deref(),
                &state.pending_done_ids,
                pending.as_ref().map(|pending| {
                    (
                        pending.target_hash.as_str(),
                        pending.target_content.as_str(),
                    )
                }),
            )? {
                agent_doc_cycle_state_io::record_pending_only_commit_target(file, &target_hash)?;
                state.pending_only_commit_target_hash = Some(target_hash.clone());
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_only_commit_continuation_inferred file={} target_hash={} proof=retained_exact_done_reap_with_atomic_queue_completion",
                        file.display(),
                        target_hash,
                    ),
                );
            }
        }
        let mut authority_content =
            agent_doc_document_realtime_io::try_resolve_current_document_content(
                file,
                "pending_only_commit_resume_authority",
            )?;
        let mut disk_content =
            agent_doc_document_realtime_io::resolve_disk_current_document_content(
                file,
                "pending_only_commit_resume_disk",
            )?;
        let (target_hash, migrated_legacy_intent) = match pending_only_commit_target(
            file,
            &state,
            &authority_content,
            &disk_content,
        )? {
            PendingOnlyCommitDecision::NotReady => return Ok(false),
            PendingOnlyCommitDecision::Eligible {
                target_hash,
                migrated_legacy_intent,
            } => (target_hash, migrated_legacy_intent),
            PendingOnlyCommitDecision::Superseded { target_hash } => {
                let retained_matches = agent_doc_document_realtime_io::pending_document_write(file)
                    .is_some_and(|pending| pending.target_hash.eq_ignore_ascii_case(&target_hash));
                if retained_matches
                        && !agent_doc_document_realtime_io::retire_retained_projection_superseded_by_authority(
                            file,
                            &target_hash,
                            "pending_only_commit_superseded",
                        )?
                    {
                        return Ok(false);
                    }
                anyhow::ensure!(
                    agent_doc_cycle_state_io::clear_pending_only_commit_intent(file, &target_hash,)?,
                    "pending-only commit continuation for {} lost its superseded target identity before retirement",
                    file.display(),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_only_commit_continuation_superseded file={} retired_target_hash={} authoritative_hash={} authority=editor_crdt disk_exact=true",
                        file.display(),
                        target_hash,
                        agent_doc_hash::content_hash(&authority_content),
                    ),
                );
                return Ok(true);
            }
            PendingOnlyCommitDecision::InvalidAtomicCompletion { target_hash } => {
                let head_content =
                        agent_doc_git_io::revision::show_head(file)?.ok_or_else(|| {
                            anyhow::anyhow!(
                                "cannot roll back invalid pending-only completion for {} without readable HEAD content",
                                file.display(),
                            )
                        })?;
                agent_doc_document_realtime_io::settle_committed_projection_if_current_through_authority(
                        file,
                        &head_content,
                        &authority_content,
                        "pending_only_commit_invalid_atomic_completion_rollback",
                    )?;
                anyhow::ensure!(
                    agent_doc_cycle_state_io::clear_pending_only_commit_intent(file, &target_hash,)?,
                    "pending-only commit continuation for {} lost its invalid target identity before rollback retirement",
                    file.display(),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_only_commit_invalid_atomic_completion_rolled_back file={} retired_target_hash={} restored_head_hash={} authority=editor_crdt reason=queue_not_completed_with_done_reap",
                        file.display(),
                        target_hash,
                        agent_doc_hash::content_hash(&head_content),
                    ),
                );
                return Ok(true);
            }
        };
        if self.retained_document_write_blocks(file) {
            let retained_target_matches =
                agent_doc_document_realtime_io::pending_document_write(file)
                    .is_some_and(|pending| pending.target_hash.eq_ignore_ascii_case(&target_hash));
            if !retained_target_matches
                || !agent_doc_document_realtime_io::settle_retained_non_capture_projection_through_authority(
                    file,
                    "pending_only_commit_resume_retained_projection",
                )?
                || self.retained_document_write_blocks(file)
            {
                return Ok(false);
            }
            authority_content =
                agent_doc_document_realtime_io::try_resolve_current_document_content(
                    file,
                    "pending_only_commit_resume_settled_authority",
                )?;
            disk_content = agent_doc_document_realtime_io::resolve_disk_current_document_content(
                file,
                "pending_only_commit_resume_settled_disk",
            )?;
            anyhow::ensure!(
                authority_content == disk_content
                    && agent_doc_hash::content_hash(&authority_content) == target_hash,
                "pending-only commit continuation for {} changed while its retained projection settled",
                file.display(),
            );
        }
        if migrated_legacy_intent {
            agent_doc_cycle_state_io::record_pending_only_commit_target(file, &target_hash)?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "pending_only_commit_continuation_migrated file={} target_hash={} proof=exact_recorded_done_row_deletions",
                    file.display(),
                    target_hash,
                ),
            );
        }

        let outcome = agent_doc_commit_io::commit_with_outcome(file)?;
        let head_content = agent_doc_git_io::revision::show_head(file)?.ok_or_else(|| {
            anyhow::anyhow!(
                "pending-only commit continuation for {} completed without readable HEAD content",
                file.display()
            )
        })?;
        anyhow::ensure!(
            agent_doc_hash::content_hash(&head_content) == target_hash,
            "pending-only commit continuation for {} did not land its exact target in HEAD",
            file.display(),
        );
        anyhow::ensure!(
            agent_doc_cycle_state_io::clear_pending_only_commit_intent(file, &target_hash)?,
            "pending-only commit continuation for {} lost its target identity before clear",
            file.display(),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "pending_only_commit_continuation_committed file={} target_hash={} did_commit={} migrated_legacy_intent={} authority=editor_crdt",
                file.display(),
                target_hash,
                outcome.did_commit,
                migrated_legacy_intent,
            ),
        );
        Ok(true)
    }

    fn resume_captured_finalize(
        &self,
        file: &Path,
    ) -> Result<agent_doc_session_check_io::CapturedFinalizeResumeOutcome> {
        use agent_doc_session_check_io::CapturedFinalizeResumeOutcome as Outcome;
        use agent_doc_turn::CyclePhase;

        let Some(mut state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
            return Ok(Outcome::NotApplicable);
        };
        let mut reactivated_false_stale_capture = false;
        let false_stale_reactivation_cycle = (state.phase == CyclePhase::Abandoned
            && cycle_event_is(
                &state.last_event,
                "repair_retire_superseded_captured_only_orphan",
            ))
            || (state.phase == CyclePhase::ResponseCaptured
                && cycle_event_is(
                    &state.last_event,
                    "session_check_reactivated_false_stale_capture_retirement",
                ));
        if false_stale_reactivation_cycle
            && let (Some(capture_id), Some(response_sha256)) =
                (state.capture_id.as_deref(), state.response_sha256.as_deref())
            && let Some(capture) = agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
            && capture.cycle_id == state.cycle_id
            && capture.response_sha256 == response_sha256
            && agent_doc_document_realtime_io::settle_retained_captured_projection_through_authority(
                        file,
                        &capture.response_body,
                        CapturedFinalizeSource::FalseStaleRetirementSettlement.as_str(),
                    )?
            {
                let cycle_reactivated =
                    agent_doc_cycle_state_io::reactivate_false_stale_capture_retirement(
                file,
                capture_id,
                response_sha256,
            )?;
                if !cycle_reactivated {
                return Ok(Outcome::Retained {
                    reason: "false-stale capture reactivation remains partially applied"
                        .to_string(),
                });
            }
            let Some(reactivated) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
            else {
                return Ok(Outcome::Retained {
                    reason: "false-stale capture reactivation lost its cycle projection"
                    .to_string(),
                });
            };
            if reactivated.phase != CyclePhase::ResponseCaptured
                || reactivated.capture_id.as_deref() != Some(capture_id)
                || reactivated.response_sha256.as_deref() != Some(response_sha256)
            {
                return Ok(Outcome::Retained {
                    reason: "false-stale capture reactivation did not converge on the same cycle"
                        .to_string(),
                });
            }
            state = reactivated;
            reactivated_false_stale_capture = true;
        }
        // A commit records the semantic closeout, but it does not imply that an
        // attached editor has already projected the acknowledged Lazily state
        // to disk. Recovery must keep owning that durable effect after the
        // phase transition. It may save only the exact converged authority that
        // already contains this cycle's captured response; it never replays a
        // committed response into an unrelated operator edit.
        if state.phase == CyclePhase::Committed {
            let (Some(capture_id), Some(response_sha256)) = (
                state.capture_id.as_deref(),
                state.response_sha256.as_deref(),
            ) else {
                return Ok(Outcome::NotApplicable);
            };
            let Some(capture) =
                agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
            else {
                return Ok(Outcome::NotApplicable);
            };
            if capture.cycle_id != state.cycle_id
                || capture.response_sha256 != response_sha256
                || capture.response_body.trim().is_empty()
                || agent_doc_document_realtime_io::pending_document_write(file).is_none()
            {
                return Ok(Outcome::NotApplicable);
            }
            let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
                file,
                CapturedFinalizeSource::CommittedEffectSinkCurrent.as_str(),
            )?;
            if !agent_doc_turn::response_replay::response_materialized_in_content(
                &capture.response_body,
                &current,
            ) {
                return Ok(Outcome::NotApplicable);
            }
            return match agent_doc_document_realtime_io::settle_retained_captured_projection_through_authority(
                file,
                &capture.response_body,
                CapturedFinalizeSource::CommittedEffectSinkSettlement.as_str(),
            ) {
                Ok(true) => Ok(Outcome::Committed),
                Ok(false) => Ok(Outcome::Retained {
                    reason: "committed capture is current in Lazily but its durable editor-save effect has not settled".to_string(),
                }),
                Err(err) => Ok(Outcome::Retained {
                    reason: format!(
                        "committed capture durable editor-save effect is not yet safe to settle: {err:#}"
                    ),
                }),
            };
        }
        if !matches!(
            state.phase,
            CyclePhase::ResponseCaptured | CyclePhase::WriteApplied
        ) {
            return Ok(Outcome::NotApplicable);
        }
        let _closeout_owner = match try_claim_recovery_closeout_owner(file, &state.cycle_id)? {
            Ok(owner) => owner,
            Err(reason) => return Ok(Outcome::Retained { reason }),
        };
        let (Some(capture_id), Some(response_sha256)) =
            (state.capture_id.clone(), state.response_sha256.clone())
        else {
            return Ok(Outcome::NotApplicable);
        };
        let Some(capture) =
            agent_doc_cycle_state_io::load_projected_captured_response(file, &capture_id)?
        else {
            return Ok(Outcome::NotApplicable);
        };
        if capture.cycle_id != state.cycle_id
            || capture.response_sha256 != response_sha256
            || capture.response_body.trim().is_empty()
        {
            return Ok(Outcome::NotApplicable);
        }

        let has_retained_delivery =
            agent_doc_document_realtime_io::pending_document_write(file).is_some();
        if has_retained_delivery {
            let settled = match agent_doc_document_realtime_io::settle_retained_captured_projection_through_authority(
                file,
                &capture.response_body,
                CapturedFinalizeSource::RetainedProjectionSettlement.as_str(),
            ) {
                Ok(settled) => settled,
                Err(err) => {
                    return Ok(Outcome::Retained {
                        reason: format!("retained projection is not yet safe to settle: {err:#}"),
                    });
                }
            };
            if !settled {
                return Ok(Outcome::Retained {
                    reason: "retained target has not reached exact canonical/disk convergence"
                        .to_string(),
                });
            }
        } else if !reactivated_false_stale_capture {
            let mut current = agent_doc_document_realtime_io::try_resolve_current_document_content(
                file,
                CapturedFinalizeSource::ReconciliationCurrent.as_str(),
            )?;
            let mut disk = agent_doc_document_realtime_io::resolve_disk_current_document_content(
                file,
                CapturedFinalizeSource::ReconciliationDisk.as_str(),
            )?;
            let response_materialized =
                agent_doc_turn::response_replay::response_materialized_in_content(
                    &capture.response_body,
                    &current,
                );
            if current != disk && response_materialized {
                match agent_doc_document_realtime_io::
                    settle_acknowledged_captured_projection_through_authority(
                        file,
                        &capture.response_body,
                        CapturedFinalizeSource::NativeSaveWithoutRetainedIntent.as_str(),
                    ) {
                    Ok(Some(projected)) => {
                        current = projected;
                        disk =
                            agent_doc_document_realtime_io::resolve_disk_current_document_content(
                                file,
                                CapturedFinalizeSource::NativeSaveWithoutRetainedIntentSettled
                                    .as_str(),
                            )?;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return Ok(Outcome::Retained {
                            reason: format!(
                                "response_captured: safe native editor-save recovery remains pending; no forced disk write was attempted: {error:#}"
                            ),
                        });
                    }
                }
            }
            let decision = agent_doc_turn::closeout_recovery::reconcile_write_applied_evidence(
                agent_doc_turn::closeout_recovery::WriteAppliedReconciliationEvidence {
                    cycle_phase: state.phase,
                    response_materialized_in_authority:
                        agent_doc_turn::response_replay::response_materialized_in_content(
                            &capture.response_body,
                            &current,
                        ),
                    authority_matches_disk: current == disk,
                },
            );
            match decision {
                agent_doc_turn::closeout_recovery::WriteAppliedReconciliationDecision::AlreadyProjected => {}
                agent_doc_turn::closeout_recovery::WriteAppliedReconciliationDecision::PromoteCycleProjection => {
                    agent_doc_cycle_state_io::mark_write_applied(
                        file,
                        "session_check_capture_write_applied_reconciled",
                        Some(&current),
                        Some(&disk),
                    )?;
                    let Some(reconciled) =
                        agent_doc_cycle_state_io::load_with_closeout_projection(file)?
                    else {
                        return Ok(Outcome::Retained {
                            reason: "write-applied capture reconciliation lost its cycle projection"
                                .to_string(),
                        });
                    };
                    state = reconciled;
                    }
                    agent_doc_turn::closeout_recovery::WriteAppliedReconciliationDecision::RetainUntilVisible => {
                        return Ok(Outcome::Retained {
                            reason: if response_materialized && current != disk {
                                "response_captured: the exact captured response is current in editor authority, but safe native-save/reload recovery has not yet produced the same disk projection. No forced disk write was attempted. If the editor listener cannot hot-reload, reload the IDE host, then rerun session-check only"
                                    .to_string()
                            } else {
                                "response_captured: captured response is waiting for exact authority/disk convergence before write-applied"
                                    .to_string()
                            },
                        });
                    }
                agent_doc_turn::closeout_recovery::WriteAppliedReconciliationDecision::NotApplicable => {
                    return Ok(Outcome::NotApplicable);
                }
            }
        }

        let Some(current_state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        else {
            return Ok(Outcome::Superseded);
        };
        if current_state.cycle_id != state.cycle_id
            || current_state.capture_id.as_deref() != Some(capture_id.as_str())
            || current_state.response_sha256.as_deref() != Some(response_sha256.as_str())
        {
            return Ok(Outcome::Superseded);
        }

        if let Err(err) = agent_doc_repair_io::pending::clear_pending(file) {
            return Ok(Outcome::Retained {
                reason: format!("failed to advance the durable capture to write-applied: {err:#}"),
            });
        }
        let current = match agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            CapturedFinalizeSource::RetainedWriteAppliedCurrent.as_str(),
        ) {
            Ok(current) => current,
            Err(err) => {
                return Ok(Outcome::Retained {
                    reason: format!("failed to re-read settled document authority: {err:#}"),
                });
            }
        };
        let disk = match agent_doc_document_realtime_io::resolve_disk_current_document_content(
            file,
            CapturedFinalizeSource::RetainedWriteAppliedDisk.as_str(),
        ) {
            Ok(disk) => disk,
            Err(err) => {
                return Ok(Outcome::Retained {
                    reason: format!("failed to re-read settled disk projection: {err:#}"),
                });
            }
        };
        if disk != current
            || !agent_doc_turn::response_replay::response_materialized_in_content(
                &capture.response_body,
                &current,
            )
        {
            return Ok(Outcome::Retained {
                reason: "captured response is not yet exact in both canonical authority and disk"
                    .to_string(),
            });
        }
        if let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &current,
            agent_doc_ops_log_io::log_op,
        ) {
            return Ok(Outcome::Retained {
                reason: format!("failed to refresh the settled response snapshot: {err:#}"),
            });
        }
        if state.phase == CyclePhase::ResponseCaptured
            && let Err(err) = agent_doc_cycle_state_io::mark_write_applied(
                file,
                "session_check_retained_capture_write_applied",
                Some(&current),
                Some(&current),
            )
        {
            return Ok(Outcome::Retained {
                reason: format!("failed to project write-applied state: {err:#}"),
            });
        }
        if let Err(err) = agent_doc_commit_io::commit(file) {
            return Ok(Outcome::Retained {
                reason: format!("same-capture commit remains retryable: {err:#}"),
            });
        }

        let committed =
            agent_doc_cycle_state_io::load_with_closeout_projection(file)?.is_some_and(|current| {
                current.cycle_id == state.cycle_id && current.phase == CyclePhase::Committed
            });
        if committed {
            Ok(Outcome::Committed)
        } else {
            Ok(Outcome::Retained {
                reason: "same-capture commit returned without a committed cycle projection"
                    .to_string(),
            })
        }
    }

    fn resume_retained_closeout_after_native_save(
        &self,
        file: &Path,
    ) -> Result<agent_doc_session_check_io::CapturedFinalizeResumeOutcome> {
        self.resume_captured_finalize(file)
    }
}

pub struct RuntimeCloseoutEffects;

pub fn closeout_effects() -> RuntimeCloseoutEffects {
    RuntimeCloseoutEffects
}

/// Run the closeout coordinator inside one revision-aware document projection
/// pass. Every nested session-check/commit reader shares the projection until
/// the CRDT state vector (or detached disk hash) changes.
pub fn complete_required_closeout(file: &Path, force_disk: bool) -> Result<bool> {
    agent_doc_document_realtime_io::with_current_document_projection_pass(|| {
        agent_doc_flow_io::closeout::complete_required_closeout_with_options(
            file,
            &closeout_effects(),
            agent_doc_flow_io::closeout::CompleteRequiredCloseoutOptions { force_disk },
        )
    })
}

impl agent_doc_flow_io::closeout::CloseoutEffects for RuntimeCloseoutEffects {
    fn commit(&self, file: &Path) -> Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn commit_for_authority(&self, file: &Path, force_disk: bool) -> Result<bool> {
        agent_doc_commit_io::commit_for_authority(file, force_disk)
    }

    fn run_pending_maintenance(
        &self,
        file: &Path,
        force_disk: bool,
    ) -> Result<agent_doc_preflight_io::PendingMaintenanceReport> {
        if force_disk {
            agent_doc_preflight_io::run_pending_maintenance_force_disk(
                file,
                &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        } else {
            agent_doc_preflight_io::run_pending_maintenance(
                file,
                &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        }
    }

    fn enforce_clean_closeout(&self, file: &Path) -> Result<()> {
        agent_doc_session_check_io::enforce_clean_closeout(file, &session_check_effects())
    }

    fn enforce_clean_closeout_for_authority(&self, file: &Path, force_disk: bool) -> Result<()> {
        agent_doc_session_check_io::enforce_clean_closeout_with_force_disk(
            file,
            force_disk,
            &session_check_effects(),
        )
    }

    fn cancel_preflight_cycle(&self, file: &Path) -> Result<()> {
        agent_doc_repair_io::cancel_preflight_cycle(&REPAIR_IO_EFFECTS, file).map(|_| ())
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool> {
        agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_session_check_io::detect_bypassed_response_write(file)
    }

    fn resolve_current_document(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
        agent_doc_document_realtime_io::try_resolve_current_document_with_source(file, source)
    }

    fn resolve_current_document_for_authority(
        &self,
        file: &Path,
        source: &str,
        force_disk: bool,
    ) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
        if force_disk {
            agent_doc_document_realtime_io::resolve_disk_current_document(file, source)
        } else {
            self.resolve_current_document(file, source)
        }
    }

    fn write_current_document(
        &self,
        doc: &agent_doc_document_realtime_io::CurrentDocument,
        content: &str,
        source: &str,
    ) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
            doc.key().as_path(),
            content,
            doc.content(),
            source,
        )
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_session_check_io::{CapturedFinalizeResumeOutcome, SessionCheckEffects};
    use agent_doc_turn::CyclePhase;

    #[test]
    fn legacy_pending_only_commit_requires_atomic_queue_strike_and_done_row_deletion() {
        let head = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n",
            "- do [#done]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep] keep this\n",
            "- [ ] [#done] completed work\n",
            "<!-- /agent:backlog -->\n",
        );
        let exact = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#done]~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(exact_legacy_pending_only_reap(head, exact, &["done".to_string()],).unwrap());

        let deletion_only = exact.replace("- ~~do [#done]~~", "- do [#done]");
        assert!(
            !exact_legacy_pending_only_reap(head, &deletion_only, &["done".to_string()]).unwrap(),
            "an unstruck queue entry must make a retained backlog deletion ineligible",
        );

        let with_operator_edit = exact.replace("keep this", "operator changed this");
        assert!(
            !exact_legacy_pending_only_reap(head, &with_operator_edit, &["done".to_string()])
                .unwrap(),
            "legacy migration must not authorize a commit containing any untracked edit",
        );
        assert!(
            !exact_legacy_pending_only_reap(head, exact, &["keep".to_string()]).unwrap(),
            "the removed row must match the cycle's explicit done intent",
        );
    }

    #[test]
    fn retained_exact_pending_only_projection_recovers_missing_target_identity() {
        let head = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n",
            "- do [#done]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done] completed work\n",
            "<!-- /agent:backlog -->\n",
        );
        let target = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#done]~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let target_hash = agent_doc_hash::content_hash(target);
        let done_ids = vec!["done".to_string()];

        assert_eq!(
            infer_pending_only_commit_target_from_retained_projection(
                head,
                CyclePhase::Committed,
                None,
                &done_ids,
                Some((&target_hash, target)),
            )
            .unwrap(),
            Some(target_hash),
        );

        let deletion_only = target.replace("- ~~do [#done]~~", "- do [#done]");
        let deletion_only_hash = agent_doc_hash::content_hash(&deletion_only);
        assert_eq!(
            infer_pending_only_commit_target_from_retained_projection(
                head,
                CyclePhase::Committed,
                None,
                &done_ids,
                Some((&deletion_only_hash, &deletion_only)),
            )
            .unwrap(),
            None,
            "a deletion-only retained projection must not gain commit authority",
        );
    }

    /// `#deadlockhint` — when the recovery classifier finds nothing to recover
    /// there is no response body pending, so prescribing `write --commit` is a
    /// dead end: it rejects with `empty response — nothing to write` while the
    /// drift guard keeps firing. The hint must name `agent-doc commit`, which is
    /// the command that actually closes document-only drift.
    #[test]
    fn clean_classified_drift_hint_prescribes_commit_not_an_impossible_write() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nagent_doc_format: template\n---\n").unwrap();

        let hint = closeout_recovery_hint(&file);
        assert!(
            hint.contains("agent-doc commit"),
            "a clean-classified drift hint must name the command that can run: {hint}"
        );
        assert!(
            hint.contains("No response body is pending"),
            "the hint must say why `write --commit` is not the path: {hint}"
        );
    }

    #[test]
    fn captured_response_without_retained_intent_uses_safe_native_save_recovery() {
        let source = include_str!("lib.rs");
        let start = source
            .find("fn resume_captured_finalize(")
            .expect("captured finalize recovery must exist");
        let body = &source[start..];
        let end = body
            .find("\n}\n\npub struct RuntimeCloseoutEffects")
            .expect("resume handler must have a bounded source span");
        let body = &body[..end];

        assert_eq!(
            CapturedFinalizeSource::NativeSaveWithoutRetainedIntent.as_str(),
            "session_check_capture_without_retained_intent_native_save"
        );
        assert!(
            body.contains("CapturedFinalizeSource::NativeSaveWithoutRetainedIntent")
                && body.contains("settle_acknowledged_captured_projection_through_authority",),
            "authority/disk divergence after intent retirement must reach the native-save effect"
        );
        assert!(
            !body.contains("atomic_write_force_disk_through_authority"),
            "captured closeout recovery must never replace the editor-owned buffer via disk"
        );
    }

    #[test]
    fn exact_retained_target_reactivates_false_stale_capture_before_commit_retry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("session.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please investigate.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: investigate — test\n\n",
            "Fixed the retained closeout.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- no-pending-capture -->\n",
        );
        let target = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: investigate — test\n\n",
            "Fixed the retained closeout.\n",
            "<!-- agent:boundary:response -->\n",
            "<!-- /agent:exchange -->\n",
        );

        std::fs::write(&file, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = agent_doc_capture_io::capture_response(&file, response).unwrap();
        agent_doc_capture_io::mark_discarded(&file).unwrap();
        agent_doc_cycle_state_io::mark_abandoned(
            &file,
            "repair_retire_superseded_captured_only_orphan",
            Some(base),
            Some(base),
        )
        .unwrap();

        agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
            &file,
            target,
            base,
            "false_stale_exact_target_fixture_projection",
        )
        .unwrap();
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &file,
            base,
            target,
            "false_stale_exact_target_test",
            agent_doc_document_realtime_io::DocumentWriteDeferredReason::CrdtDeliveryAckPending,
        )
        .unwrap();
        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(response, target),
            "fixture target must materialize the captured response"
        );
        assert!(
            agent_doc_document_realtime_io::pending_document_write(&file).is_some(),
            "fixture must retain a document-write intent"
        );
        assert_eq!(
            agent_doc_document_realtime_io::try_resolve_current_document_content(
                &file,
                "false_stale_exact_target_fixture_current",
            )
            .unwrap(),
            target,
            "fixture canonical authority must equal the retained target"
        );
        let abandoned = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .unwrap();
        assert_eq!(abandoned.phase, CyclePhase::Abandoned);
        assert!(
            cycle_event_is(
                &abandoned.last_event,
                "repair_retire_superseded_captured_only_orphan"
            ),
            "projected state must preserve the typed abandonment reason: {}",
            abandoned.last_event,
        );

        let outcome = RuntimeSessionCheckEffects
            .resume_captured_finalize(&file)
            .unwrap();
        assert!(
            matches!(
                outcome,
                CapturedFinalizeResumeOutcome::Committed
                    | CapturedFinalizeResumeOutcome::Retained { .. }
            ),
            "the exact retained target must enter closeout recovery instead of returning NotApplicable: {outcome:?}"
        );
        assert!(
            agent_doc_document_realtime_io::pending_document_write(&file).is_none(),
            "exact canonical/disk convergence must retire the orphaned delivery intent"
        );
        let state = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .unwrap();
        assert_eq!(state.cycle_id, capture.cycle_id);
        assert!(
            matches!(
                state.phase,
                CyclePhase::WriteApplied | CyclePhase::Committed
            ),
            "the false-stale cycle must be reactivated and advanced: {state:?}"
        );
        let capture =
            agent_doc_cycle_state_io::load_projected_captured_response(&file, &capture.capture_id)
                .unwrap()
                .expect("the exact same capture remains in the durable ledger");
        assert_eq!(capture.cycle_id, state.cycle_id);
    }

    #[test]
    fn committed_capture_settles_its_retained_effect_sink() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("session.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please investigate.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: investigate — test\n\n",
            "Fixed the retained closeout.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- no-pending-capture -->\n",
        );
        let target = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: investigate — test\n\n",
            "Fixed the retained closeout.\n",
            "<!-- agent:boundary:response -->\n",
            "<!-- /agent:exchange -->\n",
        );

        std::fs::write(&file, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = agent_doc_capture_io::capture_response(&file, response).unwrap();
        std::fs::write(&file, target).unwrap();
        agent_doc_cycle_state_io::mark_committed(&file, "commit_success", Some(base), Some(target))
            .unwrap();
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &file,
            base,
            target,
            "committed_capture_effect_sink_test",
            agent_doc_document_realtime_io::DocumentWriteDeferredReason::CrdtDeliveryAckPending,
        )
        .unwrap();

        let before = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .unwrap();
        assert_eq!(before.phase, CyclePhase::Committed);
        assert_eq!(
            before.capture_id.as_deref(),
            Some(capture.capture_id.as_str())
        );
        assert!(agent_doc_document_realtime_io::pending_document_write(&file).is_some());

        let outcome = RuntimeSessionCheckEffects
            .resume_captured_finalize(&file)
            .unwrap();
        assert_eq!(outcome, CapturedFinalizeResumeOutcome::Committed);
        assert!(
            agent_doc_document_realtime_io::pending_document_write(&file).is_none(),
            "exact Lazily/disk proof must retire the committed cycle's retained effect"
        );
    }

    #[test]
    fn write_applied_capture_promotes_lagging_backbone_before_commit_retry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("session.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please halt the queue.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: queue halted — test\n\n",
            "The queue is halted.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- no-pending-capture -->\n",
        );
        let target = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please halt the queue.\n",
            "### Re: queue halted — test\n\n",
            "The queue is halted.\n",
            "<!-- agent:boundary:response -->\n",
            "<!-- /agent:exchange -->\n",
        );

        std::fs::write(&file, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = agent_doc_capture_io::capture_response(&file, response).unwrap();
        std::fs::write(&file, target).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            target,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_capture_io::mark_write_applied(&file).unwrap();
        assert!(!agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(&file));

        let before = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .unwrap();
        assert_eq!(before.phase, CyclePhase::WriteApplied);
        assert_eq!(
            before.capture_id.as_deref(),
            Some(capture.capture_id.as_str())
        );

        let outcome = RuntimeSessionCheckEffects
            .resume_captured_finalize(&file)
            .unwrap();
        assert!(
            matches!(
                outcome,
                CapturedFinalizeResumeOutcome::Committed
                    | CapturedFinalizeResumeOutcome::Retained { .. }
            ),
            "write-applied capture proof must enter commit recovery: {outcome:?}"
        );
        let after = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .unwrap();
        assert!(matches!(
            after.phase,
            CyclePhase::WriteApplied | CyclePhase::Committed
        ));
    }
}

pub fn closeout_recovery_hint(file: &Path) -> String {
    let state = agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
        file,
        &closeout_effects(),
    );
    match agent_doc_flow_io::closeout::closeout_recovery_command_for_file(file, state) {
        Some(command) => format!("Recovery [{}]: {}.", state.as_str(), command),
        // `#deadlockhint`: the caller saw drift but the recovery classifier found
        // nothing to recover, so there is no captured or visible response body in
        // play. Prescribing `write --commit` here is a DEADLOCK — it rejects with
        // `empty response — nothing to write`, `repair --apply-recovery` reports
        // `clean — no recovery needed`, and the guard keeps firing, so every
        // documented path declines and the session cannot advance. Name the
        // command that actually closes document-only drift, and mention
        // `write --commit` only for the case that genuinely needs it.
        None => format!(
            "No response body is pending, so this is document-only drift: run `agent-doc commit {}`, then re-run `agent-doc session-check {}`. \
             Use `agent-doc write --commit {}` instead only if you still have an unwritten response body to persist.",
            file.display(),
            file.display(),
            file.display()
        ),
    }
}
