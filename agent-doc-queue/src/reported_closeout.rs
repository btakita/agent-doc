//! Coordinator closeout for a `Reported` queue attempt (`#reportedcloseout`).
//!
//! A worker that ran `queue report-complete` leaves its attempt `Reported`
//! (`☑️`): the coordinator still has to integrate, verify, and close it. Before
//! this module nothing told the coordinator *how*: `queue claims` gave every
//! reported attempt an empty `next_action_argv`, and `agent-doc plan` emitted
//! only claim/spawn steps for the next heads. A coordinator that follows the
//! plan therefore started new work every cycle while finished work never
//! closed (haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-04: nine reported
//! attempts, 3h to 42h old).
//!
//! [`reported_claim_closeout`] derives the exact command sequence that closes
//! one reported attempt from durable facts only, never from the row's text:
//!
//! * the attempt's identity, owner, and generation (the `queue consume` fence);
//! * its lineage evidence, classified by the IO layer into
//!   [`ReportedRepoEvidence`] with the same rule `queue consume` enforces
//!   (`#subagentintegrationproof`): an attempt with no repository facts is
//!   prose-only; one with repository facts needs a verified `integrated`
//!   receipt first;
//! * for unintegrated repository work, whether the designated sink ref
//!   already contains the worker's source commit ([`SinkContainment`]).
//!
//! The result always starts with a concrete, executable step. When a fact the
//! closeout needs was never recorded, the first step inspects the lineage and
//! [`ReportedClaimCloseout::missing_facts`] names exactly which facts are
//! missing; later steps that need them carry `<placeholder>` arguments and
//! `concrete: false`.
//!
//! This module is pure: no clock, no storage, no git. The caller supplies the
//! evidence and the commit counts.

use agent_doc_element_queue::QueueItemIdentity;
use serde::{Deserialize, Serialize};

use crate::queue_claim::{QueueClaim, QueueClaimState, claim_item_handle};

/// Repository facts aggregated from one attempt's lineage receipts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedRepoFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sink_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sink_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pushed_ref: Option<String>,
}

impl ReportedRepoFacts {
    /// The integration-proof facts that were never recorded, in argument order.
    pub fn missing(&self) -> Vec<String> {
        [
            ("source_repo", self.source_repo.is_none()),
            ("source_sha", self.source_sha.is_none()),
            ("sink_ref", self.sink_ref.is_none()),
        ]
        .into_iter()
        .filter(|(_, missing)| *missing)
        .map(|(name, _)| name.to_string())
        .collect()
    }

    /// The sink repository: explicit, else the source repository (a worker
    /// that integrates in its own repository records only one).
    fn sink_repo(&self) -> Option<&str> {
        self.sink_repo.as_deref().or(self.source_repo.as_deref())
    }
}

/// Whether the designated sink ref already contains the worker's commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SinkContainment {
    /// `git merge-base --is-ancestor <source> <sink_ref>` succeeds in the sink.
    Contains,
    /// The sink has the source commit, but the sink ref does not reach it.
    DoesNotContain,
    /// The source commit is not an object in the sink repository.
    SourceAbsentFromSink,
    /// The check could not run (missing repository, missing ref, git error).
    Unverifiable { reason: String },
}

/// Lineage evidence for one exact reported attempt, classified with the rule
/// `queue consume` enforces before it strikes a reported row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportedRepoEvidence {
    /// No lineage attempt matches this identity and generation.
    LineageMissing,
    /// More than one lineage attempt matches; nothing may be picked silently.
    LineageAmbiguous,
    /// The attempt records no repository, worktree, or commit facts.
    ProseOnly,
    /// The latest `integrated` receipt verifies against the sink now.
    IntegratedVerified,
    /// An `integrated` receipt exists but no longer verifies.
    IntegratedUnverified {
        facts: ReportedRepoFacts,
        reason: String,
    },
    /// Repository work with no `integrated` receipt. `sink` is `None` when the
    /// facts needed to check containment were never recorded.
    Unintegrated {
        facts: ReportedRepoFacts,
        sink: Option<SinkContainment>,
    },
}

/// Which closeout a reported attempt needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportedCloseoutShape {
    /// No repository work: strike the row.
    ProseConsume,
    /// Repository work with verified integration proof: strike the row.
    IntegratedConsume,
    /// The sink contains (or may contain) the work: verify, record the
    /// `integrated` receipt, then strike.
    IntegrationProof,
    /// The sink does not contain the work: integrate it first.
    IntegrationRequired,
    /// The recorded integration proof no longer verifies: re-prove it.
    IntegrationProofStale,
    /// A fact the closeout needs was never recorded.
    MissingFacts,
}

impl ReportedCloseoutShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProseConsume => "prose_consume",
            Self::IntegratedConsume => "integrated_consume",
            Self::IntegrationProof => "integration_proof",
            Self::IntegrationRequired => "integration_required",
            Self::IntegrationProofStale => "integration_proof_stale",
            Self::MissingFacts => "missing_facts",
        }
    }
}

/// What one closeout step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportedCloseoutAction {
    InspectLineage,
    FetchSource,
    Integrate,
    VerifyIntegration,
    RecordIntegration,
    Consume,
}

impl ReportedCloseoutAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InspectLineage => "inspect_lineage",
            Self::FetchSource => "fetch_source",
            Self::Integrate => "integrate",
            Self::VerifyIntegration => "verify_integration",
            Self::RecordIntegration => "record_integration",
            Self::Consume => "consume",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedCloseoutStep {
    pub action: ReportedCloseoutAction,
    pub argv: Vec<String>,
    /// False when the step cannot run as written yet: `argv` carries a
    /// `<placeholder>` for a missing fact, or (for `consume`) the binary
    /// refuses it until those facts are recorded.
    pub concrete: bool,
    pub detail: String,
}

/// The ordered commands that close one reported attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedClaimCloseout {
    pub shape: ReportedCloseoutShape,
    pub detail: String,
    /// Durable facts the closeout needs that were never recorded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_facts: Vec<String>,
    pub steps: Vec<ReportedCloseoutStep>,
}

impl ReportedClaimCloseout {
    /// The first step's argv. Construction guarantees it is concrete.
    pub fn next_action_argv(&self) -> Option<&[String]> {
        self.steps
            .iter()
            .find(|step| step.concrete)
            .map(|step| step.argv.as_slice())
    }
}

/// Join argv into one copy-pasteable POSIX shell command.
pub fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_word(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_word(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '_' | '.' | '/' | ':' | '@' | '=' | '+' | ',')
        });
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

fn consume_step(document: &str, claim: &QueueClaim) -> ReportedCloseoutStep {
    match &claim.identity {
        QueueItemIdentity::Id(id) => ReportedCloseoutStep {
            action: ReportedCloseoutAction::Consume,
            argv: argv(&["agent-doc", "queue", "consume", document, "--ack-id", id]),
            concrete: true,
            detail: format!(
                "strike the reported row; the tracked item stays open. Only when the tracked work itself is complete, pass `--done {id}` to `agent-doc finalize {document}` instead"
            ),
        },
        QueueItemIdentity::FreeText(_) => {
            let owner = format!("{}#{}", claim.owner, claim.generation);
            ReportedCloseoutStep {
                action: ReportedCloseoutAction::Consume,
                argv: argv(&[
                    "agent-doc",
                    "queue",
                    "consume",
                    document,
                    "--ack-text",
                    &claim_item_handle(&claim.item_text),
                    "--ack-owner",
                    &owner,
                ]),
                concrete: true,
                detail: "strike the reported free-text row, fenced to this exact worker attempt"
                    .to_string(),
            }
        }
    }
}

fn inspect_step(document: &str, claim: &QueueClaim, detail: String) -> ReportedCloseoutStep {
    let generation = claim.generation.to_string();
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::InspectLineage,
        argv: argv(&[
            "agent-doc",
            "queue",
            "lineage",
            document,
            "--item",
            &claim_item_handle(&claim.item_text),
            "--generation",
            &generation,
            "--verbose",
        ]),
        concrete: true,
        detail,
    }
}

fn verify_step(facts: &ReportedRepoFacts) -> ReportedCloseoutStep {
    let sink_repo = facts.sink_repo().unwrap_or("<sink-repo>");
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::VerifyIntegration,
        argv: argv(&[
            "git",
            "-C",
            sink_repo,
            "merge-base",
            "--is-ancestor",
            source,
            sink_ref,
        ]),
        concrete: facts.missing().is_empty(),
        detail: format!(
            "exit status 0 proves designated sink ref {sink_ref} contains source commit {source}; any other status means it is not integrated yet"
        ),
    }
}

fn record_step(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
) -> ReportedCloseoutStep {
    let generation = claim.generation.to_string();
    let source_repo = facts.source_repo.as_deref().unwrap_or("<source-repo>");
    let sink_repo = facts.sink_repo().unwrap_or("<sink-repo>");
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::RecordIntegration,
        argv: argv(&[
            "agent-doc",
            "queue",
            "lineage-record",
            document,
            "--item",
            &claim_item_handle(&claim.item_text),
            "--generation",
            &generation,
            "--event",
            "integrated",
            "--source-repo",
            source_repo,
            "--sink-repo",
            sink_repo,
            "--source-sha",
            source,
            "--integration-sha",
            source,
            "--sink-ref",
            sink_ref,
            "--method",
            "fast-forward",
        ]),
        concrete: facts.missing().is_empty(),
        detail: "record the verified edge: the source commit itself is reachable from the designated sink ref. If the work landed as a different sink commit (rebase, cherry-pick, merge), pass that commit as --integration-sha with the matching --method instead".to_string(),
    }
}

/// The fast-forward-only command that publishes `source` onto `sink_ref`.
/// `git push` refuses a non-fast-forward update and an update of a branch
/// checked out in a worktree, so a refusal is safe and leaves the sink alone.
fn integrate_step(facts: &ReportedRepoFacts) -> Option<ReportedCloseoutStep> {
    let source = facts.source_sha.as_deref()?;
    let sink_ref = facts.sink_ref.as_deref()?;
    let sink_repo = facts.sink_repo()?;
    let (repo, remote, destination) = match sink_ref.strip_prefix("refs/remotes/") {
        Some(rest) => {
            let (remote, branch) = rest.split_once('/')?;
            (
                facts.source_repo.as_deref().unwrap_or(sink_repo),
                remote.to_string(),
                format!("refs/heads/{branch}"),
            )
        }
        None => (
            sink_repo,
            ".".to_string(),
            if sink_ref.starts_with("refs/") {
                sink_ref.to_string()
            } else {
                format!("refs/heads/{sink_ref}")
            },
        ),
    };
    let refspec = format!("{source}:{destination}");
    Some(ReportedCloseoutStep {
        action: ReportedCloseoutAction::Integrate,
        argv: argv(&["git", "-C", repo, "push", &remote, &refspec]),
        concrete: true,
        detail: format!(
            "fast-forward {sink_ref} to the worker commit {source}. A refusal (non-fast-forward, or the branch is checked out) changes nothing: then rebase or cherry-pick the work onto {sink_ref}, verify it, and record that sink commit with the matching --method"
        ),
    })
}

fn fetch_step(facts: &ReportedRepoFacts) -> Option<ReportedCloseoutStep> {
    let source = facts.source_sha.as_deref()?;
    let source_repo = facts.source_repo.as_deref()?;
    let sink_repo = facts.sink_repo()?;
    Some(ReportedCloseoutStep {
        action: ReportedCloseoutAction::FetchSource,
        argv: argv(&["git", "-C", sink_repo, "fetch", source_repo, source]),
        concrete: true,
        detail: format!(
            "the sink repository {sink_repo} has no object {source}; fetch it from the source repository, then rerun `agent-doc queue claims` to recompute the next step"
        ),
    })
}

/// The closeout for one reported attempt, or `None` for any other state.
pub fn reported_claim_closeout(
    document: &str,
    claim: &QueueClaim,
    evidence: &ReportedRepoEvidence,
) -> Option<ReportedClaimCloseout> {
    if claim.state != QueueClaimState::Reported {
        return None;
    }
    let consume = consume_step(document, claim);
    let closeout = match evidence {
        ReportedRepoEvidence::ProseOnly => ReportedClaimCloseout {
            shape: ReportedCloseoutShape::ProseConsume,
            detail: "the attempt records no repository work; integrate the reported result into this turn's response and strike the row".to_string(),
            missing_facts: Vec::new(),
            steps: vec![consume],
        },
        ReportedRepoEvidence::IntegratedVerified => ReportedClaimCloseout {
            shape: ReportedCloseoutShape::IntegratedConsume,
            detail: "lineage already records a verified integration; only the queue closeout remains".to_string(),
            missing_facts: Vec::new(),
            steps: vec![consume],
        },
        ReportedRepoEvidence::LineageMissing | ReportedRepoEvidence::LineageAmbiguous => {
            let (fact, detail) = if *evidence == ReportedRepoEvidence::LineageMissing {
                (
                    "queue_lineage_attempt",
                    "no lineage attempt records this generation, so prose-only versus repository work is unknown and `queue consume` refuses the row",
                )
            } else {
                (
                    "unambiguous_queue_lineage_attempt",
                    "more than one lineage attempt matches this generation; `queue consume` refuses to pick one",
                )
            };
            ReportedClaimCloseout {
                shape: ReportedCloseoutShape::MissingFacts,
                detail: detail.to_string(),
                missing_facts: vec![fact.to_string()],
                steps: vec![inspect_step(document, claim, detail.to_string()), consume],
            }
        }
        ReportedRepoEvidence::IntegratedUnverified { facts, reason } => {
            let missing = facts.missing();
            let detail = format!(
                "the recorded integration proof no longer verifies ({reason}); keep the row live and re-prove it against the designated sink"
            );
            let mut steps = vec![inspect_step(document, claim, detail.clone())];
            steps.push(verify_step(facts));
            steps.push(record_step(document, claim, facts));
            steps.push(consume);
            ReportedClaimCloseout {
                shape: if missing.is_empty() {
                    ReportedCloseoutShape::IntegrationProofStale
                } else {
                    ReportedCloseoutShape::MissingFacts
                },
                detail,
                missing_facts: missing,
                steps,
            }
        }
        ReportedRepoEvidence::Unintegrated { facts, sink } => {
            let missing = facts.missing();
            if !missing.is_empty() {
                let detail = format!(
                    "repository work is recorded without integration proof, and lineage lacks {}; record them on the attempt before integrating",
                    missing.join(", ")
                );
                ReportedClaimCloseout {
                    shape: ReportedCloseoutShape::MissingFacts,
                    steps: vec![
                        inspect_step(document, claim, detail.clone()),
                        verify_step(facts),
                        record_step(document, claim, facts),
                        consume,
                    ],
                    detail,
                    missing_facts: missing,
                }
            } else {
                match sink {
                    Some(SinkContainment::DoesNotContain) => {
                        let mut steps = Vec::new();
                        steps.extend(integrate_step(facts));
                        steps.extend([verify_step(facts), record_step(document, claim, facts), consume]);
                        ReportedClaimCloseout {
                            shape: ReportedCloseoutShape::IntegrationRequired,
                            detail: format!(
                                "designated sink ref {} does not contain source commit {}; integrate it before recording proof",
                                facts.sink_ref.as_deref().unwrap_or_default(),
                                facts.source_sha.as_deref().unwrap_or_default(),
                            ),
                            missing_facts: Vec::new(),
                            steps,
                        }
                    }
                    Some(SinkContainment::SourceAbsentFromSink) => {
                        let mut steps = Vec::new();
                        steps.extend(fetch_step(facts));
                        steps.extend([verify_step(facts), record_step(document, claim, facts), consume]);
                        ReportedClaimCloseout {
                            shape: ReportedCloseoutShape::IntegrationRequired,
                            detail: format!(
                                "source commit {} is not present in sink repository {}",
                                facts.source_sha.as_deref().unwrap_or_default(),
                                facts.sink_repo().unwrap_or_default(),
                            ),
                            missing_facts: Vec::new(),
                            steps,
                        }
                    }
                    Some(SinkContainment::Contains) | Some(SinkContainment::Unverifiable { .. }) | None => {
                        let detail = match sink {
                            Some(SinkContainment::Unverifiable { reason }) => format!(
                                "sink containment could not be checked ({reason}); the verification step decides"
                            ),
                            _ => format!(
                                "designated sink ref {} contains source commit {}; record the integration proof, then strike the row",
                                facts.sink_ref.as_deref().unwrap_or_default(),
                                facts.source_sha.as_deref().unwrap_or_default(),
                            ),
                        };
                        ReportedClaimCloseout {
                            shape: ReportedCloseoutShape::IntegrationProof,
                            detail,
                            missing_facts: Vec::new(),
                            steps: vec![
                                verify_step(facts),
                                record_step(document, claim, facts),
                                consume,
                            ],
                        }
                    }
                }
            }
        }
    };
    let mut closeout = closeout;
    if !closeout.missing_facts.is_empty() {
        // `queue consume` refuses the row until the missing facts exist, so
        // the closeout is not runnable as written yet.
        for step in &mut closeout.steps {
            if step.action == ReportedCloseoutAction::Consume {
                step.concrete = false;
            }
        }
    }
    debug_assert!(closeout.steps.first().is_some_and(|step| step.concrete));
    Some(closeout)
}

/// When a reported attempt became coordinator debt, and which record says so.
/// Attempts reported before `reported_at_secs` existed fall back to the worker
/// start (an upper bound on the debt's age), then the claim.
pub fn reported_since(claim: &QueueClaim) -> (u64, &'static str) {
    if let Some(at) = claim.reported_at_secs {
        (at, "reported")
    } else if let Some(worker) = claim.worker.as_ref() {
        (worker.started_at_secs, "worker started")
    } else {
        (claim.claimed_at_secs, "claimed")
    }
}

/// A reported attempt is overdue once it survives more than one committed
/// cycle: the commit that lands its report is the first, and any further
/// commit of the document with the attempt still reported is a cycle that
/// started or finished other work instead of closing it.
pub const REPORTED_CLOSEOUT_COMMITTED_CYCLE_ALLOWANCE: usize = 1;

pub fn reported_closeout_overdue(commits_since_report: usize) -> bool {
    commits_since_report > REPORTED_CLOSEOUT_COMMITTED_CYCLE_ALLOWANCE
}

/// The session-check warning for an overdue reported attempt.
pub fn overdue_reported_closeout_warning(
    claim: &QueueClaim,
    closeout: &ReportedClaimCloseout,
    commits_since_report: usize,
) -> String {
    let (_, since) = reported_since(claim);
    let next = closeout
        .next_action_argv()
        .map(shell_command)
        .unwrap_or_default();
    let missing = if closeout.missing_facts.is_empty() {
        String::new()
    } else {
        format!(" Missing facts: {}.", closeout.missing_facts.join(", "))
    };
    format!(
        "reported queue attempt {:?} ({}#{}) is still open after {commits_since_report} committed cycle(s) since {since} ({}); close it before starting new work. Next: `{next}`.{missing}",
        claim_item_handle(&claim.item_text),
        claim.owner,
        claim.generation,
        closeout.shape.as_str(),
    )
}

/// One reported attempt's closeout as the plan presents it
/// (`#reportedcloseout`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedCloseoutObligation {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub closeout: ReportedClaimCloseout,
}

impl ReportedCloseoutObligation {
    pub fn new(claim: &QueueClaim, closeout: ReportedClaimCloseout) -> Self {
        Self {
            item: claim_item_handle(&claim.item_text),
            owner: claim.owner.clone(),
            generation: claim.generation,
            closeout,
        }
    }

    /// One plan `required_commands` entry per closeout step, in order. Every
    /// entry says it precedes new claim/spawn work: the plan lists these
    /// first so finished work closes before new work starts.
    pub fn required_commands(&self) -> Vec<String> {
        let total = self.closeout.steps.len();
        let missing = if self.closeout.missing_facts.is_empty() {
            String::new()
        } else {
            format!(
                " Not runnable as written until lineage records: {}.",
                self.closeout.missing_facts.join(", ")
            )
        };
        self.closeout
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                format!(
                    "Close reported queue item {:?} ({}#{}, {}) before any new queue claim or spawn, step {}/{total} ({}): run `{}`. {}.{}",
                    self.item,
                    self.owner,
                    self.generation,
                    self.closeout.shape.as_str(),
                    index + 1,
                    step.action.as_str(),
                    shell_command(&step.argv),
                    step.detail.trim_end_matches('.'),
                    if step.concrete { "" } else { missing.as_str() },
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{QueueWorkerBinding, claim_identity};

    fn reported(text: &str, owner: &str) -> QueueClaim {
        QueueClaim {
            identity: claim_identity(text),
            item_text: text.to_string(),
            owner: owner.to_string(),
            generation: 3,
            state: QueueClaimState::Reported,
            lost_from: None,
            completion_recovered_after_loss: false,
            publication_state: None,
            worker: Some(QueueWorkerBinding {
                worker_id: "worker-1".to_string(),
                started_at_secs: 100,
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: 90,
            expires_at_secs: 7290,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            reported_at_secs: None,
        }
    }

    fn facts() -> ReportedRepoFacts {
        ReportedRepoFacts {
            source_repo: Some("/repo/worktree".to_string()),
            sink_repo: Some("/repo/main".to_string()),
            source_sha: Some("abc123".to_string()),
            sink_ref: Some("refs/heads/main".to_string()),
            pushed_ref: None,
        }
    }

    const DOC: &str = "tasks/session.md";

    #[test]
    fn prose_free_text_claim_closes_with_an_owner_fenced_consume() {
        let claim = reported(
            "https://example.test/pull/207: #review [#q-5e1afb7bfdd7]",
            "subagent:c631sdk207",
        );
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::ProseOnly).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::ProseConsume);
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "agent-doc",
                "queue",
                "consume",
                DOC,
                "--ack-text",
                "https://example.test/pull/207: #review [#q-5e1afb7bfdd7]",
                "--ack-owner",
                "subagent:c631sdk207#3",
            ])
        );
        assert_eq!(closeout.steps.len(), 1);
        assert!(closeout.missing_facts.is_empty());
    }

    #[test]
    fn prose_id_claim_closes_with_ack_id() {
        let claim = reported("do [#clidocsreleasecleanup]", "subagent:c948clidocs");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::ProseOnly).unwrap();
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "agent-doc",
                "queue",
                "consume",
                DOC,
                "--ack-id",
                "clidocsreleasecleanup"
            ])
        );
    }

    #[test]
    fn local_publication_state_does_not_change_a_prose_closeout() {
        let mut claim = reported("Draft the open questions [#q-21230b62be31]", "subagent:oq");
        claim.publication_state = Some(agent_doc_element_queue::QueueReportState::Local);
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::ProseOnly).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::ProseConsume);
        assert_eq!(closeout.steps[0].action, ReportedCloseoutAction::Consume);
    }

    #[test]
    fn already_integrated_claim_only_needs_consume() {
        let claim = reported(
            "https://example.test/pull/9: #rebase [#q-3d2bbe420d92]",
            "subagent:pr9",
        );
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::IntegratedVerified)
                .unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegratedConsume);
        assert_eq!(closeout.steps.len(), 1);
        assert_eq!(closeout.steps[0].action, ReportedCloseoutAction::Consume);
    }

    #[test]
    fn published_repo_work_in_the_sink_verifies_records_then_consumes() {
        let mut claim = reported("do [#publishtelemetry]", "subagent:telem");
        claim.publication_state = Some(agent_doc_element_queue::QueueReportState::Published);
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::Contains),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationProof);
        let actions = closeout
            .steps
            .iter()
            .map(|step| step.action)
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [
                ReportedCloseoutAction::VerifyIntegration,
                ReportedCloseoutAction::RecordIntegration,
                ReportedCloseoutAction::Consume,
            ]
        );
        assert_eq!(
            closeout.steps[0].argv,
            argv(&[
                "git",
                "-C",
                "/repo/main",
                "merge-base",
                "--is-ancestor",
                "abc123",
                "refs/heads/main"
            ])
        );
        assert_eq!(
            closeout.steps[1].argv,
            argv(&[
                "agent-doc",
                "queue",
                "lineage-record",
                DOC,
                "--item",
                "#publishtelemetry",
                "--generation",
                "3",
                "--event",
                "integrated",
                "--source-repo",
                "/repo/worktree",
                "--sink-repo",
                "/repo/main",
                "--source-sha",
                "abc123",
                "--integration-sha",
                "abc123",
                "--sink-ref",
                "refs/heads/main",
                "--method",
                "fast-forward",
            ])
        );
        assert!(closeout.steps.iter().all(|step| step.concrete));
    }

    #[test]
    fn unintegrated_repo_work_names_its_missing_integration_step_first() {
        let claim = reported("do [#fix]", "subagent:fix");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::DoesNotContain),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationRequired);
        assert_eq!(closeout.steps[0].action, ReportedCloseoutAction::Integrate);
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "git",
                "-C",
                "/repo/main",
                "push",
                ".",
                "abc123:refs/heads/main"
            ])
        );
        assert_eq!(
            closeout.steps.last().unwrap().action,
            ReportedCloseoutAction::Consume
        );
    }

    #[test]
    fn remote_tracking_sink_integrates_by_pushing_the_source_commit() {
        let claim = reported("do [#pr]", "subagent:pr");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: ReportedRepoFacts {
                sink_ref: Some("refs/remotes/origin/agent/topic".to_string()),
                ..facts()
            },
            sink: Some(SinkContainment::DoesNotContain),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "git",
                "-C",
                "/repo/worktree",
                "push",
                "origin",
                "abc123:refs/heads/agent/topic"
            ])
        );
    }

    #[test]
    fn cross_repository_source_is_fetched_before_verification() {
        let claim = reported("do [#pin]", "subagent:pin");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::SourceAbsentFromSink),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationRequired);
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "git",
                "-C",
                "/repo/main",
                "fetch",
                "/repo/worktree",
                "abc123"
            ])
        );
    }

    #[test]
    fn missing_repo_facts_are_named_and_the_first_step_inspects_lineage() {
        let claim = reported("do [#half]", "subagent:half");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: ReportedRepoFacts {
                source_sha: None,
                sink_ref: None,
                ..facts()
            },
            sink: None,
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
        assert_eq!(closeout.missing_facts, ["source_sha", "sink_ref"]);
        assert_eq!(
            closeout.steps[0].action,
            ReportedCloseoutAction::InspectLineage
        );
        assert!(closeout.steps[0].concrete);
        let record = &closeout.steps[2];
        assert_eq!(record.action, ReportedCloseoutAction::RecordIntegration);
        assert!(!record.concrete);
        assert!(record.argv.iter().any(|arg| arg == "<source-sha>"));
        assert!(record.argv.iter().any(|arg| arg == "<sink-ref>"));
    }

    #[test]
    fn missing_lineage_attempt_is_named() {
        let claim = reported("free text work", "subagent:x");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::LineageMissing).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
        assert_eq!(closeout.missing_facts, ["queue_lineage_attempt"]);
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&[
                "agent-doc",
                "queue",
                "lineage",
                DOC,
                "--item",
                "free text work",
                "--generation",
                "3",
                "--verbose",
            ])
        );
    }

    #[test]
    fn stale_integration_proof_is_reproven() {
        let claim = reported("do [#stale]", "subagent:stale");
        let evidence = ReportedRepoEvidence::IntegratedUnverified {
            facts: facts(),
            reason: "integration commit is not reachable from the designated sink ref".to_string(),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationProofStale);
        assert_eq!(
            closeout.steps[0].action,
            ReportedCloseoutAction::InspectLineage
        );
        assert_eq!(
            closeout.steps[1].action,
            ReportedCloseoutAction::VerifyIntegration
        );
    }

    #[test]
    fn non_reported_states_have_no_closeout() {
        let mut claim = reported("do [#run]", "subagent:run");
        for state in [
            QueueClaimState::Claimed,
            QueueClaimState::Running,
            QueueClaimState::Lost,
        ] {
            claim.state = state;
            assert!(
                reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::ProseOnly).is_none()
            );
        }
    }

    #[test]
    fn every_shape_starts_with_a_concrete_step() {
        let claim = reported("do [#any]", "subagent:any");
        let mut evidences = vec![
            ReportedRepoEvidence::LineageMissing,
            ReportedRepoEvidence::LineageAmbiguous,
            ReportedRepoEvidence::ProseOnly,
            ReportedRepoEvidence::IntegratedVerified,
            ReportedRepoEvidence::IntegratedUnverified {
                facts: ReportedRepoFacts::default(),
                reason: "x".to_string(),
            },
        ];
        for sink in [
            None,
            Some(SinkContainment::Contains),
            Some(SinkContainment::DoesNotContain),
            Some(SinkContainment::SourceAbsentFromSink),
            Some(SinkContainment::Unverifiable {
                reason: "y".to_string(),
            }),
        ] {
            for facts in [facts(), ReportedRepoFacts::default()] {
                evidences.push(ReportedRepoEvidence::Unintegrated {
                    facts,
                    sink: sink.clone(),
                });
            }
        }
        for evidence in evidences {
            let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
            assert!(closeout.steps[0].concrete, "{evidence:?} -> {closeout:?}");
            assert_eq!(
                closeout.next_action_argv(),
                Some(closeout.steps[0].argv.as_slice()),
                "{evidence:?}"
            );
            assert_eq!(
                closeout.steps.last().unwrap().action,
                ReportedCloseoutAction::Consume
            );
            assert_eq!(
                closeout.missing_facts.is_empty(),
                closeout.steps.iter().all(|step| step.concrete),
                "{evidence:?} -> {closeout:?}"
            );
        }
    }

    #[test]
    fn overdue_after_more_than_one_committed_cycle() {
        assert!(!reported_closeout_overdue(0));
        assert!(!reported_closeout_overdue(1));
        assert!(reported_closeout_overdue(2));
    }

    #[test]
    fn reported_since_prefers_the_report_time() {
        let mut claim = reported("do [#t]", "subagent:t");
        assert_eq!(reported_since(&claim), (100, "worker started"));
        claim.reported_at_secs = Some(500);
        assert_eq!(reported_since(&claim), (500, "reported"));
        claim.reported_at_secs = None;
        claim.worker = None;
        assert_eq!(reported_since(&claim), (90, "claimed"));
    }

    #[test]
    fn stamp_reported_at_is_idempotent_and_reported_only() {
        let mut ledger = crate::queue_claim::QueueClaimLedger::default();
        let mut claim = reported("do [#s]", "subagent:s");
        claim.state = QueueClaimState::Running;
        ledger.claims.push(claim.clone());
        assert!(ledger.stamp_reported_at(&claim.identity, 3, 10).is_none());
        ledger.claims[0].state = QueueClaimState::Reported;
        assert!(
            ledger.stamp_reported_at(&claim.identity, 2, 10).is_none(),
            "generation fence"
        );
        assert_eq!(
            ledger
                .stamp_reported_at(&claim.identity, 3, 10)
                .unwrap()
                .reported_at_secs,
            Some(10)
        );
        assert_eq!(
            ledger
                .stamp_reported_at(&claim.identity, 3, 99)
                .unwrap()
                .reported_at_secs,
            Some(10)
        );
    }

    #[test]
    fn overdue_warning_names_the_next_command() {
        let claim = reported("free text work", "subagent:x");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::ProseOnly).unwrap();
        let warning = overdue_reported_closeout_warning(&claim, &closeout, 3);
        assert!(warning.contains("after 3 committed cycle(s)"), "{warning}");
        assert!(
            warning.contains(
                "Next: `agent-doc queue consume tasks/session.md --ack-text 'free text work' --ack-owner 'subagent:x#3'`"
            ),
            "{warning}"
        );
    }

    #[test]
    fn obligation_lists_one_ordered_command_per_step() {
        let claim = reported("do [#fix]", "subagent:fix");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::Contains),
        };
        let obligation = ReportedCloseoutObligation::new(
            &claim,
            reported_claim_closeout(DOC, &claim, &evidence).unwrap(),
        );
        let commands = obligation.required_commands();
        assert_eq!(commands.len(), 3);
        assert!(commands[0].contains("step 1/3 (verify_integration): run `git -C /repo/main merge-base --is-ancestor abc123 refs/heads/main`"), "{}", commands[0]);
        assert!(
            commands[1].contains("step 2/3 (record_integration)"),
            "{}",
            commands[1]
        );
        assert!(
            commands[2].contains(
                "step 3/3 (consume): run `agent-doc queue consume tasks/session.md --ack-id fix`"
            ),
            "{}",
            commands[2]
        );
        assert!(
            commands
                .iter()
                .all(|command| command.contains("before any new queue claim or spawn"))
        );
    }

    #[test]
    fn shell_command_quotes_only_unsafe_words() {
        assert_eq!(
            shell_command(&argv(&["a", "b c", "it's", "#id"])),
            "a 'b c' 'it'\\''s' '#id'"
        );
    }
}
