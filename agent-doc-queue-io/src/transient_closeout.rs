//! Durable-state evidence for the shared closeout gate (`#transientcloseout`,
//! `#stopquiescentrows`).
//!
//! [`agent_doc_queue::transient_closeout`] classifies live queue rows from
//! the document text. Whether a `🟡`/`🚧` row's claim is heartbeat-fresh and
//! whether a closed `agent:loop` window holds a `📥` row back live in
//! `state.db`, so every turn-end gate (the Claude/Grok Stop hook and
//! session-check) reads them here, the same way, before it classifies.
//! `#integrationinflight`: a `☑️` row's recorded integration in flight lives
//! in the queue lineage ledger, read here the same way.
//! `#finishedheadrefill`: a `🚧` row whose current-generation worker ended with a
//! recorded, unreported hand-back is report-owed, read from the claim ledger
//! here the same way.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_queue::transient_closeout::{
    CloseoutEvidence, IntegrationInFlight, ReportOwedAttempt, TransientCloseoutAssessment,
    assess_transient_closeout, fresh_claim_phase,
};

/// The evidence for `content` at `now_secs`. Reads the claim ledger and the
/// schedule state without writing either. An unreadable ledger proves no
/// claim fresh, so its rows keep refusing.
pub fn closeout_evidence_at(file: &Path, content: &str, now_secs: u64) -> CloseoutEvidence {
    // `#integrationinflight`: read from the same claim ledger, below.
    let mut integrations_in_flight = Default::default();
    // `#finishedheadrefill`: read from the same claim ledger, below.
    let mut report_owed = HashMap::new();
    let mut codex_coordinator_claims = Default::default();
    let (fresh_claims, reportable_lost) = match crate::queue_claim::load_ledger_following_read_only(
        file, content,
    ) {
        Ok(ledger) => {
            let frozen_secs = crate::subagent_pool::frozen_secs();
            let fresh = ledger
                .claims
                .iter()
                .filter_map(|claim| {
                    fresh_claim_phase(
                        claim,
                        now_secs,
                        frozen_secs,
                        agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
                    )
                    .map(|phase| (claim.identity.clone(), phase))
                })
                .collect();
            // `#lostrowreportreopen`: an ended worker's recorded,
            // unreported hand-back owes a report, not continue or retry.
            let reportable = ledger
                .claims
                .iter()
                .filter(|claim| claim.lost_end_hand_back().is_some())
                .map(|claim| claim.identity.clone())
                .collect();
            // `#finishedheadrefill`: a finished worker's unreported hand-back
            // owes `queue report-complete` for its generation, not a refresh.
            report_owed = ledger
                .claims
                .iter()
                .filter_map(|claim| {
                    ReportOwedAttempt::of(claim).map(|owed| (claim.identity.clone(), owed))
                })
                .collect();
            codex_coordinator_claims = ledger
                .claims
                .iter()
                .filter(|claim| {
                    claim.owner_session.as_ref().is_some_and(|owner| {
                        owner.harness.as_deref() == Some("codex") && owner.session_id.is_some()
                    })
                })
                .map(|claim| claim.identity.clone())
                .collect();
            integrations_in_flight =
                reported_integrations_in_flight(file, &ledger.claims, now_secs);
            (fresh, reportable)
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] could not read queue claims for {} to prove worker freshness; claimed rows keep refusing closeout: {err:#}",
                file.display()
            );
            Default::default()
        }
    };
    let schedule_held_heads = crate::queue_schedule::schedule_held_heads(file, content)
        .into_iter()
        .collect();
    CloseoutEvidence {
        fresh_claims,
        codex_coordinator_claims,
        schedule_held_heads,
        reportable_lost,
        report_owed,
        document: Some(file.display().to_string()),
        // `#gatedheadpending`: a blocker that closed into the external done
        // archive is resolved, not external.
        archived_done_ids: agent_doc_element_backlog_io::done_archive::blocker_archive_ids(
            file, content,
        ),
        integrations_in_flight,
    }
}

/// `#integrationinflight`: the integration in flight of each `Reported`
/// claim, from the lineage attempt of its own owner and generation. An
/// unreadable lineage ledger proves nothing in flight, so reported rows keep
/// refusing.
fn reported_integrations_in_flight(
    file: &Path,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
    now_secs: u64,
) -> HashMap<QueueItemIdentity, IntegrationInFlight> {
    if !claims
        .iter()
        .any(|claim| claim.state == agent_doc_queue::queue_claim::QueueClaimState::Reported)
    {
        return HashMap::new();
    }
    match crate::queue_lineage::load_read_only(file) {
        Ok(lineage) => {
            crate::queue_lineage::integrations_in_flight_for_claims(&lineage, claims, now_secs)
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] could not read queue lineage for {} to prove an integration in flight; reported rows keep refusing closeout: {err:#}",
                file.display()
            );
            HashMap::new()
        }
    }
}

/// [`closeout_evidence_at`] on the claim clock.
pub fn closeout_evidence(file: &Path, content: &str) -> CloseoutEvidence {
    closeout_evidence_at(file, content, crate::queue_claim::claim_clock_now_secs())
}

/// Classify `content`'s live transient rows with the durable evidence.
pub fn assess(file: &Path, content: &str) -> Result<TransientCloseoutAssessment> {
    assess_transient_closeout(content, &closeout_evidence(file, content))
}

/// [`agent_doc_queue::transient_closeout::every_debt_gated_by_recorded_blocker`]
/// with the external done archive resolved, so an archived (closed) blocker
/// never counts as one that still holds a row (`#blockerreportterminal`).
pub fn every_debt_gated_by_recorded_blocker(
    file: &Path,
    content: &str,
    debts: &[agent_doc_queue::transient_closeout::TransientCloseoutDebt],
) -> bool {
    agent_doc_queue::transient_closeout::every_debt_gated_by_recorded_blocker(
        content,
        debts,
        agent_doc_element_backlog_io::done_archive::blocker_archive_ids(file, content).as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use agent_doc_queue::transient_closeout::QuiescentReason;

    use super::*;
    use crate::queue_lineage::{
        DEFAULT_INTEGRATING_DEADLINE_SECS, IntegrationMethod, QueueLineageEventKind, RecordRequest,
        record_following_live_claim,
    };

    const ITEM: &str = "do [#integrationinflight]";
    const OWNER: &str = "subagent:inflight";

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit(repo: &Path, name: &str, message: &str) -> String {
        std::fs::write(repo.join(name), message).unwrap();
        git(repo, &["add", name]);
        git(repo, &["commit", "-qm", message]);
        git(repo, &["rev-parse", "HEAD"])
    }

    fn write_doc(doc: &Path, row: &str) -> String {
        let content = format!("<!-- agent:queue -->\n- {row}\n<!-- /agent:queue -->\n");
        std::fs::write(doc, &content).unwrap();
        content
    }

    fn integrating(source: &str, candidate: &str, repo: &Path, at_secs: u64) -> RecordRequest {
        RecordRequest {
            kind: QueueLineageEventKind::Integrating,
            at_secs: Some(at_secs),
            repo_path: Some(repo.to_path_buf()),
            source_sha: Some(source.to_string()),
            integration_sha: Some(candidate.to_string()),
            verification: Some("make check".to_string()),
            ..RecordRequest::default()
        }
    }

    /// 2026-11-15 `tasks/agent-doc/agent-doc-bugs.md` (`#resumemaintrace`,
    /// `#gatedheadpending`, `#installreattachambiguous`): the coordinator had
    /// merged each worker branch in its integration worktree and `make check`
    /// was still running, yet the Stop hook's gate refused every `☑️` row.
    /// With the durable `integrating` receipt the gate waits on it, until
    /// its deadline, and `integrated` supersedes it.
    #[test]
    fn integrationinflight_the_stop_gate_waits_on_a_recorded_integration_until_its_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "In Flight"]);
        std::fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        let doc = root.join("session.md");
        write_doc(&doc, ITEM);
        git(root, &["add", ".gitignore", "session.md"]);
        git(root, &["commit", "-qm", "base"]);

        crate::queue_claim::claim(&doc, ITEM, OWNER, 600).unwrap();
        crate::queue_claim::start_attempt(&doc, ITEM, OWNER, "worker:inflight", 600, 1).unwrap();
        let source = commit(root, "work.txt", "worker work");
        record_following_live_claim(
            &doc,
            ITEM,
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_repo_path: Some(root.to_path_buf()),
                sink_repo_path: Some(root.to_path_buf()),
                source_sha: Some(source.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        crate::queue_claim::report_attempt(&doc, ITEM, Some(1)).unwrap();
        let candidate = commit(root, "merge.txt", "integration candidate");
        let content = write_doc(&doc, &format!("☑️ [#{OWNER}#1] {ITEM}"));

        // No record: the reported row refuses and names the receipt.
        let now = 1_000_000;
        let assessment =
            assess_transient_closeout(&content, &closeout_evidence_at(&doc, &content, now))
                .unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert!(
            assessment.debts[0].owed.contains("--event integrating"),
            "{assessment:?}"
        );

        record_following_live_claim(&doc, ITEM, 1, integrating(&source, &candidate, root, now))
            .unwrap();
        let evidence = closeout_evidence_at(&doc, &content, now + 60);
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(assessment.quiescent.len(), 1, "{assessment:?}");
        assert_eq!(
            assessment.quiescent[0].reason,
            QuiescentReason::WaitingOnIntegration
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_integration"));

        // Past the deadline the gate refuses again.
        let late = now + DEFAULT_INTEGRATING_DEADLINE_SECS;
        let assessment =
            assess_transient_closeout(&content, &closeout_evidence_at(&doc, &content, late))
                .unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert!(
            assessment.debts[0].owed.contains("passed its deadline"),
            "{assessment:?}"
        );

        // `integrated` supersedes the record: the row owes its strike, not
        // a wait.
        let branch = git(root, &["branch", "--show-current"]);
        record_following_live_claim(
            &doc,
            ITEM,
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source),
                integration_sha: Some(candidate),
                integration_method: Some(IntegrationMethod::Merge),
                sink_ref: Some(branch),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let evidence = closeout_evidence_at(&doc, &content, now + 60);
        assert!(evidence.integrations_in_flight.is_empty(), "{evidence:?}");
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
    }

    /// `#finishedheadrefill`, at the Stop gate: the
    /// subagent-end hook froze a FINISHED worker's `Running` attempt with its
    /// hand-back before the coordinator reported it. The Stop hook's evidence
    /// reads that attempt from the claim ledger as report-owed, so the `🚧`
    /// row refuses with the exact `queue report-complete` for generation 1
    /// instead of the generic stale-worker refresh, and the report clears it.
    #[test]
    fn finishedheadrefill_the_stop_gate_names_the_report_for_a_finished_running_worker() {
        const FINISHED: &str = "do [#finishedworker]";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        write_doc(&doc, FINISHED);
        crate::queue_claim::claim(&doc, FINISHED, "subagent:fin", 600).unwrap();
        crate::queue_claim::start_attempt(&doc, FINISHED, "subagent:fin", "worker:fin", 600, 1)
            .unwrap();
        let content = write_doc(&doc, &format!("🚧 [#subagent:fin#1] {FINISHED}"));

        // A live worker stays quiescent regardless of heartbeat age.
        let late = crate::queue_claim::claim_clock_now_secs() + 3_600;
        let assessment = agent_doc_queue::transient_closeout::assess_transient_closeout(
            &content,
            &closeout_evidence_at(&doc, &content, late),
        )
        .unwrap();
        assert_eq!(assessment.quiescent.len(), 1, "{assessment:#?}");
        assert!(assessment.debts.is_empty(), "{assessment:#?}");

        let reason = agent_doc_queue::queue_claim::subagent_end_freeze_reason(Some(
            "**Summary** fixed it. **What changed** branch fix/finished, commit 1234abc.",
        ));
        let frozen = crate::queue_claim::freeze_attempt(&doc, FINISHED, Some(1), &reason).unwrap();
        assert_eq!(
            frozen.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        let evidence = closeout_evidence(&doc, &content);
        assert_eq!(evidence.report_owed.len(), 1, "{evidence:#?}");
        assert!(evidence.reportable_lost.is_empty(), "{evidence:#?}");
        let failure =
            agent_doc_queue::transient_closeout::normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .expect("the finished worker's row still owes its report");
        assert!(
            failure.contains("report the hand-back its ended worker left"),
            "{failure}"
        );
        let document = doc.display().to_string();
        assert!(
            failure.contains(&format!(
                "agent-doc queue report-complete {document} --item '#finishedworker' --generation 1 --response '<complete worker hand-back>'"
            )),
            "{failure}"
        );
        assert!(
            !failure.contains("no bound running worker claim backs this row"),
            "{failure}"
        );
    }
}
