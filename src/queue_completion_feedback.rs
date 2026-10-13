//! Binary-owned completion feedback for background-subagent queue claims.
//!
//! Workers never write the shared session document. The coordinator reports a
//! worker result through `queue report-complete` (and `queue release` provides
//! an idempotent fallback). The binary appends one inert status artifact to
//! `agent:exchange` through the editor/CRDT authority. Reporting keeps the
//! claim active while integration and verification run.

use std::path::Path;

use agent_doc_element::component_scope::{changed_component_scope, with_component_write_scope};
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result, bail};

const SOURCE: &str = "queue_subagent_completion_feedback";

#[derive(Debug)]
pub(crate) struct CompletionFeedbackOutcome {
    pub claim: QueueClaim,
    pub appended: bool,
}

pub(crate) fn report_complete(
    file: &Path,
    item: &str,
) -> Result<Option<CompletionFeedbackOutcome>> {
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let Some(claim) = observed_claim(file, item, &current)? else {
        return Ok(None);
    };
    if !claim.owner.starts_with("subagent:") {
        return Ok(None);
    }
    let appended = append_completion_feedback(file, &current, &claim)?;
    Ok(Some(CompletionFeedbackOutcome { claim, appended }))
}

/// Backward-compatible fallback: old coordinators that only call `release`
/// still publish the notice. The exact-generation release prevents the write's
/// latency from clearing a refreshed or reassigned claim.
pub(crate) fn release_with_feedback(file: &Path, item: &str) -> Result<Option<QueueClaim>> {
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let Some(expected) = observed_claim(file, item, &current)? else {
        return agent_doc_queue_io::queue_claim::release(file, item);
    };
    if expected.owner.starts_with("subagent:") {
        append_completion_feedback(file, &current, &expected)?;
    }
    let released = agent_doc_queue_io::queue_claim::release_exact(file, &expected)?;
    if released.is_none() {
        bail!(
            "queue claim changed while completion feedback was published for {:?}; the newer claim remains active",
            expected.item_text
        );
    }
    Ok(released)
}

fn observed_claim(file: &Path, item: &str, current: &str) -> Result<Option<QueueClaim>> {
    let mut ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, current)?;
    // Use the same pure identity/continuation matching as release without
    // mutating the durable ledger.
    Ok(ledger.release(item))
}

fn append_completion_feedback(file: &Path, current: &str, claim: &QueueClaim) -> Result<bool> {
    let identity = serde_json::to_string(&claim.identity).context("serialize claim identity")?;
    let key_material = format!(
        "{identity}\n{}\n{}\n{}",
        claim.owner, claim.claimed_at_secs, claim.expires_at_secs
    );
    let key = agent_doc_hash::content_hash(&key_material);
    let marker = format!("<!-- agent-doc-subagent-completion key={key} -->");
    let item = one_line(&claim.item_text).replace(['“', '”'], "'");
    let owner = claim.owner.replace('`', "'");
    let note = format!(
        "> **Subagent completed:** `{owner}` returned work for queue item “{item}”. Coordinator integration and closeout are pending.\n{marker}"
    );
    if current.contains(&marker) {
        return Ok(false);
    }
    let updated = agent_doc_element_exchange::insert_prompt_line_before_boundary(current, &note)?;
    let scope = changed_component_scope(current, &updated)
        .context("subagent completion feedback must change only agent:exchange")?;
    if !scope.contains("exchange", 0) || scope.components().len() != 1 {
        bail!("subagent completion feedback escaped the first agent:exchange component");
    }
    with_component_write_scope(scope, || {
        agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
            file, current, &updated, SOURCE,
        )
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "subagent_completion_feedback_appended owner={} claimed_at={} key={} item_bytes={}",
            claim.owner,
            claim.claimed_at_secs,
            key,
            claim.item_text.len()
        ),
    );
    Ok(true)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
            claimed_at_secs: 10,
            expires_at_secs: 20,
            owner_session: None,
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
        let identity = serde_json::to_string(&claim.identity).unwrap();
        let key = agent_doc_hash::content_hash(&format!(
            "{identity}\n{}\n{}\n{}",
            claim.owner, claim.claimed_at_secs, claim.expires_at_secs
        ));
        let marker = format!("<!-- agent-doc-subagent-completion key={key} -->");
        let note = format!(
            "> **Subagent completed:** `{}` returned work for queue item “{}”. Coordinator integration and closeout are pending.\n{marker}",
            claim.owner,
            one_line(&claim.item_text)
        );
        let after =
            agent_doc_element_exchange::insert_prompt_line_before_boundary(before, &note).unwrap();

        assert_eq!(after.matches("**Subagent completed:**").count(), 1);
        assert!(after.find(&note).unwrap() < after.find("<!-- agent:boundary:keep -->").unwrap());
        assert!(after.contains("- finish sdk.md with deterministic tests"));
        assert!(after.contains(&marker));
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
        assert_eq!(
            agent_doc_document_realtime::write_policy::classify_safe_out_of_band_agent_doc_mutation(
                before, &after,
            ),
            Some("exchange"),
            "an open coordinator cycle must accept the binary completion artifact"
        );
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

        let first = report_complete(&doc, "finish sdk.md with deterministic tests")
            .unwrap()
            .unwrap();
        assert!(first.appended);
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .len(),
            1,
            "completion feedback must not release the integration guard"
        );
        let second = report_complete(&doc, "finish sdk.md with deterministic tests")
            .unwrap()
            .unwrap();
        assert!(!second.appended, "the same completion must dedupe");

        let released = release_with_feedback(&doc, "finish sdk.md with deterministic tests")
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
        assert_eq!(content.matches("**Subagent completed:**").count(), 1);
        assert!(
            content.find("**Subagent completed:**").unwrap()
                < content.find("<!-- agent:boundary:keep -->").unwrap()
        );
    }
}
