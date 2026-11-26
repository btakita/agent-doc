//! An outward effect an earlier attempt of the same row produced
//! (`#attemptlineage`).
//!
//! `#closepressure` keeps a row that requires a pull request open until a
//! hand-back shows one. A hand-back from a later attempt of the same row can
//! honestly cite the pull request an earlier attempt opened: the earlier worker
//! stopped, was frozen, or was requeued, and the replacement found the work
//! already done. That pull request is the row's effect, so it completes the
//! row. A pull request the row never created still does not.
//!
//! The decision here is pure. The I/O crate supplies the row lineage's
//! attempts and, when it can read GitHub, the cited pull request's facts.
//!
//! A cited pull request is the row lineage's effect when either holds:
//!
//! - **Recorded receipt.** An earlier attempt of the row recorded it: a
//!   recorded hand-back of that attempt shows it created, or the attempt's
//!   lineage receipts name the pull request's head branch or head commit.
//! - **Provenance.** The pull request was created while an earlier recorded
//!   attempt of the row was the row's current attempt, that is, after that
//!   attempt was created and before the next generation was. A pull request
//!   created before the row's first attempt existed predates the row.
//!
//! Either way the pull request must be open or merged. A closed, unmerged pull
//! request is not a completed effect. Unknown facts fail closed: without a
//! recorded receipt and without readable facts, the cited pull request does not
//! count.

/// One pull request a hand-back names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedPullRequest {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl CitedPullRequest {
    /// The canonical `https://github.com/<owner>/<repo>/pull/<n>` spelling.
    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/{}/pull/{}",
            self.owner, self.repo, self.number
        )
    }

    fn same(&self, other: &CitedPullRequest) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repo.eq_ignore_ascii_case(&other.repo)
            && self.number == other.number
    }
}

/// What durable state recorded about one attempt of the row lineage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageAttemptEffects {
    pub generation: u64,
    /// When the attempt was created. `None` when no receipt carries a time.
    pub created_at_secs: Option<u64>,
    /// Branches and refs the attempt's receipts name (`branch`, `pushed_ref`,
    /// `sink_ref`), as recorded.
    pub refs: Vec<String>,
    /// Commits the attempt's receipts name (`source_sha`, `integration_sha`).
    pub shas: Vec<String>,
    /// Pull requests a recorded hand-back of this attempt shows it created.
    pub created_pull_requests: Vec<CitedPullRequest>,
}

/// GitHub's view of one pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestFacts {
    pub created_at_secs: u64,
    pub head_ref: String,
    pub head_sha: String,
    pub state: PullRequestLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestLifecycle {
    Open,
    Merged,
    Closed,
}

impl PullRequestLifecycle {
    /// Parse GitHub's `state` field (`OPEN`, `MERGED`, `CLOSED`).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "OPEN" => Some(Self::Open),
            "MERGED" => Some(Self::Merged),
            "CLOSED" => Some(Self::Closed),
            _ => None,
        }
    }

    /// A merged or closed pull request ends a goal loop on that pull request
    /// (`#mergedprgoal`). Another address or review generation cannot land.
    pub fn ends_goal_loop(self) -> bool {
        matches!(self, Self::Merged | Self::Closed)
    }
}

/// Why a cited pull request counts as the row lineage's effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineageEffectProof {
    /// An earlier attempt's own receipt names it.
    RecordedReceipt {
        pull_request: CitedPullRequest,
        generation: u64,
    },
    /// It was created while an earlier attempt was the row's current attempt.
    Provenance {
        pull_request: CitedPullRequest,
        generation: u64,
    },
}

impl LineageEffectProof {
    pub fn pull_request(&self) -> &CitedPullRequest {
        match self {
            Self::RecordedReceipt { pull_request, .. } | Self::Provenance { pull_request, .. } => {
                pull_request
            }
        }
    }

    pub fn generation(&self) -> u64 {
        match self {
            Self::RecordedReceipt { generation, .. } | Self::Provenance { generation, .. } => {
                *generation
            }
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::RecordedReceipt { .. } => "recorded_receipt",
            Self::Provenance { .. } => "provenance",
        }
    }
}

/// The one pull request a goal row is about (`#mergedprgoal`).
///
/// The claim text wins when it cites exactly one pull request, which is how a
/// pipe stage keeps its parent after agent-doc nests the later stage:
/// `https://github.com/…/pull/102: #address: #adversarial-pr`. A bare stage
/// such as `#adversarial-pr` uses `ancestor_pull_url`, the single pull-request
/// heading above that row. Zero or several citations is `None`.
pub fn goal_subject_pull_request(
    item_text: &str,
    ancestor_pull_url: Option<&str>,
) -> Option<CitedPullRequest> {
    let cited = cited_pull_requests(item_text);
    match cited.len() {
        1 => cited.into_iter().next(),
        0 => ancestor_pull_url.and_then(|url| {
            let cited = cited_pull_requests(url);
            (cited.len() == 1)
                .then(|| cited.into_iter().next())
                .flatten()
        }),
        _ => None,
    }
}

/// Every `https://github.com/<owner>/<repo>/pull/<n>` URL in `text`, outside
/// quoted (`>`) lines, deduplicated in order.
pub fn cited_pull_requests(text: &str) -> Vec<CitedPullRequest> {
    const PREFIX: &str = "https://github.com/";
    let mut cited: Vec<CitedPullRequest> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with('>') {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find(PREFIX) {
            let after = &rest[start + PREFIX.len()..];
            rest = after;
            let mut parts = after.splitn(4, '/');
            let (Some(owner), Some(repo), Some(pull), Some(tail)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if pull != "pull" || !valid_segment(owner) || !valid_segment(repo) {
                continue;
            }
            let digits: String = tail.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            let Ok(number) = digits.parse::<u64>() else {
                continue;
            };
            let pull_request = CitedPullRequest {
                owner: owner.to_string(),
                repo: repo.to_string(),
                number,
            };
            if !cited.iter().any(|seen| seen.same(&pull_request)) {
                cited.push(pull_request);
            }
        }
    }
    cited
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// `#attemptlineage`: the first pull request `response` cites that an earlier
/// attempt of this row lineage produced, with its proof.
///
/// `current_generation` is the attempt that hands back now; only attempts with
/// a lower generation are earlier. `observe` reads one pull request's facts and
/// returns `None` when they are unavailable.
pub fn earlier_attempt_pull_request(
    response: &str,
    current_generation: u64,
    attempts: &[LineageAttemptEffects],
    mut observe: impl FnMut(&CitedPullRequest) -> Option<PullRequestFacts>,
) -> Option<LineageEffectProof> {
    let mut earlier: Vec<&LineageAttemptEffects> = attempts
        .iter()
        .filter(|attempt| attempt.generation < current_generation)
        .collect();
    if earlier.is_empty() {
        return None;
    }
    earlier.sort_by_key(|attempt| attempt.generation);
    for pull_request in cited_pull_requests(response) {
        if let Some(attempt) = earlier.iter().find(|attempt| {
            attempt
                .created_pull_requests
                .iter()
                .any(|created| created.same(&pull_request))
        }) {
            return Some(LineageEffectProof::RecordedReceipt {
                generation: attempt.generation,
                pull_request,
            });
        }
        let Some(facts) = observe(&pull_request) else {
            continue;
        };
        if facts.state == PullRequestLifecycle::Closed {
            continue;
        }
        if let Some(attempt) = earlier
            .iter()
            .find(|attempt| attempt_names_head(attempt, &facts))
        {
            return Some(LineageEffectProof::RecordedReceipt {
                generation: attempt.generation,
                pull_request,
            });
        }
        if let Some(generation) =
            created_during_earlier_attempt(&earlier, attempts, facts.created_at_secs)
        {
            return Some(LineageEffectProof::Provenance {
                generation,
                pull_request,
            });
        }
    }
    None
}

fn attempt_names_head(attempt: &LineageAttemptEffects, facts: &PullRequestFacts) -> bool {
    let head_ref = facts.head_ref.trim();
    let head_sha = facts.head_sha.trim();
    attempt
        .refs
        .iter()
        .any(|recorded| !head_ref.is_empty() && branch_name(recorded) == head_ref)
        || attempt.shas.iter().any(|recorded| {
            let recorded = recorded.trim();
            recorded.len() >= 7
                && head_sha.len() >= 7
                && (head_sha.starts_with(recorded) || recorded.starts_with(head_sha))
        })
}

/// `refs/heads/x`, `refs/remotes/<remote>/x`, and `<remote>/x` all name `x`.
fn branch_name(recorded: &str) -> &str {
    let recorded = recorded.trim();
    if let Some(rest) = recorded.strip_prefix("refs/heads/") {
        return rest;
    }
    if let Some(rest) = recorded.strip_prefix("refs/remotes/") {
        return rest.split_once('/').map_or(rest, |(_, branch)| branch);
    }
    recorded
        .strip_prefix("origin/")
        .or_else(|| recorded.strip_prefix("upstream/"))
        .unwrap_or(recorded)
}

/// The earlier attempt whose tenure covers `created_at`: from its creation to
/// the creation of the next generation of the row.
fn created_during_earlier_attempt(
    earlier: &[&LineageAttemptEffects],
    all: &[LineageAttemptEffects],
    created_at: u64,
) -> Option<u64> {
    earlier.iter().find_map(|attempt| {
        let start = attempt.created_at_secs?;
        if created_at < start {
            return None;
        }
        let next_start = all
            .iter()
            .filter(|later| later.generation > attempt.generation)
            .filter_map(|later| later.created_at_secs)
            .min();
        match next_start {
            Some(end) if created_at >= end => None,
            _ => Some(attempt.generation),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PR402: &str = "https://github.com/haiven-dev/haiven-sdk/pull/402";

    fn attempt(generation: u64, created_at: u64) -> LineageAttemptEffects {
        LineageAttemptEffects {
            generation,
            created_at_secs: Some(created_at),
            ..LineageAttemptEffects::default()
        }
    }

    fn facts(created_at: u64, state: PullRequestLifecycle) -> PullRequestFacts {
        PullRequestFacts {
            created_at_secs: created_at,
            head_ref: "fix/sdk-chat-moderated-event".to_string(),
            head_sha: "7913f3cb7190a2bca86b1fc6f4bcd505c91c2a51".to_string(),
            state,
        }
    }

    #[test]
    fn mergedprgoal_a_pipe_claim_names_one_pull_request_and_a_merged_one_ends_the_loop() {
        let pipe = "https://github.com/haiven-dev/haiven-docs/pull/102: #address: #adversarial-pr";
        let subject = goal_subject_pull_request(pipe, None).unwrap();
        assert_eq!(subject.number, 102);
        assert_eq!(subject.repo, "haiven-docs");
        assert!(PullRequestLifecycle::Merged.ends_goal_loop());
        assert!(PullRequestLifecycle::Closed.ends_goal_loop());
        assert!(!PullRequestLifecycle::Open.ends_goal_loop());
        let bare = goal_subject_pull_request(
            "#adversarial-pr",
            Some("https://github.com/haiven-dev/haiven-docs/pull/102"),
        )
        .unwrap();
        assert_eq!(bare.url(), subject.url());
        assert!(goal_subject_pull_request("#adversarial-pr", None).is_none());
        assert!(
            goal_subject_pull_request(
                "https://github.com/acme/a/pull/1 and https://github.com/acme/b/pull/2",
                Some("https://github.com/acme/a/pull/1"),
            )
            .is_none()
        );
    }

    /// The `stgsdkmod` sequence: generation 1 opened #402 at 22:14Z and was
    /// frozen; generations 2-4 each found it finished and cited it.
    #[test]
    fn attemptlineage_a_pull_request_an_earlier_generation_opened_completes_the_row() {
        let attempts = [
            attempt(1, 1_795_471_189),
            attempt(2, 1_795_481_542),
            attempt(3, 1_795_482_076),
            attempt(4, 1_795_482_105),
        ];
        let response =
            format!("**Summary**\nPR {PR402} is finished. Commits, both by generation 1.");
        for current in 2..=4 {
            let proof = earlier_attempt_pull_request(&response, current, &attempts, |_| {
                Some(facts(1_795_475_668, PullRequestLifecycle::Open))
            })
            .expect("generation 1 created #402 during its tenure");
            assert_eq!(proof.generation(), 1);
            assert_eq!(proof.kind(), "provenance");
            assert_eq!(proof.pull_request().url(), PR402);
        }
    }

    #[test]
    fn attemptlineage_a_pull_request_that_predates_the_row_still_does_not_count() {
        let attempts = [attempt(1, 1_795_471_189), attempt(2, 1_795_481_542)];
        let response = format!("The setup pull request is the merged {PR402}.");
        assert_eq!(
            earlier_attempt_pull_request(&response, 2, &attempts, |_| {
                Some(facts(1_795_000_000, PullRequestLifecycle::Merged))
            }),
            None
        );
    }

    #[test]
    fn attemptlineage_the_current_attempt_and_unknown_facts_are_not_earlier_effects() {
        let attempts = [attempt(1, 1_795_471_189), attempt(2, 1_795_481_542)];
        let response = format!("{PR402} is finished.");
        // Created during generation 2's own tenure: that attempt must show it
        // opened the pull request (`#closepressure`), not cite it.
        assert_eq!(
            earlier_attempt_pull_request(&response, 2, &attempts, |_| {
                Some(facts(1_795_481_600, PullRequestLifecycle::Open))
            }),
            None
        );
        // No facts and no receipt: fail closed.
        assert_eq!(
            earlier_attempt_pull_request(&response, 2, &attempts, |_| None),
            None
        );
        // A first attempt has no earlier attempt.
        assert_eq!(
            earlier_attempt_pull_request(&response, 1, &attempts, |_| {
                Some(facts(1_795_475_668, PullRequestLifecycle::Open))
            }),
            None
        );
    }

    #[test]
    fn attemptlineage_a_closed_unmerged_pull_request_is_not_a_completed_effect() {
        let attempts = [attempt(1, 1_795_471_189), attempt(2, 1_795_481_542)];
        let response = format!("{PR402} is finished.");
        assert_eq!(
            earlier_attempt_pull_request(&response, 2, &attempts, |_| {
                Some(facts(1_795_475_668, PullRequestLifecycle::Closed))
            }),
            None
        );
    }

    #[test]
    fn attemptlineage_recorded_receipts_prove_the_effect_without_timing() {
        let mut first = attempt(1, 1_795_471_189);
        first.refs = vec!["refs/remotes/origin/fix/sdk-chat-moderated-event".to_string()];
        let attempts = [first, attempt(2, 1_795_481_542)];
        let response = format!("{PR402} is finished.");
        // The head branch an earlier attempt recorded, even when the pull
        // request's creation time falls outside its tenure.
        let proof = earlier_attempt_pull_request(&response, 2, &attempts, |_| {
            Some(facts(1_795_481_900, PullRequestLifecycle::Open))
        })
        .unwrap();
        assert_eq!(proof.kind(), "recorded_receipt");

        // A recorded creation hand-back needs no remote read.
        let mut first = attempt(1, 1_795_471_189);
        first.created_pull_requests = cited_pull_requests(&format!("Opened {PR402}"));
        let attempts = [first, attempt(2, 1_795_481_542)];
        let proof = earlier_attempt_pull_request(&response, 2, &attempts, |_| None).unwrap();
        assert_eq!(proof.kind(), "recorded_receipt");
        assert_eq!(proof.generation(), 1);
    }

    #[test]
    fn attemptlineage_cited_pull_requests_skip_quoted_prompt_lines() {
        let text = format!(
            "> **Queue prompt:**\n> replace the run that opened https://github.com/o/r/pull/1\n\nDone: {PR402}, see {PR402}/files.\n"
        );
        let cited = cited_pull_requests(&text);
        assert_eq!(cited.len(), 1);
        assert_eq!(cited[0].url(), PR402);
    }
}
