//! Evidence for an earlier attempt's outward effect (`#attemptlineage`).
//!
//! The decision lives in [`agent_doc_queue::attempt_lineage_effect`]. This
//! module gathers its inputs: the row lineage's recorded attempts, the recorded
//! hand-backs that show a pull request created, and GitHub's view of a cited
//! pull request. GitHub is read only when the process enabled observation
//! (`address_effect::enable_github_observation`), and a fixture owner
//! (`sample-org`, or a repository named `sample…`) is never sent to the
//! network, so unit tests stay offline and fail closed.

use std::path::Path;
use std::time::Duration;

use agent_doc_queue::attempt_lineage_effect::{
    CitedPullRequest, LineageAttemptEffects, LineageEffectProof, PullRequestFacts,
    PullRequestLifecycle, cited_pull_requests,
};
use agent_doc_queue::queue_claim::{QueueClaim, claim_identity};
use anyhow::Result;

/// The row lineage's attempts for `item`, with every recorded hand-back that
/// shows a pull request created folded into its own generation.
pub fn lineage_attempt_effects(file: &Path, item: &str) -> Result<Vec<LineageAttemptEffects>> {
    let ledger = crate::queue_lineage::load(file)?;
    let mut attempts = crate::queue_lineage::attempt_effects_for_item(&ledger, item);
    let identity = claim_identity(item);
    let handoffs = crate::goal_loop_handoff::load(file)?;
    for handoff in handoffs
        .handoffs
        .iter()
        .filter(|handoff| handoff.identity == identity)
    {
        let created = created_pull_requests(&handoff.response);
        if created.is_empty() {
            continue;
        }
        match attempts
            .iter_mut()
            .find(|attempt| attempt.generation == handoff.generation)
        {
            Some(attempt) => attempt.created_pull_requests.extend(created),
            None => attempts.push(LineageAttemptEffects {
                generation: handoff.generation,
                created_pull_requests: created,
                ..LineageAttemptEffects::default()
            }),
        }
    }
    attempts.sort_by_key(|attempt| attempt.generation);
    Ok(attempts)
}

/// Pull requests a line of `response` shows this attempt created.
fn created_pull_requests(response: &str) -> Vec<CitedPullRequest> {
    response
        .lines()
        .filter(|line| agent_doc_queue::prompt_pressure::response_shows_created_pull_request(line))
        .flat_map(cited_pull_requests)
        .collect()
}

/// `#attemptlineage`: the pull request `response` cites that an earlier
/// attempt of `claim`'s row produced, read with `observe`.
pub fn earlier_attempt_pull_request_with(
    file: &Path,
    claim: &QueueClaim,
    response: &str,
    observe: impl FnMut(&CitedPullRequest) -> Option<PullRequestFacts>,
) -> Result<Option<LineageEffectProof>> {
    let attempts = lineage_attempt_effects(file, &claim.item_text)?;
    Ok(
        agent_doc_queue::attempt_lineage_effect::earlier_attempt_pull_request(
            response,
            claim.generation,
            &attempts,
            observe,
        ),
    )
}

/// [`earlier_attempt_pull_request_with`] reading GitHub.
pub fn earlier_attempt_pull_request(
    file: &Path,
    claim: &QueueClaim,
    response: &str,
) -> Result<Option<LineageEffectProof>> {
    earlier_attempt_pull_request_with(file, claim, response, github_pull_request_facts)
}

/// Log the accepted proof (`#attemptlineage`).
pub fn log_accepted(file: &Path, claim: &QueueClaim, proof: &LineageEffectProof) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_lineage_effect_accepted owner={} generation={} effect_generation={} proof={} pull_request={} item_bytes={} (#attemptlineage)",
            claim.owner,
            claim.generation,
            proof.generation(),
            proof.kind(),
            proof.pull_request().url(),
            claim.item_text.trim().len(),
        ),
    );
}

fn fixture(pull_request: &CitedPullRequest) -> bool {
    pull_request.owner == "sample-org" || pull_request.repo.starts_with("sample")
}

/// GitHub's view of `pull_request`, or `None` when observation is off, the
/// owner is a fixture, or `gh` fails.
pub fn github_pull_request_facts(pull_request: &CitedPullRequest) -> Option<PullRequestFacts> {
    if !crate::address_effect::github_enabled() || fixture(pull_request) {
        return None;
    }
    let repo = format!("{}/{}", pull_request.owner, pull_request.repo);
    let number = pull_request.number.to_string();
    let stdout = crate::address_effect::gh(
        &[
            "pr",
            "view",
            number.as_str(),
            "--repo",
            repo.as_str(),
            "--json",
            "createdAt,headRefName,headRefOid,state",
            "--jq",
            "[(.createdAt|fromdateiso8601), .headRefName, .headRefOid, .state]",
        ],
        Duration::from_secs(8),
    )?;
    parse_facts(&stdout)
}

fn parse_facts(stdout: &str) -> Option<PullRequestFacts> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let fields = value.as_array()?;
    Some(PullRequestFacts {
        created_at_secs: fields.first()?.as_u64()?,
        head_ref: fields.get(1)?.as_str()?.to_string(),
        head_sha: fields.get(2)?.as_str()?.to_string(),
        state: PullRequestLifecycle::parse(fields.get(3)?.as_str()?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attemptlineage_parses_the_gh_view() {
        let facts = parse_facts(
            "[1795475668,\"fix/sdk-chat-moderated-event\",\"7913f3cb7190a2bca86b1fc6f4bcd505c91c2a51\",\"OPEN\"]\n",
        )
        .unwrap();
        assert_eq!(facts.created_at_secs, 1_795_475_668);
        assert_eq!(facts.state, PullRequestLifecycle::Open);
        assert!(parse_facts("not json").is_none());
    }

    #[test]
    fn attemptlineage_fixture_owners_never_reach_github() {
        let sample = CitedPullRequest {
            owner: "sample-org".to_string(),
            repo: "widgets".to_string(),
            number: 1,
        };
        assert!(fixture(&sample));
        // Observation is off in unit tests, so nothing is read at all.
        assert!(github_pull_request_facts(&sample).is_none());
    }

    #[test]
    fn attemptlineage_a_recorded_creation_line_names_only_its_own_pull_request() {
        let created = created_pull_requests(
            "The setup pull request is the merged https://github.com/o/r/pull/228.\nOpened https://github.com/o/r/pull/229\n",
        );
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].number, 229);
    }
}
