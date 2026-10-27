//! Binary-owned completion feedback for background-subagent queue claims.
//!
//! Workers never write the shared session document. The coordinator reports a
//! worker result through `queue report-complete`. The binary appends the actual
//! worker response as one inert, attested artifact in `agent:exchange` through
//! the editor/CRDT authority.
//! Reporting keeps the claim active while integration and verification run;
//! ordinary claim release never implies that a worker completed successfully.

use std::path::Path;
use std::time::Duration;

use agent_doc_element::component_scope::{changed_component_scope, with_component_write_scope};
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result, bail};

const SOURCE: &str = "queue_subagent_completion_feedback";
const COMPLETION_LOCK_WAIT: Duration = Duration::from_secs(5);
// The cell grammar and its structural recognition are shared with every
// recovery/closeout classifier (`agent_doc_document::subagent_completion`).
use agent_doc_document::subagent_completion::{
    BODY as COMPLETION_BODY, LEGACY_HEADING_PREFIX as LEGACY_COMPLETION_HEADING_PREFIX,
};

#[derive(Debug)]
pub(crate) struct CompletionFeedbackOutcome {
    pub claim: QueueClaim,
    pub appended: bool,
    /// Durable editor-authority target still awaiting visible projection.
    pub projection_pending: bool,
    pub intent_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CompletionFeedbackRepairOutcome {
    pub repaired: usize,
    pub accepted: bool,
}

pub(crate) fn report_complete(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    response: &str,
) -> Result<Option<CompletionFeedbackOutcome>> {
    anyhow::ensure!(
        !response.trim().is_empty(),
        "subagent completion response must not be empty"
    );
    report_complete_with_lock_wait(file, item, generation, response, COMPLETION_LOCK_WAIT)
}

fn report_complete_with_lock_wait(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    response: &str,
    lock_wait: Duration,
) -> Result<Option<CompletionFeedbackOutcome>> {
    // `#subagentcompletionfeedback`: unlike SESSION_ACTOR_WRITE_QUEUE, this
    // lock serializes independent CLI processes. Re-resolve and rebuild inside
    // the owner so stale full-document successors cannot lose sibling notices.
    let lock_path = agent_doc_queue_io::queue_claim::completion_feedback_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, lock_wait, |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        bail!(
            "timed out after {}ms waiting to publish subagent completion feedback for {}; no document mutation was attempted",
            lock_wait.as_millis(),
            file.display(),
        );
    };
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
    let Some(claim) = observed_claim(file, item, &current)? else {
        return Ok(None);
    };
    if !claim.owner.starts_with("subagent:") {
        return Ok(None);
    }
    let claim =
        agent_doc_queue_io::queue_claim::report_attempt(file, &claim.item_text, generation)?;
    // Derive the reported marker and completion cell from one cut and publish
    // one successor. Two separate retained editor writes can conflict while
    // the first is still awaiting projection.
    let projection =
        agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection(file, &current, None)?;
    let (appended, intent_id) =
        append_completion_feedback(file, &current, &projection.target_content, &claim, response)?;
    agent_doc_queue_io::queue_cmd::record_claim_runtime_projection(file, &projection, None)?;
    Ok(Some(CompletionFeedbackOutcome {
        claim,
        appended,
        projection_pending: intent_id.is_some(),
        intent_id,
    }))
}

fn observed_claim(file: &Path, item: &str, current: &str) -> Result<Option<QueueClaim>> {
    let mut ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, current)?;
    // Use the same pure identity/continuation matching as release without
    // mutating the durable ledger.
    Ok(ledger.release(item))
}

fn append_completion_feedback(
    file: &Path,
    current: &str,
    projected: &str,
    claim: &QueueClaim,
    response: &str,
) -> Result<(bool, Option<String>)> {
    let key = agent_doc_queue_io::queue_claim::completion_feedback_key(claim)?;
    let marker = agent_doc_document::subagent_completion::marker(&key);
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
    let appended = !projected.contains(&marker);
    let updated = if appended {
        agent_doc_element_exchange::insert_prompt_line_before_boundary(projected, &note)?
    } else {
        projected.to_string()
    };
    if updated == current {
        let intent_id = agent_doc_document_realtime_io::pending_document_write(file)
            .filter(|pending| pending.target_content == current)
            .map(|pending| pending.intent_id);
        return Ok((false, intent_id));
    }
    let scope = changed_component_scope(current, &updated)
        .context("subagent completion feedback must change only agent:exchange/queue")?;
    let scope_pairs = scope.to_pairs();
    if (appended && !scope.contains("exchange", 0))
        || scope_pairs
            .iter()
            .any(|(name, _)| name != "exchange" && name != "queue")
    {
        bail!("subagent completion feedback escaped agent:exchange/queue components");
    }
    // An attached editor owns disk projection. Hand the target to the durable
    // retained-intent graph and return as soon as the controller accepts it;
    // its reactive projector applies the target without keeping this one-shot
    // CLI process alive through typing/native-save backpressure. Detached files
    // still use the ordinary synchronous CAS write.
    let retained_intent = publish_exchange_update(file, current, &updated, scope)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "subagent_completion_feedback_accepted owner={} claimed_at={} key={} item_bytes={} delivery={}{}",
            claim.owner,
            claim.claimed_at_secs,
            key,
            claim.item_text.len(),
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
    Ok((appended, retained_intent))
}

/// Settle any retained completion successor, then rederive the queue marker
/// from the durable claim ledger. This never releases a claim or writes behind
/// an editor-owned buffer.
pub(crate) fn repair_claim_projection(
    file: &Path,
    effects: &impl agent_doc_queue_io::queue_cmd::QueueCommandEffects,
) -> Result<bool> {
    let lock_path = agent_doc_queue_io::queue_claim::completion_feedback_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, COMPLETION_LOCK_WAIT, |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        bail!(
            "timed out after {}ms waiting to repair queue projection for {}; retry `agent-doc queue repair-projection {:?}`",
            COMPLETION_LOCK_WAIT.as_millis(),
            file.display(),
            file,
        );
    };

    if agent_doc_document_realtime_io::pending_document_write(file).is_some() {
        agent_doc_document_realtime_io::recover_retained_document_write_before_new_cycle(
            file,
            agent_doc_document_realtime_io::RetainedWriteCycleBoundary::QueueProjectionRepair,
        )?;
    }
    if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file) {
        bail!(
            "queue projection is durably retained as intent {} but no editor delivery receipt is available; open {} in the active editor/project, then rerun `agent-doc queue repair-projection {:?}`",
            pending.intent_id,
            file.display(),
            file,
        );
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
    let lock_path = agent_doc_queue_io::queue_claim::completion_feedback_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, COMPLETION_LOCK_WAIT, |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        bail!(
            "timed out after {}ms waiting to repair subagent completion feedback for {}; no document mutation was attempted",
            COMPLETION_LOCK_WAIT.as_millis(),
            file.display(),
        );
    };
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
    let lock_path = agent_doc_queue_io::queue_claim::completion_feedback_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, COMPLETION_LOCK_WAIT, |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        bail!(
            "timed out after {}ms waiting to repair stale completion evidence for {}; no document mutation was attempted",
            COMPLETION_LOCK_WAIT.as_millis(),
            file.display(),
        );
    };
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

    fn test_claim() -> QueueClaim {
        QueueClaim {
            identity: QueueItemIdentity::FreeText("finish-sdk".into()),
            item_text: "finish sdk.md\nwith deterministic tests".into(),
            owner: "subagent:sdk-feedback".into(),
            generation: 1,
            state: agent_doc_queue::queue_claim::QueueClaimState::Running,
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

    #[test]
    fn report_is_immediate_but_keeps_claim_until_release() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sdk-feedback\nagent_doc_format: template\n---\n\n",
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
            "Implemented SDK support.\n\nTests: 12 passed.",
        )
        .unwrap()
        .unwrap();
        assert!(first.appended);
        assert!(!first.projection_pending);
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
            "Implemented SDK support.\n\nTests: 12 passed.",
        )
        .unwrap()
        .unwrap();
        assert!(
            !second.appended,
            "the same completion must dedupe across TTL refresh"
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
        assert!(
            report_complete(&doc, item, Some(first.generation), "worker one completed")
                .unwrap()
                .is_none(),
            "a tombstoned attempt cannot manufacture completion feedback"
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
        let stale =
            report_complete(&doc, item, Some(first.generation), "late stale result").unwrap_err();
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

        let started = std::time::Instant::now();
        let threads = items
            .into_iter()
            .map(|item| {
                let doc = doc.clone();
                std::thread::spawn(move || {
                    report_complete(&doc, item, Some(1), "concurrent worker completed")
                        .unwrap()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.iter().all(|outcome| outcome.appended));
        assert!(outcomes.iter().all(|outcome| outcome.projection_pending));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "four reports must return on durable acceptance, not wait for editor projection"
        );

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

        let first = report_complete(&doc, "first worker", Some(1), "first result")
            .unwrap()
            .unwrap();
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

        let second = report_complete(&doc, "second worker", Some(1), "second result")
            .unwrap()
            .unwrap();
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
        let lock_path =
            agent_doc_queue_io::queue_claim::completion_feedback_lock_path(&doc).unwrap();
        let agent_doc_sync_io::SyncLockAcquire::Acquired(_held) =
            agent_doc_sync_io::acquire_sync_lock(&lock_path, Duration::ZERO, |_| {})
        else {
            panic!("fixture must acquire completion lock");
        };
        let started = std::time::Instant::now();
        let error = report_complete_with_lock_wait(
            &doc,
            "finish sdk",
            Some(1),
            "worker completed",
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timed out after 50ms"));
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("Subagent completed")
        );
    }
}
