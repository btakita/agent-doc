use std::path::Path;

use anyhow::Result;

pub use agent_doc_document_realtime_io::{
    RUNTIME_PIPELINE_FRONTMATTER_EFFECTS as PIPELINE_FRONTMATTER_EFFECTS,
    RuntimePipelineFrontmatterEffects as PipelineFrontmatterEffects,
};

pub struct RuntimeRepairIoEffects;

pub static REPAIR_IO_EFFECTS: RuntimeRepairIoEffects = RuntimeRepairIoEffects;

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

fn cycle_event_is(state_event: &str, expected: &str) -> bool {
    state_event == expected
        || state_event
            .split_ascii_whitespace()
            .any(|field| field.strip_prefix("reason=") == Some(expected))
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
                        "session_check_false_stale_capture_retirement_settlement",
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
        if !matches!(
            state.phase,
            CyclePhase::ResponseCaptured | CyclePhase::WriteApplied
        ) {
            return Ok(Outcome::NotApplicable);
        }
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
                "session_check_retained_captured_projection_settlement",
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
            let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
                file,
                "session_check_write_applied_capture_reconciliation_current",
            )?;
            let disk = agent_doc_document_realtime_io::resolve_disk_current_document_content(
                file,
                "session_check_write_applied_capture_reconciliation_disk",
            )?;
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
                            reason: "response_captured: captured response is waiting for exact authority/disk convergence before write-applied"
                                .to_string(),
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
            "session_check_retained_capture_write_applied",
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
            "session_check_retained_capture_disk",
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
}

pub struct RuntimeCloseoutEffects;

pub fn closeout_effects() -> RuntimeCloseoutEffects {
    RuntimeCloseoutEffects
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
        None => format!(
            "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
            file.display(),
            file.display()
        ),
    }
}
