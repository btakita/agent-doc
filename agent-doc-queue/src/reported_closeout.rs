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
//!   (`#subagentintegrationproof`): an attempt is prose-only only when its
//!   lineage carries an explicit `prose_only` declaration and no repository
//!   facts; one with repository facts needs a verified `integrated` receipt
//!   first; one with neither is unknown (`#reportedcloseoutfacts`);
//! * for unintegrated repository work, how the designated sink ref relates to
//!   the worker's source commit ([`SinkContainment`]).
//!
//! Every closeout either has a concrete common next step
//! ([`ReportedClaimCloseout::next_action_argv`]) or a
//! [`ReportedCloseoutDecision`] whose choices name alternative step branches.
//! A fact the closeout needs that was never recorded is named in
//! [`ReportedClaimCloseout::missing_facts`] and comes with a step that records
//! it; steps that need an unknown value carry `<placeholder>` arguments and
//! `concrete: false`. Inspecting lineage alone is never the whole plan: it
//! re-confirms the gap without closing it (the 2026-11-04 sdk-dev stall).
//!
//! Every closeout also carries a deterministic operator-request packet
//! ([`ReportedClaimCloseout::operator_request_packet`]). When the remaining
//! step needs a fact or choice the coordinator cannot settle from durable
//! evidence, presenting that packet is the closeout's next action: it becomes
//! the ordinary `agent:todo` + `🙋` operator wait (`#packettodo`), and until
//! then the turn's closeout report keeps continuation required
//! ([`pending_reported_closeout`]) instead of ending in silence.
//!
//! This module is pure: no clock, no storage, no git. The caller supplies the
//! evidence and the commit counts.

use std::collections::HashSet;

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

/// How the designated sink ref relates to the worker's source commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SinkContainment {
    /// `git merge-base --is-ancestor <source> <sink_ref>` succeeds in the sink.
    Contains,
    /// The sink has the source commit, but the sink ref does not reach it.
    DoesNotContain {
        /// The sink ref is an ancestor of the source, so a fast-forward update
        /// lands it. False means the sink moved past the worker's base and a
        /// fast-forward push is refused as non-fast-forward.
        #[serde(default)]
        fast_forward: bool,
        /// A worktree that has the sink branch checked out. `git push` refuses
        /// to update a checked-out branch, so integration runs there instead.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checked_out_worktree: Option<String>,
    },
    /// The source commit and the sink ref share no commit: the recorded sink
    /// is not where this work can land by ancestry (wrong sink repository or
    /// ref, or a cross-repository pin that needs its own sink commit).
    NoSharedHistory,
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
    /// The attempt carries an explicit `prose_only` declaration and records
    /// no repository, worktree, or commit facts.
    ProseOnly,
    /// The attempt records neither repository facts nor a prose-only
    /// declaration. A started worker with no recorded facts is
    /// indistinguishable from repository work nobody recorded, so this is
    /// never prose (`#reportedcloseoutfacts`).
    Undeclared,
    /// The latest `integrated` receipt verifies against the sink now.
    IntegratedVerified,
    /// An `integrated` receipt exists but no longer verifies.
    IntegratedUnverified {
        facts: ReportedRepoFacts,
        reason: String,
        /// The refusal is a `cherry-pick`/`rebase` patch-id mismatch: the
        /// landed commit changed the patch (a resolved conflict), so the
        /// re-proof is a `conflict-resolved` receipt, not the same method.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        patch_id_mismatch: bool,
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
    /// Declared prose-only result: strike the row.
    ProseConsume,
    /// Repository work with verified integration proof: strike the row.
    IntegratedConsume,
    /// The sink contains (or may contain) the work: verify, record the
    /// `integrated` receipt, then strike.
    IntegrationProof,
    /// The sink does not contain the work: integrate it first.
    IntegrationRequired,
    /// The source shares no history with the recorded sink ref: correct the
    /// sink facts or port the change; an operator choice.
    SinkMismatch,
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
            Self::SinkMismatch => "sink_mismatch",
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
    /// Record the attempt's repository facts (`lineage-record --event committed`).
    RecordRepositoryWork,
    /// Record the attempt's explicit prose-only declaration
    /// (`lineage-record --event prose-only`).
    DeclareProseOnly,
    FetchSource,
    /// Fast-forward the sink ref to the source commit.
    Integrate,
    /// Cherry-pick the source commit onto the checked-out sink branch.
    CherryPickOntoSink,
    /// Merge the source commit into the checked-out sink branch.
    MergeIntoSink,
    VerifyIntegration,
    RecordIntegration,
    Consume,
}

impl ReportedCloseoutAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InspectLineage => "inspect_lineage",
            Self::RecordRepositoryWork => "record_repository_work",
            Self::DeclareProseOnly => "declare_prose_only",
            Self::FetchSource => "fetch_source",
            Self::Integrate => "integrate",
            Self::CherryPickOntoSink => "cherry_pick_onto_sink",
            Self::MergeIntoSink => "merge_into_sink",
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
    /// The [`ReportedCloseoutDecision`] choice this step belongs to. Steps of
    /// different alternatives are mutually exclusive; steps with no
    /// alternative run on every branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternative: Option<String>,
    pub detail: String,
}

/// One branch of a [`ReportedCloseoutDecision`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedCloseoutChoice {
    /// Matches [`ReportedCloseoutStep::alternative`] of this branch's steps.
    pub label: String,
    pub detail: String,
}

/// A choice the closeout cannot make from durable facts alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedCloseoutDecision {
    pub question: String,
    /// Empty when the answer is a value (a sink ref, a lineage attempt) rather
    /// than one of a fixed set of branches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ReportedCloseoutChoice>,
    /// True when only the operator can answer: present
    /// [`ReportedClaimCloseout::operator_request_packet`] now. False when the
    /// coordinator can settle it from durable evidence (the worker's reported
    /// result, the repositories); it presents the packet only when that
    /// evidence does not settle it.
    pub requires_operator: bool,
}

/// The ordered commands that close one reported attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedClaimCloseout {
    pub shape: ReportedCloseoutShape,
    pub detail: String,
    /// Durable facts the closeout needs that were never recorded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_facts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<ReportedCloseoutDecision>,
    pub steps: Vec<ReportedCloseoutStep>,
    /// Stable `subagent-operator-request-reportedcloseout-<hash>` id of this
    /// attempt's operator request; an open `agent:todo` item with this id
    /// means the closeout is waiting on the operator.
    #[serde(default)]
    pub operator_request_id: String,
    /// The exact blocking operator-request packet to present in the response
    /// when the closeout needs the operator (`#packettodo`).
    #[serde(default)]
    pub operator_request_packet: String,
}

impl ReportedClaimCloseout {
    /// The first step, when it is concrete and common to every branch. `None`
    /// when a [`ReportedCloseoutDecision`] must be settled (or a missing value
    /// supplied) first: a later concrete step, such as the closing `consume`,
    /// is never offered ahead of the steps it depends on.
    pub fn next_action_argv(&self) -> Option<&[String]> {
        self.steps
            .first()
            .filter(|step| step.concrete && step.alternative.is_none())
            .map(|step| step.argv.as_slice())
    }

    /// Only the operator can make the next move.
    pub fn requires_operator(&self) -> bool {
        self.decision
            .as_ref()
            .is_some_and(|decision| decision.requires_operator)
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

const REPOSITORY_WORK: &str = "repository_work";
const PROSE_ONLY: &str = "prose_only";
const CHERRY_PICK: &str = "cherry_pick";
const MERGE: &str = "merge";
const CORRECT_SINK: &str = "correct_sink";
/// The `--method` that records a hand-resolved-conflict integration.
const CONFLICT_RESOLVED_METHOD: &str = "conflict-resolved";

/// The lineage event that declares an attempt prose-only
/// (`agent-doc queue lineage-record --event prose-only`).
pub const PROSE_ONLY_LINEAGE_EVENT: &str = "prose-only";

/// Stable operator-request id for one reported attempt: the reserved
/// `subagent-operator-request-` family plus a digest of identity, owner, and
/// generation, so a re-derived closeout recognizes its own open request.
pub fn operator_request_id(claim: &QueueClaim) -> String {
    let identity = match &claim.identity {
        QueueItemIdentity::Id(id) => format!("id:{id}"),
        QueueItemIdentity::FreeText(text) => format!("text:{text}"),
    };
    let digest = agent_doc_hash::short_content_hash(&format!(
        "{identity}\n{}\n{}",
        claim.owner, claim.generation
    ));
    format!("subagent-operator-request-reportedcloseout-{digest}")
}

/// The one exact command that strikes a reported queue row
/// (`#closeoutguidanceagree`).
///
/// Completion feedback (`queue report-complete`), session-check's overdue
/// closeout, the Stop hook's writeback reminder, mid-turn steering and the
/// `queue claims` consume step all render this, so a coordinator is never told
/// two different commands for one attempt. They disagreed on 2026-11-05: the
/// completion feedback and the Stop hook read the first `#` of a free-text row,
/// so `[#id]: answer` got `--ack-id <id>` (refused when another attempt also
/// reported for `<id>`) and a `[#q-…]` row got `--ack-id q-…` (not a backlog
/// id, always refused), while session-check named the attempt-fenced
/// `--ack-text <row> --ack-owner owner#gen` for the same attempt.
///
/// `#ackidattemptfence`: an id strike names that same `--ack-owner` fence when
/// the attempt is known, so two visible rows that both strike by id are two
/// different commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportedRowStrike {
    /// `queue consume <doc> --ack-id <id> [--ack-owner <owner#gen>]`: an
    /// id-backed row (`do [#id]`, `[#id]`), or a waiting-tree parent whose
    /// reported `☑️` attempt is nested under it, which only `--ack-id <parent
    /// id>` strikes (`#nestedreceiptack`, `#reportedwritebackprescription`).
    /// `owner` is `Some(owner#gen)` when the attempt is known
    /// (`#ackidattemptfence`); a correction head or a legacy generation-0
    /// receipt stays unfenced.
    AckId { id: String, owner: Option<String> },
    /// `queue consume <doc> --ack-text <row> --ack-owner <owner#gen>`: a
    /// free-text row (prose, `[#id]: answer`, a `[#q-…]` row) bound to one
    /// exact worker attempt.
    AckText { handle: String, owner: String },
    /// A free-text row with no exact attempt to fence the strike to: the
    /// verbatim `> **Queue prompt:**` echo persisted by `agent-doc finalize`.
    Echo,
}

impl ReportedRowStrike {
    /// The `queue consume` argv, or `None` for [`ReportedRowStrike::Echo`].
    pub fn argv(&self, document: &str) -> Option<Vec<String>> {
        match self {
            Self::AckId {
                id,
                owner: Some(owner),
            } => Some(argv(&[
                "agent-doc",
                "queue",
                "consume",
                document,
                "--ack-id",
                id,
                "--ack-owner",
                owner,
            ])),
            Self::AckId { id, owner: None } => Some(argv(&[
                "agent-doc",
                "queue",
                "consume",
                document,
                "--ack-id",
                id,
            ])),
            Self::AckText { handle, owner } => Some(argv(&[
                "agent-doc",
                "queue",
                "consume",
                document,
                "--ack-text",
                handle,
                "--ack-owner",
                owner,
            ])),
            Self::Echo => None,
        }
    }
}

/// How reported queue row `item_text` strikes, given the exact attempt
/// (`owner`, `generation`) when the caller knows it (`#closeoutguidanceagree`).
///
/// The row's claim identity decides: an id-backed row acknowledges by id. A
/// free-text row's `#` mentions never name a tracked item, with one exception:
/// a waiting-tree parent (`[#id] ... <!-- agent-doc:waiting-item -->`) whose
/// reported attempt is a nested child row, which `--ack-text <parent>` cannot
/// address. Every other free-text row is fenced to its attempt; a legacy
/// generation-0 receipt or an unknown attempt falls back to the echo. An id
/// strike carries the same fence when the attempt is known
/// (`#ackidattemptfence`).
pub fn reported_row_strike(item_text: &str, attempt: Option<(&str, u64)>) -> ReportedRowStrike {
    let item = item_text.trim();
    let owner = known_attempt_owner(attempt);
    if let QueueItemIdentity::Id(id) = crate::queue_claim::claim_identity(item) {
        return ReportedRowStrike::AckId { id, owner };
    }
    // A binary-generated trailing `[#q-…]` is an occurrence id, never a
    // tracked item, so it is stripped before the parent's id is read.
    if item.contains(crate::queue_consume::WAITING_ON_YOU_ITEM_MARKER)
        && let Some(id) = crate::queue_response::queue_prompt_done_id(
            &agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item),
        )
    {
        return ReportedRowStrike::AckId { id, owner };
    }
    match owner {
        Some(owner) => ReportedRowStrike::AckText {
            handle: claim_item_handle(item),
            owner,
        },
        None => ReportedRowStrike::Echo,
    }
}

/// `OWNER#GENERATION` for a positive exact attempt, else `None`.
fn known_attempt_owner(attempt: Option<(&str, u64)>) -> Option<String> {
    match attempt {
        Some((owner, generation)) if !owner.trim().is_empty() && generation > 0 => {
            Some(format!("{}#{}", owner.trim(), generation))
        }
        _ => None,
    }
}

/// [`reported_row_strike`] for a claim's exact attempt.
pub fn claim_row_strike(claim: &QueueClaim) -> ReportedRowStrike {
    reported_row_strike(&claim.item_text, Some((&claim.owner, claim.generation)))
}

fn consume_step(document: &str, claim: &QueueClaim) -> ReportedCloseoutStep {
    let strike = claim_row_strike(claim);
    let detail = match &strike {
        ReportedRowStrike::AckId { id, .. } => format!(
            "strike the reported row only after the open item records a `(stays open: <reason>)` clause; `--ack-id` refuses and leaves the row unstruck without one (#acknoreason), and a successful strike names that reason (#ackstrikeopenbacklog). When the worker's or operator's answer resolves the tracked item, pass `--done {id}` to `agent-doc finalize {document}` instead and close any parent item the same answer resolves; never acknowledge a head whose item the answer finished"
        ),
        _ => "strike the reported free-text row, fenced to this exact worker attempt".to_string(),
    };
    // A Reported claim always carries a positive generation; a generation-0
    // legacy claim still names the unfenced `--ack-text` selector rather than
    // an echo, which is not a `queue consume` step.
    let argv = strike.argv(document).unwrap_or_else(|| {
        argv(&[
            "agent-doc",
            "queue",
            "consume",
            document,
            "--ack-text",
            &claim_item_handle(&claim.item_text),
        ])
    });
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::Consume,
        argv,
        concrete: true,
        alternative: None,
        detail,
    }
}

fn lineage_record_argv(document: &str, claim: &QueueClaim, event: &str) -> Vec<String> {
    let generation = claim.generation.to_string();
    argv(&[
        "agent-doc",
        "queue",
        "lineage-record",
        document,
        "--item",
        &claim_item_handle(&claim.item_text),
        "--generation",
        &generation,
        "--event",
        event,
    ])
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
        alternative: None,
        detail,
    }
}

/// Which sink repository a repository-facts step proposes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SinkRepo {
    /// The recorded sink, else the source repository (a worker integrating
    /// in its own repository records only one).
    Recorded,
    /// The recorded sink is wrong: always a placeholder.
    Unknown,
}

/// Record the attempt's repository facts. Recorded values are kept; every
/// unknown value is a marked `<placeholder>` and the step is not concrete.
fn record_repository_work_step(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    sink: SinkRepo,
    alternative: Option<&str>,
    detail: String,
) -> ReportedCloseoutStep {
    let mut argv = lineage_record_argv(document, claim, "committed");
    let source_repo = facts.source_repo.as_deref().unwrap_or("<source-repo>");
    let source_sha = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_repo = match sink {
        SinkRepo::Recorded => facts.sink_repo(),
        SinkRepo::Unknown => None,
    }
    .unwrap_or("<sink-repo>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    argv.extend(
        [
            ("--source-repo", source_repo),
            ("--source-sha", source_sha),
            ("--sink-repo", sink_repo),
            ("--sink-ref", sink_ref),
        ]
        .into_iter()
        .flat_map(|(flag, value)| [flag.to_string(), value.to_string()]),
    );
    if facts.source_repo.is_none() {
        // Unknown repository work: the worktree and branch are worth
        // recording too (`queue lineage` checks them against Git).
        argv.extend(
            ["--worktree", "<worktree>", "--branch", "<branch>"]
                .into_iter()
                .map(str::to_string),
        );
    }
    let concrete = !argv.iter().any(|arg| is_placeholder(arg));
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::RecordRepositoryWork,
        argv,
        concrete,
        alternative: alternative.map(str::to_string),
        detail,
    }
}

fn declare_prose_only_step(document: &str, claim: &QueueClaim) -> ReportedCloseoutStep {
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::DeclareProseOnly,
        argv: lineage_record_argv(document, claim, PROSE_ONLY_LINEAGE_EVENT),
        concrete: true,
        alternative: Some(PROSE_ONLY.to_string()),
        detail: "only when the worker changed no repository, worktree, or branch: record the durable prose-only declaration; `lineage-record` refuses it once repository facts exist. Then the consume step runs as written".to_string(),
    }
}

fn is_placeholder(arg: &str) -> bool {
    arg.starts_with('<') && arg.ends_with('>')
}

fn verify_step(facts: &ReportedRepoFacts, alternative: Option<&str>) -> ReportedCloseoutStep {
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
        alternative: alternative.map(str::to_string),
        detail: format!(
            "exit status 0 proves designated sink ref {sink_ref} contains source commit {source}; any other status means it is not integrated yet"
        ),
    }
}

/// The `integrated` receipt. `integration` is `None` for a method whose sink
/// commit exists only after the integration step runs (cherry-pick, merge).
fn record_step(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    method: &str,
    integration: Option<&str>,
    alternative: Option<&str>,
    detail: String,
) -> ReportedCloseoutStep {
    let source_repo = facts.source_repo.as_deref().unwrap_or("<source-repo>");
    let sink_repo = facts.sink_repo().unwrap_or("<sink-repo>");
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    let integration = integration.unwrap_or("<sink-commit-sha>");
    let mut argv = lineage_record_argv(document, claim, "integrated");
    argv.extend(
        [
            ("--source-repo", source_repo),
            ("--sink-repo", sink_repo),
            ("--source-sha", source),
            ("--integration-sha", integration),
            ("--sink-ref", sink_ref),
            ("--method", method),
        ]
        .into_iter()
        .flat_map(|(flag, value)| [flag.to_string(), value.to_string()]),
    );
    let concrete = !argv.iter().any(|arg| is_placeholder(arg));
    ReportedCloseoutStep {
        action: ReportedCloseoutAction::RecordIntegration,
        argv,
        concrete,
        alternative: alternative.map(str::to_string),
        detail,
    }
}

fn fast_forward_record_step(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
) -> ReportedCloseoutStep {
    record_step(
        document,
        claim,
        facts,
        "fast-forward",
        Some(facts.source_sha.as_deref().unwrap_or("<source-sha>")),
        None,
        "record the verified edge: the source commit itself is reachable from the designated sink ref. If the work landed as a different sink commit (rebase, cherry-pick, merge), pass that commit as --integration-sha with the matching --method instead".to_string(),
    )
}

/// The worktree a sink-side integration runs in: the one that has the sink
/// branch checked out, else a marked placeholder.
fn sink_worktree(checked_out: Option<&str>) -> &str {
    checked_out.unwrap_or("<worktree-with-sink-ref-checked-out>")
}

/// The `integrated` receipt for a commit that integrated the source through a
/// hand-resolved conflict (`--method conflict-resolved`). `--resolves` names
/// the source explicitly, so the proof does not depend on the commit message.
fn conflict_resolved_record_step(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    integration: Option<&str>,
    alternative: Option<&str>,
    detail: String,
) -> ReportedCloseoutStep {
    let mut step = record_step(
        document,
        claim,
        facts,
        CONFLICT_RESOLVED_METHOD,
        integration,
        alternative,
        detail,
    );
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    step.argv.extend(["--resolves".to_string(), source.to_string()]);
    step.concrete = !step.argv.iter().any(|arg| is_placeholder(arg));
    step
}

/// `cherry_pick` branch: apply the source commit on the checked-out sink
/// branch, then record the new sink commit. The record step verifies the
/// patch-id equivalence itself, so it is the proof.
fn cherry_pick_steps(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    checked_out: Option<&str>,
) -> Vec<ReportedCloseoutStep> {
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    let sink_repo = facts.sink_repo().unwrap_or("<sink-repo>");
    let worktree = sink_worktree(checked_out);
    vec![
        ReportedCloseoutStep {
            action: ReportedCloseoutAction::CherryPickOntoSink,
            argv: argv(&["git", "-C", worktree, "cherry-pick", source]),
            concrete: checked_out.is_some() && facts.source_sha.is_some(),
            alternative: Some(CHERRY_PICK.to_string()),
            detail: format!(
                "apply source commit {source} onto {sink_ref} in a clean worktree that has it checked out (`git -C {sink_repo} worktree add <path> {sink_ref}` makes one). A conflict is resolved there, never by force"
            ),
        },
        record_step(
            document,
            claim,
            facts,
            "cherry-pick",
            None,
            Some(CHERRY_PICK),
            format!(
                "record the cherry-picked commit: `git -C {sink_repo} rev-parse {sink_ref}` after the cherry-pick is --integration-sha. The command verifies patch-id equivalence with {source} and that the commit stays reachable from {sink_ref}. When the cherry-pick conflicted and you resolved it, the patch ids differ and this refuses (`different patch ids`); record the same commit with `--method {CONFLICT_RESOLVED_METHOD} --resolves {source}` instead, which proves sink reachability, the named source, and that the commit touches every file {source} touched"
            ),
        ),
    ]
}

/// `merge` branch: merge the source into the checked-out sink branch, verify
/// ancestry, then record the merge commit.
fn merge_steps(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    checked_out: Option<&str>,
) -> Vec<ReportedCloseoutStep> {
    let source = facts.source_sha.as_deref().unwrap_or("<source-sha>");
    let sink_ref = facts.sink_ref.as_deref().unwrap_or("<sink-ref>");
    let sink_repo = facts.sink_repo().unwrap_or("<sink-repo>");
    let worktree = sink_worktree(checked_out);
    vec![
        ReportedCloseoutStep {
            action: ReportedCloseoutAction::MergeIntoSink,
            argv: argv(&["git", "-C", worktree, "merge", "--no-ff", "--no-edit", source]),
            concrete: checked_out.is_some() && facts.source_sha.is_some(),
            alternative: Some(MERGE.to_string()),
            detail: format!(
                "merge source commit {source} into {sink_ref} in a clean worktree that has it checked out (`git -C {sink_repo} worktree add <path> {sink_ref}` makes one)"
            ),
        },
        verify_step(facts, Some(MERGE)),
        record_step(
            document,
            claim,
            facts,
            "merge",
            None,
            Some(MERGE),
            format!(
                "record the merge commit: `git -C {sink_repo} rev-parse {sink_ref}` after the merge is --integration-sha"
            ),
        ),
    ]
}

/// The fast-forward command that publishes `source` onto `sink_ref`. When the
/// sink branch is checked out in a worktree, `git push` refuses to update it,
/// so the fast-forward runs as `merge --ff-only` in that worktree instead.
fn integrate_step(
    facts: &ReportedRepoFacts,
    checked_out: Option<&str>,
) -> Option<ReportedCloseoutStep> {
    let source = facts.source_sha.as_deref()?;
    let sink_ref = facts.sink_ref.as_deref()?;
    let sink_repo = facts.sink_repo()?;
    if let Some(worktree) = checked_out {
        return Some(ReportedCloseoutStep {
            action: ReportedCloseoutAction::Integrate,
            argv: argv(&["git", "-C", worktree, "merge", "--ff-only", source]),
            concrete: true,
            alternative: None,
            detail: format!(
                "{sink_ref} is checked out in {worktree}, so fast-forward it there to the worker commit {source}; `--ff-only` refuses rather than creating a merge"
            ),
        });
    }
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
        alternative: None,
        detail: format!(
            "fast-forward {sink_ref} to the worker commit {source}. A refusal changes nothing; rerun `agent-doc queue claims` and follow the branch it then derives"
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
        alternative: None,
        detail: format!(
            "the sink repository {sink_repo} has no object {source}; fetch it from the source repository, then rerun `agent-doc queue claims` to recompute the next step"
        ),
    })
}

fn choice(label: &str, detail: impl Into<String>) -> ReportedCloseoutChoice {
    ReportedCloseoutChoice {
        label: label.to_string(),
        detail: detail.into(),
    }
}

/// Unknown result class: record repository work, or declare prose-only.
fn result_class_closeout(
    document: &str,
    claim: &QueueClaim,
    fact: &str,
    detail: String,
) -> ReportedClaimCloseout {
    let item = claim_item_handle(&claim.item_text);
    ReportedClaimCloseout {
        shape: ReportedCloseoutShape::MissingFacts,
        missing_facts: vec![fact.to_string()],
        decision: Some(ReportedCloseoutDecision {
            question: format!(
                "Did {}'s attempt (generation {}) for {item:?} change a repository? Lineage records neither repository facts nor a prose-only declaration.",
                claim.owner, claim.generation
            ),
            choices: vec![
                choice(
                    REPOSITORY_WORK,
                    "it committed, pushed, or left changes in a repository or worktree: record that work, then integrate it",
                ),
                choice(
                    PROSE_ONLY,
                    "it changed no repository, worktree, or branch: declare the result prose-only, then strike the row",
                ),
            ],
            requires_operator: false,
        }),
        steps: vec![
            record_repository_work_step(
                document,
                claim,
                &ReportedRepoFacts::default(),
                SinkRepo::Recorded,
                Some(REPOSITORY_WORK),
                "record the worker's repository facts from its reported result (repository, commit, worktree, branch) and the designated sink it must land on; replace every <placeholder>, and leave the sink placeholders to the operator when the destination was never decided. Then rerun `agent-doc queue claims` for the integration steps".to_string(),
            ),
            declare_prose_only_step(document, claim),
            consume_step(document, claim),
        ],
        detail,
        operator_request_id: String::new(),
        operator_request_packet: String::new(),
    }
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
            detail: "the attempt declared a prose-only result and records no repository work; integrate the reported result into this turn's response and strike the row".to_string(),
            missing_facts: Vec::new(),
            decision: None,
            steps: vec![consume],
            operator_request_id: String::new(),
            operator_request_packet: String::new(),
        },
        ReportedRepoEvidence::IntegratedVerified => ReportedClaimCloseout {
            shape: ReportedCloseoutShape::IntegratedConsume,
            detail: "lineage already records a verified integration; only the queue closeout remains".to_string(),
            missing_facts: Vec::new(),
            decision: None,
            steps: vec![consume],
            operator_request_id: String::new(),
            operator_request_packet: String::new(),
        },
        ReportedRepoEvidence::Undeclared => result_class_closeout(
            document,
            claim,
            "repo_work_or_prose_declaration",
            "the attempt records neither repository facts nor a prose-only declaration; a started worker with no recorded facts is indistinguishable from unrecorded repository work, so `queue consume` refuses the row until one is recorded".to_string(),
        ),
        ReportedRepoEvidence::LineageMissing => result_class_closeout(
            document,
            claim,
            "queue_lineage_attempt",
            "no lineage attempt records this generation, so prose-only versus repository work is unknown and `queue consume` refuses the row; `lineage-record` establishes the attempt from the live claim".to_string(),
        ),
        ReportedRepoEvidence::LineageAmbiguous => {
            let detail = "more than one lineage attempt matches this generation; `queue consume` refuses to pick one".to_string();
            ReportedClaimCloseout {
                shape: ReportedCloseoutShape::MissingFacts,
                missing_facts: vec!["unambiguous_queue_lineage_attempt".to_string()],
                decision: Some(ReportedCloseoutDecision {
                    question: format!(
                        "More than one lineage attempt matches {}'s generation {} for {:?}. Which attempt is this report, and which should be cancelled?",
                        claim.owner,
                        claim.generation,
                        claim_item_handle(&claim.item_text)
                    ),
                    choices: Vec::new(),
                    requires_operator: true,
                }),
                steps: vec![inspect_step(document, claim, detail.clone()), consume],
                detail,
                operator_request_id: String::new(),
                operator_request_packet: String::new(),
            }
        }
        ReportedRepoEvidence::IntegratedUnverified {
            facts,
            reason,
            patch_id_mismatch,
        } => {
            let missing = facts.missing();
            let detail = if *patch_id_mismatch {
                format!(
                    "the recorded integration proof no longer verifies ({reason}); the landed commit is not patch-equivalent to the source, so keep the row live and re-prove it as a conflict-resolved integration"
                )
            } else {
                format!(
                    "the recorded integration proof no longer verifies ({reason}); keep the row live and re-prove it against the designated sink"
                )
            };
            let mut steps = vec![inspect_step(document, claim, detail.clone())];
            if !missing.is_empty() {
                steps.push(record_repository_work_step(
                    document,
                    claim,
                    facts,
                    SinkRepo::Recorded,
                    None,
                    format!("record the missing {}", missing.join(", ")),
                ));
            }
            if *patch_id_mismatch {
                steps.push(conflict_resolved_record_step(
                    document,
                    claim,
                    facts,
                    None,
                    None,
                    format!(
                        "record the landed sink commit as --integration-sha with `--method {CONFLICT_RESOLVED_METHOD}`: the command proves it is reachable from the designated sink ref, names the source (`--resolves`), and touches every file the source touched"
                    ),
                ));
            } else {
                steps.push(verify_step(facts, None));
                steps.push(fast_forward_record_step(document, claim, facts));
            }
            steps.push(consume);
            ReportedClaimCloseout {
                shape: if missing.is_empty() {
                    ReportedCloseoutShape::IntegrationProofStale
                } else {
                    ReportedCloseoutShape::MissingFacts
                },
                decision: (!missing.is_empty()).then(|| missing_repo_facts_decision(claim, facts, &missing)),
                detail,
                missing_facts: missing,
                steps,
                operator_request_id: String::new(),
                operator_request_packet: String::new(),
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
                    decision: Some(missing_repo_facts_decision(claim, facts, &missing)),
                    steps: vec![
                        record_repository_work_step(
                            document,
                            claim,
                            facts,
                            SinkRepo::Recorded,
                            None,
                            format!(
                                "record the missing {}; keep every recorded value, replace each <placeholder>. Then rerun `agent-doc queue claims` for the integration steps",
                                missing.join(", ")
                            ),
                        ),
                        verify_step(facts, None),
                        fast_forward_record_step(document, claim, facts),
                        consume,
                    ],
                    detail,
                    missing_facts: missing,
                    operator_request_id: String::new(),
                    operator_request_packet: String::new(),
                }
            } else {
                unintegrated_closeout(document, claim, facts, sink.as_ref(), consume)
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
    closeout.operator_request_id = operator_request_id(claim);
    closeout.operator_request_packet = operator_request_packet(claim, &closeout);
    debug_assert!(
        closeout.next_action_argv().is_some() || closeout.decision.is_some(),
        "{closeout:?}"
    );
    Some(closeout)
}

fn missing_repo_facts_decision(
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    missing: &[String],
) -> ReportedCloseoutDecision {
    let source = facts.source_sha.as_deref().unwrap_or("its commit");
    ReportedCloseoutDecision {
        question: format!(
            "{}'s attempt (generation {}) for {:?} recorded repository work without {}. {}",
            claim.owner,
            claim.generation,
            claim_item_handle(&claim.item_text),
            missing.join(", "),
            if missing.iter().any(|fact| fact == "sink_ref") {
                format!("Which branch or ref should {source} land on?")
            } else {
                "What are the missing values?".to_string()
            }
        ),
        choices: Vec::new(),
        // The destination of unrequested repository work is an operator
        // decision; a missing source fact is usually in the worker's report.
        requires_operator: missing.iter().any(|fact| fact == "sink_ref"),
    }
}

fn unintegrated_closeout(
    document: &str,
    claim: &QueueClaim,
    facts: &ReportedRepoFacts,
    sink: Option<&SinkContainment>,
    consume: ReportedCloseoutStep,
) -> ReportedClaimCloseout {
    let source = facts.source_sha.as_deref().unwrap_or_default();
    let sink_ref = facts.sink_ref.as_deref().unwrap_or_default();
    let sink_repo = facts.sink_repo().unwrap_or_default();
    let closeout = |shape, detail: String, decision, steps| ReportedClaimCloseout {
        shape,
        detail,
        missing_facts: Vec::new(),
        decision,
        steps,
        operator_request_id: String::new(),
        operator_request_packet: String::new(),
    };
    match sink {
        Some(SinkContainment::DoesNotContain {
            fast_forward: true,
            checked_out_worktree,
        }) => {
            let mut steps = Vec::new();
            steps.extend(integrate_step(facts, checked_out_worktree.as_deref()));
            steps.extend([
                verify_step(facts, None),
                fast_forward_record_step(document, claim, facts),
                consume,
            ]);
            closeout(
                ReportedCloseoutShape::IntegrationRequired,
                format!(
                    "designated sink ref {sink_ref} does not contain source commit {source}, and it fast-forwards to it; integrate it before recording proof"
                ),
                None,
                steps,
            )
        }
        Some(SinkContainment::DoesNotContain {
            fast_forward: false,
            checked_out_worktree,
        }) => {
            let worktree = checked_out_worktree.as_deref();
            let mut steps = cherry_pick_steps(document, claim, facts, worktree);
            steps.extend(merge_steps(document, claim, facts, worktree));
            steps.push(consume);
            closeout(
                ReportedCloseoutShape::IntegrationRequired,
                format!(
                    "designated sink ref {sink_ref} has moved past the worker's base, so source commit {source} cannot fast-forward onto it (a `git push` would be refused as non-fast-forward); cherry-pick or merge it onto {sink_ref}, then record that sink commit with the matching --method"
                ),
                Some(ReportedCloseoutDecision {
                    question: format!(
                        "{sink_ref} in {sink_repo} cannot fast-forward to {source}. Integrate it by cherry-pick or by merge?"
                    ),
                    choices: vec![
                        choice(
                            CHERRY_PICK,
                            format!("apply {source} as one new commit on {sink_ref}; the record step verifies patch-id equivalence"),
                        ),
                        choice(
                            MERGE,
                            format!("merge {source} into {sink_ref}, keeping the worker commit itself reachable"),
                        ),
                    ],
                    requires_operator: false,
                }),
                steps,
            )
        }
        Some(SinkContainment::NoSharedHistory) => {
            let mut steps = vec![record_repository_work_step(
                document,
                claim,
                &ReportedRepoFacts {
                    sink_repo: None,
                    sink_ref: None,
                    ..facts.clone()
                },
                SinkRepo::Unknown,
                Some(CORRECT_SINK),
                "re-record the attempt with the sink it actually integrates into (replace <sink-repo> and <sink-ref>); then rerun `agent-doc queue claims`".to_string(),
            )];
            steps.extend(cherry_pick_steps(document, claim, facts, None));
            steps.push(consume);
            closeout(
                ReportedCloseoutShape::SinkMismatch,
                format!(
                    "source commit {source} shares no history with designated sink ref {sink_ref} in {sink_repo}, so no fast-forward, merge-base check, or push can land it there; the recorded sink is wrong for this commit, or the change must be ported onto {sink_ref}"
                ),
                Some(ReportedCloseoutDecision {
                    question: format!(
                        "Source commit {source} ({}) shares no history with {sink_ref} in {sink_repo}. Which sink should {}'s attempt integrate into, or should the change be ported onto {sink_ref}?",
                        facts.source_repo.as_deref().unwrap_or_default(),
                        claim.owner,
                    ),
                    choices: vec![
                        choice(
                            CORRECT_SINK,
                            "the recorded sink is wrong: name the repository and ref this commit lands on",
                        ),
                        choice(
                            CHERRY_PICK,
                            format!("port the change: cherry-pick {source} onto {sink_ref} in a worktree of {sink_repo}"),
                        ),
                    ],
                    requires_operator: true,
                }),
                steps,
            )
        }
        Some(SinkContainment::SourceAbsentFromSink) => {
            let mut steps = Vec::new();
            steps.extend(fetch_step(facts));
            steps.extend([
                verify_step(facts, None),
                fast_forward_record_step(document, claim, facts),
                consume,
            ]);
            closeout(
                ReportedCloseoutShape::IntegrationRequired,
                format!("source commit {source} is not present in sink repository {sink_repo}"),
                None,
                steps,
            )
        }
        Some(SinkContainment::Contains) | Some(SinkContainment::Unverifiable { .. }) | None => {
            let detail = match sink {
                Some(SinkContainment::Unverifiable { reason }) => format!(
                    "sink containment could not be checked ({reason}); the verification step decides"
                ),
                _ => format!(
                    "designated sink ref {sink_ref} contains source commit {source}; record the integration proof, then strike the row"
                ),
            };
            closeout(
                ReportedCloseoutShape::IntegrationProof,
                detail,
                None,
                vec![
                    verify_step(facts, None),
                    fast_forward_record_step(document, claim, facts),
                    consume,
                ],
            )
        }
    }
}

/// The blocking operator-request packet for one reported attempt, in the
/// `SUBAGENT_OPERATOR_REQUEST_PROTOCOL` shape so finalize records it as the
/// ordinary `agent:todo` + `🙋` operator wait (`#packettodo`).
fn operator_request_packet(claim: &QueueClaim, closeout: &ReportedClaimCloseout) -> String {
    let item = claim_item_handle(&claim.item_text);
    let request = match &closeout.decision {
        Some(decision) => decision.question.clone(),
        None => format!(
            "The {} closeout of {}'s reported attempt (generation {}) for {item:?} cannot proceed. How should it close?",
            closeout.shape.as_str(),
            claim.owner,
            claim.generation,
        ),
    };
    let mut packet = format!(
        "[#{}] subagent operator request\n- *queue_item:*\n  - {item}\n- *kind:* question\n- *blocking:* true\n- *request:*\n  - {request}\n- *context:*\n  - Reported attempt {}#{} ({}).\n  - {}\n",
        closeout.operator_request_id,
        claim.owner,
        claim.generation,
        closeout.shape.as_str(),
        closeout.detail,
    );
    if !closeout.missing_facts.is_empty() {
        packet.push_str(&format!(
            "  - Missing facts: {}.\n",
            closeout.missing_facts.join(", ")
        ));
    }
    let choices = closeout
        .decision
        .as_ref()
        .map(|decision| decision.choices.as_slice())
        .unwrap_or_default();
    if choices.is_empty() {
        packet.push_str("- *choices:* []\n");
    } else {
        packet.push_str("- *choices:*\n");
        for choice in choices {
            packet.push_str(&format!("  - **{}**: {}\n", choice.label, choice.detail));
        }
    }
    packet
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

/// The next move of a closeout in one line: its next command, else its
/// decision and, when only the operator can answer, the packet to present.
pub fn closeout_next_move(closeout: &ReportedClaimCloseout) -> String {
    match (closeout.next_action_argv(), &closeout.decision) {
        (Some(next), _) => format!(
            "Next: `{}`. If a step is refused and `agent-doc queue claims` derives no other runnable step, present the blocking packet [#{}] from `reported_closeout.operator_request_packet` instead of ending the turn.",
            shell_command(next),
            closeout.operator_request_id,
        ),
        (None, Some(decision)) if decision.requires_operator => format!(
            "Next: ask the operator — present the blocking packet [#{}] from `reported_closeout.operator_request_packet` in this response. Question: {}",
            closeout.operator_request_id, decision.question
        ),
        (None, Some(decision)) => format!(
            "Next: decide from the worker's reported result and the repositories, then run only that branch's steps: {} Choices: {}. If that evidence does not settle it, present the blocking packet [#{}] from `reported_closeout.operator_request_packet`.",
            decision.question,
            decision
                .choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
            closeout.operator_request_id,
        ),
        (None, None) => format!(
            "Next: present the blocking packet [#{}] from `reported_closeout.operator_request_packet`.",
            closeout.operator_request_id
        ),
    }
}

/// The session-check warning for an overdue reported attempt.
pub fn overdue_reported_closeout_warning(
    claim: &QueueClaim,
    closeout: &ReportedClaimCloseout,
    commits_since_report: usize,
) -> String {
    let (_, since) = reported_since(claim);
    let missing = if closeout.missing_facts.is_empty() {
        String::new()
    } else {
        format!(" Missing facts: {}.", closeout.missing_facts.join(", "))
    };
    format!(
        "reported queue attempt {:?} ({}#{}) is still open after {commits_since_report} committed cycle(s) since {since} ({}); close it this turn, beside (never instead of) independent queue work. {}{missing}",
        claim_item_handle(&claim.item_text),
        claim.owner,
        claim.generation,
        closeout.shape.as_str(),
        closeout_next_move(closeout),
    )
}

/// Who makes a reported closeout's next move (`#questionsneverblock`).
///
/// A closeout is per-item debt. Whatever its readiness, it never gates
/// unrelated queue work: new claims, spawns, and independent heads proceed in
/// the same plan. Only the item itself and work that declares `after=` it
/// wait (the queue's own `#dagdraingate`, since a reported row stays open).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportedCloseoutReadiness {
    /// The coordinator can act this turn: a concrete common next step, or a
    /// decision it settles from the worker's report and the repositories.
    Coordinator,
    /// Only the operator can answer, and its request is not open yet:
    /// present the operator-request packet once, then leave the critical path.
    OperatorRequest,
    /// The operator request is already open in `agent:todo`: the explicit
    /// `🙋` wait. Nothing is owed this turn.
    AwaitingOperator,
}

impl ReportedCloseoutReadiness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coordinator => "coordinator",
            Self::OperatorRequest => "operator_request",
            Self::AwaitingOperator => "awaiting_operator",
        }
    }
}

impl ReportedClaimCloseout {
    /// The coordinator can make the next move without the operator: a
    /// concrete common next step, or a decision it may settle from evidence.
    pub fn coordinator_can_act(&self) -> bool {
        self.next_action_argv().is_some()
            || self
                .decision
                .as_ref()
                .is_some_and(|decision| !decision.requires_operator)
    }

    /// [`ReportedCloseoutReadiness`] against the open `agent:todo` ids.
    pub fn readiness(&self, open_todo_ids: &HashSet<String>) -> ReportedCloseoutReadiness {
        if open_todo_ids.contains(&self.operator_request_id.to_ascii_lowercase()) {
            ReportedCloseoutReadiness::AwaitingOperator
        } else if self.coordinator_can_act() {
            ReportedCloseoutReadiness::Coordinator
        } else {
            ReportedCloseoutReadiness::OperatorRequest
        }
    }
}

/// The scope rule every closeout entry states, so no reader takes one
/// item's open question as a barrier for the whole turn.
pub const REPORTED_CLOSEOUT_SCOPE_RULE: &str = "It gates only this item and work declared `after=` it: claim, spawn, and drain every independent queue head as planned";

/// One reported attempt's closeout as the plan presents it
/// (`#reportedcloseout`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedCloseoutObligation {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    /// Who makes the next move (`#questionsneverblock`).
    pub readiness: ReportedCloseoutReadiness,
    pub closeout: ReportedClaimCloseout,
}

impl ReportedCloseoutObligation {
    pub fn new(
        claim: &QueueClaim,
        closeout: ReportedClaimCloseout,
        open_todo_ids: &HashSet<String>,
    ) -> Self {
        Self {
            item: claim_item_handle(&claim.item_text),
            owner: claim.owner.clone(),
            generation: claim.generation,
            readiness: closeout.readiness(open_todo_ids),
            closeout,
        }
    }

    /// The open `agent:todo` ids include this attempt's operator request: the
    /// closeout is waiting on the operator, not on the coordinator.
    pub fn awaiting_operator(&self, open_todo_ids: &HashSet<String>) -> bool {
        self.readiness == ReportedCloseoutReadiness::AwaitingOperator
            || open_todo_ids.contains(&self.closeout.operator_request_id.to_ascii_lowercase())
    }

    /// Plan `required_commands` entries in order (`#questionsneverblock`).
    ///
    /// * [`ReportedCloseoutReadiness::Coordinator`]: a decision entry when the
    ///   closeout must choose first, then one entry per step, all work for
    ///   this turn and none of it a prerequisite for new claims or spawns.
    /// * [`ReportedCloseoutReadiness::OperatorRequest`]: one entry that
    ///   presents the operator-request packet; the branch steps wait for the
    ///   answer and stay in [`Self::closeout`].
    /// * [`ReportedCloseoutReadiness::AwaitingOperator`]: nothing; the open
    ///   `agent:todo` request is the wait.
    pub fn required_commands(&self) -> Vec<String> {
        let item = format!(
            "queue item {:?} ({}#{}, {})",
            self.item,
            self.owner,
            self.generation,
            self.closeout.shape.as_str(),
        );
        match self.readiness {
            ReportedCloseoutReadiness::AwaitingOperator => Vec::new(),
            ReportedCloseoutReadiness::OperatorRequest => vec![format!(
                "Reported {item} needs the operator. {} Present it once; finalize records it as the `🙋` wait in `agent:todo`. {REPORTED_CLOSEOUT_SCOPE_RULE}.",
                closeout_next_move(&self.closeout),
            )],
            ReportedCloseoutReadiness::Coordinator => self.coordinator_commands(&item),
        }
    }

    fn coordinator_commands(&self, item: &str) -> Vec<String> {
        let total = self.closeout.steps.len();
        let missing = if self.closeout.missing_facts.is_empty() {
            String::new()
        } else {
            format!(
                " Not runnable as written until lineage records: {}.",
                self.closeout.missing_facts.join(", ")
            )
        };
        let prefix =
            format!("Close reported {item} this turn, independent of new claims and spawns");
        let mut commands = Vec::new();
        if self.closeout.decision.is_some() {
            commands.push(format!(
                "{prefix}, decide first: {} {REPORTED_CLOSEOUT_SCOPE_RULE}.",
                closeout_next_move(&self.closeout)
            ));
        }
        commands.extend(self.closeout.steps.iter().enumerate().map(|(index, step)| {
            let branch = step
                .alternative
                .as_deref()
                .map(|label| format!(" [only on choice `{label}`]"))
                .unwrap_or_default();
            format!(
                "{prefix}, step {}/{total} ({}){branch}: run `{}`. {}.{}",
                index + 1,
                step.action.as_str(),
                shell_command(&step.argv),
                step.detail.trim_end_matches('.'),
                if step.concrete { "" } else { missing.as_str() },
            )
        }));
        commands
    }
}

/// The reported closeout a turn's closeout report must not end silently on
/// (`#reportedcloseoutstall`): the first obligation that is not already
/// waiting on an open operator request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReportedCloseout {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub shape: ReportedCloseoutShape,
    /// The next command when one is common to every branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
    /// Only the operator can make the next move: present the packet.
    pub requires_operator: bool,
    pub operator_request_id: String,
    pub operator_request_packet: String,
    /// Closeouts already waiting on an open operator request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub awaiting_operator: Vec<String>,
    pub guidance: String,
}

/// `#reportedcloseoutstall`: a reported claim whose closeout is still owed
/// never ends a turn in silence. Returns the closeout that keeps continuation
/// required, or `None` when every reported attempt is closed or already
/// waiting on its open operator request (the ordinary `🙋` wait).
pub fn pending_reported_closeout(
    obligations: &[ReportedCloseoutObligation],
    open_todo_ids: &HashSet<String>,
) -> Option<PendingReportedCloseout> {
    let (waiting, pending): (Vec<_>, Vec<_>) = obligations
        .iter()
        .partition(|obligation| obligation.awaiting_operator(open_todo_ids));
    let first = pending.first()?;
    let closeout = &first.closeout;
    let guidance = format!(
        "Continuation is required: reported queue item {:?} ({}#{}, {}) still owes its closeout, so this turn must not end as complete. {} Run `agent-doc queue claims <FILE>` for the full ordered steps. {REPORTED_CLOSEOUT_SCOPE_RULE}.{}",
        first.item,
        first.owner,
        first.generation,
        closeout.shape.as_str(),
        closeout_next_move(closeout),
        if pending.len() > 1 {
            format!(" {} more reported closeout(s) follow.", pending.len() - 1)
        } else {
            String::new()
        },
    );
    Some(PendingReportedCloseout {
        item: first.item.clone(),
        owner: first.owner.clone(),
        generation: first.generation,
        shape: closeout.shape,
        next_command: closeout.next_action_argv().map(shell_command),
        requires_operator: closeout.requires_operator(),
        operator_request_id: closeout.operator_request_id.clone(),
        operator_request_packet: closeout.operator_request_packet.clone(),
        awaiting_operator: waiting
            .iter()
            .map(|obligation| obligation.item.clone())
            .collect(),
        guidance,
    })
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
            heartbeat_at_secs: None,
            frozen: None,
            quota_block: None,
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
                "clidocsreleasecleanup",
                "--ack-owner",
                "subagent:c948clidocs#3",
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
            sink: Some(SinkContainment::DoesNotContain {
                fast_forward: true,
                checked_out_worktree: None,
            }),
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
            sink: Some(SinkContainment::DoesNotContain {
                fast_forward: true,
                checked_out_worktree: None,
            }),
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
    fn missing_repo_facts_are_named_with_a_step_that_records_them() {
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
        // Inspecting lineage only re-confirms the gap; the first step records it.
        let record_facts = &closeout.steps[0];
        assert_eq!(
            record_facts.action,
            ReportedCloseoutAction::RecordRepositoryWork
        );
        assert!(!record_facts.concrete);
        assert_eq!(
            record_facts.argv,
            argv(&[
                "agent-doc",
                "queue",
                "lineage-record",
                DOC,
                "--item",
                "#half",
                "--generation",
                "3",
                "--event",
                "committed",
                "--source-repo",
                "/repo/worktree",
                "--source-sha",
                "<source-sha>",
                "--sink-repo",
                "/repo/main",
                "--sink-ref",
                "<sink-ref>",
            ])
        );
        let record = &closeout.steps[2];
        assert_eq!(record.action, ReportedCloseoutAction::RecordIntegration);
        assert!(!record.concrete);
        assert!(record.argv.iter().any(|arg| arg == "<source-sha>"));
        assert!(record.argv.iter().any(|arg| arg == "<sink-ref>"));
        // Where unrequested repository work lands is the operator's call.
        let decision = closeout.decision.as_ref().unwrap();
        assert!(decision.requires_operator, "{decision:?}");
        assert!(
            decision.question.contains("Which branch or ref"),
            "{decision:?}"
        );
        assert!(closeout.next_action_argv().is_none());
    }

    /// `#reportedcloseoutfacts` (sdk-dev 2026-11-04, c631tsjs): a started
    /// worker with no repository facts and no prose declaration committed
    /// 656a66d on a side branch. Treating that as prose struck the row and
    /// orphaned the commit; it is missing facts with both recording branches.
    #[test]
    fn undeclared_attempt_is_missing_facts_with_both_recording_branches() {
        let item = "https://github.com/haiven-dev/haiven-sdk-ts: Convert *.ts files to *.js + *.d.ts [#q-0d0f4fa99c3f]";
        let claim = reported(item, "subagent:c631tsjs");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::Undeclared).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
        assert_eq!(closeout.missing_facts, ["repo_work_or_prose_declaration"]);
        // No step common to every branch may run first: the consume step is
        // refused, and either recording step settles the unknown.
        assert!(closeout.next_action_argv().is_none(), "{closeout:#?}");
        let decision = closeout.decision.as_ref().unwrap();
        assert!(!decision.requires_operator);
        assert_eq!(
            decision
                .choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>(),
            ["repository_work", "prose_only"]
        );
        let repo = &closeout.steps[0];
        assert_eq!(repo.action, ReportedCloseoutAction::RecordRepositoryWork);
        assert_eq!(repo.alternative.as_deref(), Some("repository_work"));
        assert!(!repo.concrete);
        assert_eq!(
            repo.argv,
            argv(&[
                "agent-doc",
                "queue",
                "lineage-record",
                DOC,
                "--item",
                item,
                "--generation",
                "3",
                "--event",
                "committed",
                "--source-repo",
                "<source-repo>",
                "--source-sha",
                "<source-sha>",
                "--sink-repo",
                "<sink-repo>",
                "--sink-ref",
                "<sink-ref>",
                "--worktree",
                "<worktree>",
                "--branch",
                "<branch>",
            ])
        );
        let prose = &closeout.steps[1];
        assert_eq!(prose.action, ReportedCloseoutAction::DeclareProseOnly);
        assert_eq!(prose.alternative.as_deref(), Some("prose_only"));
        assert!(prose.concrete);
        assert_eq!(
            prose.argv,
            argv(&[
                "agent-doc",
                "queue",
                "lineage-record",
                DOC,
                "--item",
                item,
                "--generation",
                "3",
                "--event",
                "prose-only",
            ])
        );
        let consume = &closeout.steps[2];
        assert_eq!(consume.action, ReportedCloseoutAction::Consume);
        assert!(
            !consume.concrete,
            "consume refuses until a branch is recorded"
        );
    }

    #[test]
    fn missing_lineage_attempt_records_the_attempt_rather_than_inspecting_it() {
        let claim = reported("free text work", "subagent:x");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::LineageMissing).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
        assert_eq!(closeout.missing_facts, ["queue_lineage_attempt"]);
        assert!(closeout.next_action_argv().is_none());
        let actions = closeout
            .steps
            .iter()
            .map(|step| step.action)
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [
                ReportedCloseoutAction::RecordRepositoryWork,
                ReportedCloseoutAction::DeclareProseOnly,
                ReportedCloseoutAction::Consume,
            ]
        );
    }

    /// `#reportedcloseoutfacts` (sdk-dev 2026-11-04, c106telem): the plan's
    /// fast-forward push was refused as non-fast-forward and its fallback
    /// existed only as prose. A sink that moved past the worker's base gets
    /// explicit cherry-pick and merge branches instead of a doomed push.
    #[test]
    fn non_fast_forward_sink_gets_explicit_cherry_pick_and_merge_branches() {
        let claim = reported("do [#fix]", "subagent:fix");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::DoesNotContain {
                fast_forward: false,
                checked_out_worktree: Some("/repo/main-wt".to_string()),
            }),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationRequired);
        assert!(
            closeout
                .steps
                .iter()
                .all(|step| step.action != ReportedCloseoutAction::Integrate),
            "a refused fast-forward push is never offered: {closeout:#?}"
        );
        let branch = |label: &str| {
            closeout
                .steps
                .iter()
                .filter(|step| step.alternative.as_deref() == Some(label))
                .collect::<Vec<_>>()
        };
        let flag = |step: &ReportedCloseoutStep, name: &str| {
            let index = step.argv.iter().position(|arg| arg == name).unwrap();
            step.argv[index + 1].clone()
        };
        let cherry = branch("cherry_pick");
        assert_eq!(cherry[0].action, ReportedCloseoutAction::CherryPickOntoSink);
        assert_eq!(
            cherry[0].argv,
            argv(&["git", "-C", "/repo/main-wt", "cherry-pick", "abc123"])
        );
        assert!(cherry[0].concrete);
        assert_eq!(flag(cherry[1], "--method"), "cherry-pick");
        assert_eq!(flag(cherry[1], "--integration-sha"), "<sink-commit-sha>");
        assert!(!cherry[1].concrete);
        let merge = branch("merge");
        assert_eq!(
            merge[0].argv,
            argv(&[
                "git",
                "-C",
                "/repo/main-wt",
                "merge",
                "--no-ff",
                "--no-edit",
                "abc123"
            ])
        );
        assert_eq!(merge[1].action, ReportedCloseoutAction::VerifyIntegration);
        assert_eq!(flag(merge[2], "--method"), "merge");
        let decision = closeout.decision.as_ref().unwrap();
        assert!(!decision.requires_operator);
        assert!(closeout.next_action_argv().is_none());
    }

    #[test]
    fn fast_forward_into_a_checked_out_sink_runs_in_that_worktree() {
        let claim = reported("do [#fix]", "subagent:fix");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::DoesNotContain {
                fast_forward: true,
                checked_out_worktree: Some("/repo/main-wt".to_string()),
            }),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(
            closeout.next_action_argv().unwrap(),
            argv(&["git", "-C", "/repo/main-wt", "merge", "--ff-only", "abc123"])
        );
        assert!(closeout.decision.is_none());
    }

    /// c106telem's recorded source is a haiven-contracts commit and its sink a
    /// haiven-api ref: no history in common, so the choice is the operator's.
    #[test]
    fn source_without_shared_sink_history_is_an_operator_sink_choice() {
        let claim = reported("do [#publishtelemetryannotati]", "subagent:c106telem");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::NoSharedHistory),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::SinkMismatch);
        assert!(closeout.requires_operator());
        let correct = &closeout.steps[0];
        assert_eq!(correct.action, ReportedCloseoutAction::RecordRepositoryWork);
        assert_eq!(correct.alternative.as_deref(), Some("correct_sink"));
        assert!(
            correct
                .argv
                .windows(2)
                .any(|pair| pair == ["--sink-ref", "<sink-ref>"])
        );
        assert!(
            correct
                .argv
                .windows(2)
                .any(|pair| pair == ["--sink-repo", "<sink-repo>"]),
            "a wrong sink is never re-proposed: {correct:?}"
        );
        assert!(
            correct
                .argv
                .windows(2)
                .any(|pair| pair == ["--source-sha", "abc123"])
        );
        let packet = &closeout.operator_request_packet;
        let parsed = crate::operator_request_packet::blocking_operator_request_packets(packet);
        assert_eq!(parsed.len(), 1, "{packet}");
        assert_eq!(parsed[0].id, closeout.operator_request_id);
        assert_eq!(
            parsed[0].queue_item.as_deref(),
            Some("#publishtelemetryannotati")
        );
        assert_eq!(parsed[0].choice_labels, ["correct_sink", "cherry_pick"]);
        // The packet's queue_item links the `🙋` wait to the reported row.
        assert!(
            agent_doc_document::queue_projection::operator_request_item_matches_row(
                "do [#publishtelemetryannotati]",
                parsed[0].queue_item.as_deref().unwrap(),
            )
        );
        assert!(
            crate::operator_request_packet::blocking_operator_request_recommendation_error(
                &parsed[0]
            )
            .is_none()
        );
    }

    #[test]
    fn operator_request_id_is_stable_per_attempt() {
        let claim = reported("do [#fix]", "subagent:fix");
        let id = operator_request_id(&claim);
        assert!(
            id.starts_with("subagent-operator-request-reportedcloseout-"),
            "{id}"
        );
        assert_eq!(id, operator_request_id(&claim.clone()));
        let mut next = claim.clone();
        next.generation += 1;
        assert_ne!(id, operator_request_id(&next));
        assert!(
            crate::subagent_brief::subagent_operator_request_id(&format!(
                "[#{id}] subagent operator request"
            ))
            .is_some()
        );
    }

    #[test]
    fn stale_integration_proof_is_reproven() {
        let claim = reported("do [#stale]", "subagent:stale");
        let evidence = ReportedRepoEvidence::IntegratedUnverified {
            facts: facts(),
            reason: "integration commit is not reachable from the designated sink ref".to_string(),
            patch_id_mismatch: false,
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

    /// `#lineageconflictresolved`: a cherry-pick refused for a patch-id
    /// mismatch is re-proven as a conflict-resolved integration naming its
    /// source, never by repeating the same refused method or by a
    /// fast-forward record the sink cannot satisfy.
    #[test]
    fn patch_id_mismatch_is_reproven_as_conflict_resolved() {
        let claim = reported("do [#resolved]", "subagent:resolved");
        let evidence = ReportedRepoEvidence::IntegratedUnverified {
            facts: facts(),
            reason: "explicit CherryPick mapping has different patch ids: a != b".to_string(),
            patch_id_mismatch: true,
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationProofStale);
        let records: Vec<_> = closeout
            .steps
            .iter()
            .filter(|step| step.action == ReportedCloseoutAction::RecordIntegration)
            .collect();
        assert_eq!(records.len(), 1, "{closeout:#?}");
        let flag = |name: &str| {
            let index = records[0].argv.iter().position(|arg| arg == name).unwrap();
            records[0].argv[index + 1].clone()
        };
        assert_eq!(flag("--method"), "conflict-resolved");
        assert_eq!(flag("--resolves"), "abc123");
        assert_eq!(flag("--source-sha"), "abc123");
        assert_eq!(flag("--integration-sha"), "<sink-commit-sha>");
        assert!(!records[0].concrete);
        assert!(
            closeout
                .steps
                .iter()
                .all(|step| step.action != ReportedCloseoutAction::VerifyIntegration),
            "an ancestry check cannot pass for a conflict-resolved commit: {closeout:#?}"
        );

        let cherry = reported_claim_closeout(
            DOC,
            &claim,
            &ReportedRepoEvidence::Unintegrated {
                facts: facts(),
                sink: Some(SinkContainment::DoesNotContain {
                    fast_forward: false,
                    checked_out_worktree: None,
                }),
            },
        )
        .unwrap();
        let cherry_record = cherry
            .steps
            .iter()
            .find(|step| {
                step.action == ReportedCloseoutAction::RecordIntegration
                    && step.alternative.as_deref() == Some(CHERRY_PICK)
            })
            .unwrap();
        assert!(
            cherry_record
                .detail
                .contains("--method conflict-resolved --resolves abc123"),
            "{cherry_record:#?}"
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
    fn every_shape_has_a_next_action_or_a_decision() {
        let claim = reported("do [#any]", "subagent:any");
        let mut evidences = vec![
            ReportedRepoEvidence::LineageMissing,
            ReportedRepoEvidence::LineageAmbiguous,
            ReportedRepoEvidence::ProseOnly,
            ReportedRepoEvidence::Undeclared,
            ReportedRepoEvidence::IntegratedVerified,
            ReportedRepoEvidence::IntegratedUnverified {
                facts: ReportedRepoFacts::default(),
                reason: "x".to_string(),
                patch_id_mismatch: false,
            },
            ReportedRepoEvidence::IntegratedUnverified {
                facts: facts(),
                reason: "x".to_string(),
                patch_id_mismatch: true,
            },
        ];
        for sink in [
            None,
            Some(SinkContainment::Contains),
            Some(SinkContainment::DoesNotContain {
                fast_forward: true,
                checked_out_worktree: None,
            }),
            Some(SinkContainment::DoesNotContain {
                fast_forward: false,
                checked_out_worktree: None,
            }),
            Some(SinkContainment::NoSharedHistory),
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
            assert!(
                closeout.next_action_argv().is_some() || closeout.decision.is_some(),
                "{evidence:?} -> {closeout:?}"
            );
            assert_eq!(
                closeout.steps.last().unwrap().action,
                ReportedCloseoutAction::Consume
            );
            // A missing fact always comes with a decision, and (except for an
            // ambiguous ledger, which only the operator can untangle) a step
            // that records it: inspection alone is never the plan.
            if !closeout.missing_facts.is_empty() {
                assert!(closeout.decision.is_some(), "{evidence:?}");
                assert!(
                    evidence == ReportedRepoEvidence::LineageAmbiguous
                        || closeout.steps.iter().any(|step| matches!(
                            step.action,
                            ReportedCloseoutAction::RecordRepositoryWork
                                | ReportedCloseoutAction::DeclareProseOnly
                        )),
                    "{evidence:?} -> {closeout:?}"
                );
                assert!(
                    closeout
                        .steps
                        .iter()
                        .filter(|step| step.action == ReportedCloseoutAction::Consume)
                        .all(|step| !step.concrete)
                );
            }
            // Every alternative a step names is a choice of the decision.
            let labels = closeout
                .decision
                .iter()
                .flat_map(|decision| decision.choices.iter().map(|choice| choice.label.clone()))
                .collect::<Vec<_>>();
            for step in &closeout.steps {
                if let Some(label) = &step.alternative {
                    assert!(labels.contains(label), "{label} not in {labels:?}");
                }
                // A placeholder is never presented as runnable.
                if step.argv.iter().any(|arg| is_placeholder(arg)) {
                    assert!(!step.concrete, "{step:?}");
                }
            }
            // The packet is always one the closeout machinery records.
            let packets = crate::operator_request_packet::blocking_operator_request_packets(
                &closeout.operator_request_packet,
            );
            assert_eq!(packets.len(), 1, "{}", closeout.operator_request_packet);
            assert_eq!(packets[0].choice_labels, labels);
        }
    }

    #[test]
    fn pending_closeout_keeps_continuation_until_the_operator_request_is_open() {
        let claim = reported("free text work", "subagent:x");
        let obligation = ReportedCloseoutObligation::new(
            &claim,
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::Undeclared).unwrap(),
            &HashSet::new(),
        );
        let pending =
            pending_reported_closeout(std::slice::from_ref(&obligation), &HashSet::new())
                .expect("an owed closeout never ends the turn silently");
        assert_eq!(pending.shape, ReportedCloseoutShape::MissingFacts);
        assert!(pending.next_command.is_none());
        assert!(
            pending.guidance.starts_with("Continuation is required"),
            "{}",
            pending.guidance
        );
        assert!(
            pending.guidance.contains("repository_work | prose_only"),
            "{}",
            pending.guidance
        );
        assert!(pending.guidance.contains(&pending.operator_request_id));

        let open = HashSet::from([obligation.closeout.operator_request_id.clone()]);
        assert!(
            pending_reported_closeout(std::slice::from_ref(&obligation), &open).is_none(),
            "an open operator request is the explicit wait"
        );
        assert!(pending_reported_closeout(&[], &HashSet::new()).is_none());
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
            &HashSet::new(),
        );
        assert_eq!(obligation.readiness, ReportedCloseoutReadiness::Coordinator);
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
                "step 3/3 (consume): run `agent-doc queue consume tasks/session.md --ack-id fix --ack-owner 'subagent:fix#3'`"
            ),
            "{}",
            commands[2]
        );
        assert!(commands.iter().all(|command| {
            command.contains("this turn, independent of new claims and spawns")
                && !command.contains("before any new")
        }));
    }

    /// `#questionsneverblock` (sdk-dev 2026-11-04): an operator-gated
    /// closeout is one packet entry that names its scope, never a list of
    /// "before any new claim" steps that stalls every independent head.
    #[test]
    fn operator_gated_obligation_is_one_packet_entry_scoped_to_its_item() {
        let claim = reported("do [#port]", "subagent:port");
        let evidence = ReportedRepoEvidence::Unintegrated {
            facts: facts(),
            sink: Some(SinkContainment::NoSharedHistory),
        };
        let closeout = reported_claim_closeout(DOC, &claim, &evidence).unwrap();
        assert!(closeout.requires_operator(), "{closeout:#?}");
        assert!(!closeout.coordinator_can_act());
        let request_id = closeout.operator_request_id.clone();
        let obligation = ReportedCloseoutObligation::new(&claim, closeout.clone(), &HashSet::new());
        assert_eq!(
            obligation.readiness,
            ReportedCloseoutReadiness::OperatorRequest
        );
        let commands = obligation.required_commands();
        assert_eq!(commands.len(), 1, "{commands:#?}");
        assert!(commands[0].contains(&request_id), "{}", commands[0]);
        assert!(commands[0].contains("Present it once"), "{}", commands[0]);
        assert!(
            commands[0].contains(REPORTED_CLOSEOUT_SCOPE_RULE),
            "{}",
            commands[0]
        );
        assert!(!commands[0].contains("before any new"), "{}", commands[0]);

        // Once the request is open in agent:todo the wait is explicit and
        // the plan owes nothing more for this item.
        let open = HashSet::from([request_id]);
        let waiting = ReportedCloseoutObligation::new(&claim, closeout, &open);
        assert_eq!(
            waiting.readiness,
            ReportedCloseoutReadiness::AwaitingOperator
        );
        assert!(waiting.required_commands().is_empty());
        assert!(waiting.awaiting_operator(&HashSet::new()));
    }

    /// A decision the coordinator settles from evidence is this turn's work,
    /// listed as its decision plus branch steps, but still not a gate.
    #[test]
    fn coordinator_decidable_obligation_lists_its_decision_without_gating() {
        let claim = reported("free text work", "subagent:x");
        let closeout =
            reported_claim_closeout(DOC, &claim, &ReportedRepoEvidence::Undeclared).unwrap();
        assert!(!closeout.requires_operator(), "{closeout:#?}");
        assert!(closeout.coordinator_can_act());
        let obligation = ReportedCloseoutObligation::new(&claim, closeout.clone(), &HashSet::new());
        assert_eq!(obligation.readiness, ReportedCloseoutReadiness::Coordinator);
        let commands = obligation.required_commands();
        assert_eq!(commands.len(), 1 + closeout.steps.len());
        assert!(commands[0].contains("decide first"), "{}", commands[0]);
        assert!(
            commands[0].contains(REPORTED_CLOSEOUT_SCOPE_RULE),
            "{}",
            commands[0]
        );
        assert!(
            commands
                .iter()
                .all(|command| !command.contains("before any new"))
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
