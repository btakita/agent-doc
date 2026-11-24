//! Remote observation for `#address` completion (`#addressproof`).
//!
//! The queue crate decides. This module only reads the pull request. A fixture
//! owner (`sample-org`) is never sent to the network. GitHub observation is off
//! until the process enables it, so unit tests stay offline and fail closed.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{error::Error, fmt};

use agent_doc_queue::address_completion_proof::{
    AddressCompletion, AddressEffectFacts, AddressEffectGuard, AddressObservation,
    AddressReviewState, AddressSnapshot, AddressTarget, address_prompt_revision,
    address_review_target, forced_observation, judge_address_completion,
};
use agent_doc_queue::queue_claim::QueueClaim;
use agent_doc_sqlite::state_store;
use anyhow::{Context, Result};
use serde::Deserialize;

static GITHUB: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct CommittedAddressEffectReconciled {
    attempts: Vec<(String, String, Option<String>)>,
}

impl fmt::Display for CommittedAddressEffectReconciled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#addressproof: reconciled committed prior effect(s) {}; retry admission from the newly durable proof",
            self.attempts
                .iter()
                .map(|(cycle, target, _)| format!("{cycle}:{target}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

impl Error for CommittedAddressEffectReconciled {}

/// A before-observation was reconciled during admission. The selected prompt
/// must not execute, but the now-durable proof permits one fresh queue read.
pub fn committed_effect_reconciled(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<CommittedAddressEffectReconciled>()
        .is_some()
}

/// Queue ids whose exact prior inline effects were proven during admission.
///
/// The caller may exclude these ids from one immediate admission retry. The
/// durable before/proven receipts remain intact, so the exclusion advances the
/// queue without turning the reconciled prompt into a fresh execution.
pub fn reconciled_effect_queue_head_ids(error: &anyhow::Error) -> Option<Vec<String>> {
    error
        .downcast_ref::<CommittedAddressEffectReconciled>()?
        .attempts
        .iter()
        .map(|(_, _, queue_head_id)| queue_head_id.clone())
        .collect()
}

/// Allow this process to read pull requests with `gh`.
pub fn enable_github_observation() {
    GITHUB.store(true, Ordering::Relaxed);
}

pub fn github_enabled() -> bool {
    GITHUB.load(Ordering::Relaxed)
}

/// Record an inline execution after its cycle has been admitted, before the
/// agent receives the prompt.
pub fn capture_before_execution(
    file: &Path,
    submitted: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<()> {
    let effects = submitted
        .into_iter()
        .filter_map(|item| {
            let revision = address_prompt_revision(item.as_ref());
            let target = remote_target(&revision)?;
            Some((revision, target))
        })
        .collect::<Vec<_>>();
    if effects.is_empty() {
        return Ok(());
    }
    let cycle = open_cycle(file)?.context("address observation requires an admitted cycle")?;
    reserve_inline_execution(file, &cycle, &effects, fetch_snapshot)
}

/// The existing before receipt is also a durable hold. Admission cannot
/// distinguish a crash just before prompt delivery from one just after it.
/// Retain the observed attempt rather than sampling a new baseline.
fn inline_hold(
    conn: &state_store::Connection,
    document_hash: &str,
    revision: &str,
    target: &AddressTarget,
) -> Result<Option<String>> {
    const PREFIX: &str = "address_effect_v2:before:inline:";
    let suffix = format!(":{}:{}", revision_hash(revision), target.key());
    let mut query = conn.prepare(
        "SELECT state_kind FROM queue_document_state
         WHERE document_hash = ?1 AND substr(state_kind, 1, length(?2)) = ?2
           AND substr(state_kind, -length(?3)) = ?3
           AND length(state_kind) > length(?2) + length(?3)
         ORDER BY state_kind LIMIT 2",
    )?;
    let mut rows = query.query((document_hash, PREFIX, &suffix))?;
    let Some(first) = rows.next()? else {
        return Ok(None);
    };
    let state_kind: String = first.get(0)?;
    anyhow::ensure!(
        rows.next()?.is_none(),
        "#addressproof: multiple earlier inline executions for {} require exact cycle reconciliation",
        target.key()
    );
    Ok(Some(
        state_kind[PREFIX.len()..state_kind.len() - suffix.len()].to_string(),
    ))
}

fn inline_observed_cycle(
    file: &Path,
    revision: &str,
    target: &AddressTarget,
) -> Result<Option<String>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    if !state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = state_store::open_state_db_read_only(&root)?;
    if !state_store::state_table_exists(&conn, "queue_document_state")? {
        return Ok(None);
    }
    inline_hold(&conn, &agent_doc_hash::path_hash(file)?, revision, target)
}

/// A before observation may fetch its after-state while its owning turn is
/// still open, or after that exact turn has durably committed a response.
///
/// The committed case is recovery, not a new execution: closeout normally
/// performs this observation while the cycle is open, but a process/editor
/// interruption can land the response and miss only the read-only GitHub
/// observation. Abandoned and merely-started cycles remain fenced because
/// they do not prove that the agent ever received or answered the prompt.
fn inline_cycle_allows_after_observation(
    file: &Path,
    current_open_cycle: Option<&str>,
    prior_cycle: &str,
) -> Result<bool> {
    if current_open_cycle == Some(prior_cycle) {
        return Ok(true);
    }
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(false);
    };
    if !state_store::state_db_path(&root).exists() {
        return Ok(false);
    }
    let conn = state_store::open_state_db_read_only(&root)?;
    if !state_store::state_table_exists(&conn, "document_cycles")? {
        return Ok(false);
    }
    let document_id = file.canonicalize()?.to_string_lossy().into_owned();
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM document_cycles
             WHERE document_id = ?1 AND cycle_id = ?2
               AND state IN ('committed', 'completed', 'closed')
               AND response_commit IS NOT NULL
         )",
        (&document_id, prior_cycle),
        |row| row.get(0),
    )
    .context("read committed inline address cycle")
}

fn refuse_inline_hold(cycle: Option<String>, target: &AddressTarget) -> Result<()> {
    if let Some(cycle) = cycle {
        anyhow::bail!(
            "#addressproof: inline cycle {cycle} already received a before observation for {}. Reconcile the exact prior response and remote effect before repeating this prompt revision.",
            target.key(),
        );
    }
    Ok(())
}

fn reserve_inline_execution(
    file: &Path,
    cycle: &str,
    effects: &[(String, AddressTarget)],
    mut fetch: impl FnMut(&AddressTarget) -> Option<AddressSnapshot>,
) -> Result<()> {
    let root = agent_doc_fs::find_project_root(file).context("no agent-doc project root")?;
    let mut conn = state_store::open_state_db(&root)?;
    let document_hash = agent_doc_hash::path_hash(file)?;
    let mut baselines = Vec::new();
    let mut reconciled_prior_cycles = Vec::new();
    for (revision, target) in effects {
        if let Some(prior_cycle) = inline_hold(&conn, &document_hash, revision, target)? {
            let committed = inline_cycle_allows_after_observation(file, None, &prior_cycle)?;
            if committed {
                let prior_attempt = AddressAttempt::inline(&prior_cycle, revision);
                let observation =
                    observe_snapshot(file, &prior_attempt, target, |target| fetch(target))?;
                if matches!(
                    judge_address_completion(&observation, None),
                    AddressCompletion::Proven(_)
                ) {
                    reconciled_prior_cycles.push((
                        prior_cycle,
                        target.key(),
                        agent_doc_queue::queue_response::queue_prompt_done_id(revision),
                    ));
                    continue;
                }
            }
            refuse_inline_hold(Some(prior_cycle), target)?;
        }
        let baseline = fetch(target).context(
            "could not capture the pull-request baseline before dispatch (#addressproof)",
        )?;
        baselines.push((revision, target, baseline));
    }
    if !reconciled_prior_cycles.is_empty() {
        return Err(CommittedAddressEffectReconciled {
            attempts: reconciled_prior_cycles,
        }
        .into());
    }
    // The before receipts are a single admission cut. A failure before the
    // handoff exposes none of the prompts and leaves none with a false hold.
    let tx = agent_doc_sqlite::immediate_transaction(&mut conn)?;
    for (revision, target, baseline) in baselines {
        refuse_inline_hold(inline_hold(&tx, &document_hash, revision, target)?, target)?;
        insert_snapshot(
            &tx,
            file,
            &AddressAttempt::inline(cycle, revision),
            target,
            SnapshotKind::Before,
            &baseline,
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Record the before observation for the exact reserved worker attempt.
pub fn capture_before_worker(file: &Path, claim: &QueueClaim) -> Result<()> {
    let Some(target) = remote_target(&claim.item_text) else {
        return Ok(());
    };
    refuse_inline_hold(
        inline_observed_cycle(file, &claim.item_text, &target)?,
        &target,
    )?;
    let attempt = AddressAttempt::for_claim(claim);
    if load_snapshot(file, &attempt, &target, SnapshotKind::Before)?.is_none()
        && inherit_proven_stage(file, claim, &attempt, &target)?
    {
        return Ok(());
    }
    ensure_dispatch_baseline(file, &attempt, &target, fetch_snapshot)
}

/// Validate the durable before receipt at spawn. A reservation that lost
/// this receipt may have reached native spawn before its bind was saved.
pub fn require_worker_observation(file: &Path, claim: &QueueClaim) -> Result<()> {
    let Some(target) = remote_target(&claim.item_text) else {
        return Ok(());
    };
    refuse_inline_hold(
        inline_observed_cycle(file, &claim.item_text, &target)?,
        &target,
    )?;
    let attempt = AddressAttempt::for_claim(claim);
    anyhow::ensure!(
        load_snapshot(file, &attempt, &target, SnapshotKind::Before)?.is_some(),
        "address generation {} has no before receipt; reconcile this reservation before native spawn (#addressproof)",
        claim.generation,
    );
    Ok(())
}

/// Recover a proved address stage of the same revision when the goal
/// hand-back requeued a later stage. A different revision, a plain requeue,
/// or an unproved prior attempt cannot inherit evidence.
fn inherit_proven_stage(
    file: &Path,
    claim: &QueueClaim,
    attempt: &AddressAttempt,
    target: &AddressTarget,
) -> Result<bool> {
    let ledger = crate::goal_loop_handoff::load(file)?;
    let Some(prior) = ledger.latest_for(&claim.identity).filter(|prior| {
        prior.generation < claim.generation
            && address_prompt_revision(&prior.item_text)
                == address_prompt_revision(&claim.item_text)
    }) else {
        return Ok(false);
    };
    anyhow::ensure!(
        !prior.address_effect_unknown,
        "address generation {} still has an unknown remote effect; reconcile its retained hand-back before dispatching generation {}",
        prior.generation,
        claim.generation,
    );
    if prior.address_fix_pending || prior.address_effect_unproven {
        return Ok(false);
    }
    let prior_attempt = AddressAttempt::worker(
        &prior.identity,
        &prior.owner,
        prior.generation,
        &prior.item_text,
    );
    let Some(proven) = load_snapshot(file, &prior_attempt, target, SnapshotKind::Proven)? else {
        return Ok(false);
    };
    let Some(before) = load_snapshot(file, &prior_attempt, target, SnapshotKind::Before)? else {
        anyhow::bail!("address observation has a prior proof without its before observation");
    };
    save_snapshot(file, attempt, target, SnapshotKind::Before, &before)?;
    save_snapshot(file, attempt, target, SnapshotKind::Proven, &proven)?;
    Ok(true)
}

#[derive(Debug)]
struct AddressAttempt(String);

impl AddressAttempt {
    fn inline(cycle: &str, item: &str) -> Self {
        Self(format!("inline:{cycle}:{}", revision_hash(item)))
    }

    fn for_claim(claim: &QueueClaim) -> Self {
        Self::worker(
            &claim.identity,
            &claim.owner,
            claim.generation,
            &claim.item_text,
        )
    }

    fn worker(
        identity: &agent_doc_element_queue::QueueItemIdentity,
        owner: &str,
        generation: u64,
        item: &str,
    ) -> Self {
        let identity = match identity {
            agent_doc_element_queue::QueueItemIdentity::Id(id) => format!("id:{id}"),
            agent_doc_element_queue::QueueItemIdentity::FreeText(text) => format!("text:{text}"),
        };
        Self(format!(
            "worker:{}:{}:{generation}:{}",
            agent_doc_hash::content_hash(&identity),
            agent_doc_hash::content_hash(owner),
            revision_hash(item)
        ))
    }
}

fn revision_hash(item: &str) -> String {
    agent_doc_hash::content_hash(&address_prompt_revision(item))
}

fn open_cycle(file: &Path) -> Result<Option<String>> {
    Ok(agent_doc_cycle_state_io::load(file)?
        .filter(|cycle| cycle.is_open())
        .map(|cycle| cycle.cycle_id))
}

fn ensure_dispatch_baseline(
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    fetch: impl FnOnce(&AddressTarget) -> Option<AddressSnapshot>,
) -> Result<()> {
    if load_snapshot(file, attempt, target, SnapshotKind::Before)?.is_none() {
        let snapshot = fetch(target).context(
            "could not capture the pull-request baseline before dispatch (#addressproof)",
        )?;
        save_snapshot(file, attempt, target, SnapshotKind::Before, &snapshot)?;
    }
    Ok(())
}

/// An uncorrelated or unreadable observation of a real target leaves its
/// attempt reportable, without authorizing another execution. Fixtures and
/// offline consumers leave GitHub observation disabled.
pub fn worker_observation_requires_reconciliation(
    claim: &QueueClaim,
    observation: &AddressObservation,
) -> bool {
    let checked = remote_target(&claim.item_text).is_some()
        || forced_observation() == Some(AddressObservation::MissingBaseline);
    checked
        && matches!(
            observation,
            AddressObservation::Unseen | AddressObservation::MissingBaseline
        )
}

pub fn observation_for_claim(file: &Path, claim: &QueueClaim) -> AddressObservation {
    if let Some(observation) = forced_observation() {
        return observation;
    }
    let Some(target) = remote_target(&claim.item_text) else {
        return AddressObservation::Unseen;
    };
    let attempt = AddressAttempt::for_claim(claim);
    observe_or_warn(file, &attempt, &target, true)
}

/// Install per-row observations for the strike/consumption decision.
pub fn install_for_content(file: &Path, content: &str) -> Option<AddressEffectGuard> {
    let forced = forced_observation();
    if forced.is_none() && !github_enabled() {
        return None;
    }
    let rows = address_item_texts(content);
    if rows.is_empty() {
        return None;
    }
    let cycle = match agent_doc_cycle_state_io::load(file) {
        Ok(cycle) => cycle.filter(|cycle| {
            cycle.is_open()
                || (cycle.phase == agent_doc_turn::CyclePhase::Committed
                    && cycle.response_sha256.is_some())
        }),
        Err(error) => {
            eprintln!("[queue] could not read the address response cycle: {error:#}");
            None
        }
    };
    let current_open_cycle = cycle
        .as_ref()
        .filter(|cycle| cycle.is_open())
        .map(|cycle| cycle.cycle_id.as_str());
    let claims = match crate::queue_claim::load_ledger_following_read_only(file, content) {
        Ok(claims) => Some(claims),
        Err(error) => {
            eprintln!("[queue] could not read the address claim ledger: {error:#}");
            None
        }
    };
    let mut facts = AddressEffectFacts::default();
    for item in rows {
        let Some(target) = address_review_target(&item) else {
            continue;
        };
        let submitted =
            agent_doc_queue::queue_response::submitted_continuation_prompt(content, &item, |_| {
                false
            });
        let revision = address_prompt_revision(&submitted);
        facts.by_key.entry(revision).or_insert_with(|| {
            if let Some(forced) = &forced {
                return forced.clone();
            }
            if fixture_target(&target) {
                return AddressObservation::Unseen;
            }
            let Some(claims) = claims.as_ref() else {
                return AddressObservation::Unseen;
            };
            let (attempt, fetch) = match claims.resolve_selector(&item) {
                Ok(Some(claim)) if claim.worker_binding_holds_attempt() => {
                    (Some(AddressAttempt::for_claim(claim)), true)
                }
                Ok(None) => {
                    let prior = match inline_observed_cycle(file, &submitted, &target) {
                        Ok(prior) => prior,
                        Err(error) => {
                            eprintln!(
                                "[queue] could not read prior inline address attempt: {error:#}"
                            );
                            return AddressObservation::Unseen;
                        }
                    };
                    let fetch = prior.as_deref().is_some_and(|prior_cycle| {
                        match inline_cycle_allows_after_observation(
                            file,
                            current_open_cycle,
                            prior_cycle,
                        ) {
                            Ok(fetch) => fetch,
                            Err(error) => {
                                eprintln!(
                                    "[queue] could not classify the prior inline address cycle: {error:#}"
                                );
                                false
                            }
                        }
                    });
                    (
                        prior.map(|cycle| AddressAttempt::inline(&cycle, &submitted)),
                        fetch,
                    )
                }
                Ok(Some(_)) => (None, false),
                Err(error) => {
                    eprintln!("[queue] ambiguous address claim: {error:#}");
                    (None, false)
                }
            };
            match attempt {
                Some(attempt) => observe_or_warn(file, &attempt, &target, fetch),
                None => AddressObservation::MissingBaseline,
            }
        });
    }
    (!facts.by_key.is_empty()).then(|| AddressEffectGuard::install(facts))
}

fn observe_or_warn(
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    fetch: bool,
) -> AddressObservation {
    match observe_snapshot(file, attempt, target, |target| {
        fetch.then(|| fetch_snapshot(target)).flatten()
    }) {
        Ok(observation) => observation,
        Err(error) => {
            eprintln!("[queue] could not reconcile the address observation: {error:#}");
            AddressObservation::Unseen
        }
    }
}

fn observe_snapshot(
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    fetch: impl FnOnce(&AddressTarget) -> Option<AddressSnapshot>,
) -> Result<AddressObservation> {
    let before = load_snapshot(file, attempt, target, SnapshotKind::Before)?;
    let proven = load_snapshot(file, attempt, target, SnapshotKind::Proven)?;
    let Some(baseline) = before else {
        anyhow::ensure!(
            proven.is_none(),
            "address proof is orphaned from its before observation"
        );
        return Ok(AddressObservation::MissingBaseline);
    };
    if let Some(current) = proven {
        let observation = AddressObservation::Seen {
            baseline: Some(baseline),
            current,
        };
        anyhow::ensure!(
            matches!(
                judge_address_completion(&observation, None),
                AddressCompletion::Proven(_)
            ),
            "durable address proof contradicts its before observation",
        );
        return Ok(observation);
    }
    let Some(current) = fetch(target) else {
        return Ok(AddressObservation::Unseen);
    };
    let observation = AddressObservation::Seen {
        baseline: Some(baseline),
        current,
    };
    if matches!(
        judge_address_completion(&observation, None),
        AddressCompletion::Proven(_)
    ) && let AddressObservation::Seen { current, .. } = &observation
    {
        save_snapshot(file, attempt, target, SnapshotKind::Proven, current)?;
    }
    Ok(observation)
}

fn fixture_target(target: &AddressTarget) -> bool {
    target.owner == "sample-org" || target.repo.starts_with("sample")
}

fn remote_target(item: &str) -> Option<AddressTarget> {
    if !github_enabled() {
        return None;
    }
    address_review_target(item).filter(|target| !fixture_target(target))
}

fn address_item_texts(content: &str) -> Vec<String> {
    agent_doc_queue::queue_continuation::all_queue_head_texts(content)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| address_review_target(item).is_some())
        .collect()
}

#[derive(Clone, Copy)]
enum SnapshotKind {
    Before,
    Proven,
}

fn state_kind(attempt: &AddressAttempt, target: &AddressTarget, kind: SnapshotKind) -> String {
    let stage = match kind {
        SnapshotKind::Before => "before",
        SnapshotKind::Proven => "proven",
    };
    format!("address_effect_v2:{stage}:{}:{}", attempt.0, target.key())
}

fn load_snapshot(
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    kind: SnapshotKind,
) -> Result<Option<AddressSnapshot>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    if !state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = state_store::open_state_db_read_only(&root)?;
    if !state_store::state_table_exists(&conn, "queue_document_state")? {
        return Ok(None);
    }
    state_store::load_queue_document_state_from_db(
        &conn,
        &agent_doc_hash::path_hash(file)?,
        &state_kind(attempt, target, kind),
    )?
    .map(|record| serde_json::from_str(&record.payload_json).context("parse address observation"))
    .transpose()
}

fn save_snapshot(
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    kind: SnapshotKind,
    snapshot: &AddressSnapshot,
) -> Result<()> {
    let root = agent_doc_fs::find_project_root(file).context("no agent-doc project root")?;
    let conn = state_store::open_state_db(&root)?;
    insert_snapshot(&conn, file, attempt, target, kind, snapshot)
}

fn insert_snapshot(
    conn: &state_store::Connection,
    file: &Path,
    attempt: &AddressAttempt,
    target: &AddressTarget,
    kind: SnapshotKind,
    snapshot: &AddressSnapshot,
) -> Result<()> {
    // The first observation owns this exact attempt and phase. Another
    // process may resume the same attempt, but may not replace that evidence.
    conn.execute(
        "INSERT INTO queue_document_state
         (document_hash, state_kind, canonical_path, payload_json, updated_at_secs)
         VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s', 'now') AS INTEGER))
         ON CONFLICT(document_hash, state_kind) DO NOTHING",
        [
            agent_doc_hash::path_hash(file)?,
            state_kind(attempt, target, kind),
            file.canonicalize()?.to_string_lossy().into_owned(),
            serde_json::to_string(snapshot)?,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct HeadView {
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

#[derive(Debug, Deserialize)]
struct GhComment {
    id: u64,
    #[serde(default)]
    in_reply_to_id: Option<u64>,
    #[serde(default)]
    commit_id: Option<String>,
    #[serde(default)]
    pull_request_review_id: Option<u64>,
}

fn fetch_snapshot(target: &AddressTarget) -> Option<AddressSnapshot> {
    let repo = format!("{}/{}", target.owner, target.repo);
    let number = target.number.to_string();
    let head_json = gh(
        &[
            "pr",
            "view",
            number.as_str(),
            "--repo",
            repo.as_str(),
            "--json",
            "headRefOid",
        ],
        Duration::from_secs(8),
    )?;
    let head: HeadView = serde_json::from_str(&head_json).ok()?;
    let api = format!("repos/{repo}/pulls/{number}/comments");
    let comments_json = gh(&["api", "--paginate", api.as_str()], Duration::from_secs(8))?;
    let comments = parse_comments(&comments_json)?;
    let reply_ids = reply_ids(target, &comments);
    let comment_commit = target.discussion_id.and_then(|id| {
        comments
            .iter()
            .find(|comment| comment.id == id)
            .and_then(|comment| comment.commit_id.clone())
    });
    let review_state = fetch_review_state(target, &head.head_ref_oid).map(Box::new);
    Some(AddressSnapshot {
        head_sha: head.head_ref_oid,
        reply_ids,
        comment_commit,
        thread_named: target.thread_named(),
        review_state,
    })
}

/// `#noopaddress`: one GraphQL read of what still asks for work on the head.
/// Review threads, the latest opinionated reviews, and the head commit's check
/// contexts with their required flag for this pull request.
const REVIEW_STATE_QUERY: &str = "query($owner:String!,$repo:String!,$number:Int!){repository(owner:$owner,name:$repo){pullRequest(number:$number){headRefOid reviewDecision latestOpinionatedReviews(first:100){pageInfo{hasNextPage} nodes{state}} reviewThreads(first:100){pageInfo{hasNextPage} nodes{isResolved}} commits(last:1){nodes{commit{oid statusCheckRollup{contexts(first:100){pageInfo{hasNextPage} nodes{__typename ... on CheckRun{status conclusion isRequired(pullRequestNumber:$number)} ... on StatusContext{state isRequired(pullRequestNumber:$number)}}}}}}}}}}";

fn fetch_review_state(target: &AddressTarget, head: &str) -> Option<AddressReviewState> {
    let owner = format!("owner={}", target.owner);
    let repo = format!("repo={}", target.repo);
    let number = format!("number={}", target.number);
    let query = format!("query={REVIEW_STATE_QUERY}");
    let stdout = gh(
        &[
            "api",
            "graphql",
            "-F",
            owner.as_str(),
            "-F",
            repo.as_str(),
            "-F",
            number.as_str(),
            "-f",
            query.as_str(),
        ],
        Duration::from_secs(8),
    )?;
    parse_review_state(&stdout, head)
}

/// The review state of `head`, or `None` when the read is partial, belongs to
/// another head, or does not parse. A partial read never proves a no-op.
fn parse_review_state(stdout: &str, head: &str) -> Option<AddressReviewState> {
    let root: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let pr = root.pointer("/data/repository/pullRequest")?;
    let same = |sha: &str| !sha.trim().is_empty() && sha.trim().eq_ignore_ascii_case(head.trim());
    if !same(pr.get("headRefOid")?.as_str()?) {
        return None;
    }
    let complete = |connection: &serde_json::Value| {
        connection
            .pointer("/pageInfo/hasNextPage")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
    };
    let threads = pr.get("reviewThreads")?;
    let reviews = pr.get("latestOpinionatedReviews")?;
    if !complete(threads) || !complete(reviews) {
        return None;
    }
    let unresolved_threads = threads
        .get("nodes")?
        .as_array()?
        .iter()
        .filter(|thread| {
            thread
                .get("isResolved")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        })
        .count() as u64;
    let changes_requested = pr.get("reviewDecision").and_then(serde_json::Value::as_str)
        == Some("CHANGES_REQUESTED")
        || reviews.get("nodes")?.as_array()?.iter().any(|review| {
            review.get("state").and_then(serde_json::Value::as_str) == Some("CHANGES_REQUESTED")
        });
    let commit = pr.pointer("/commits/nodes/0/commit")?;
    if !same(commit.get("oid")?.as_str()?) {
        return None;
    }
    let mut failing_required_checks = 0;
    let mut pending_required_checks = 0;
    // A head with no checks reports a null rollup: nothing is required there.
    if let Some(contexts) = commit
        .get("statusCheckRollup")
        .filter(|rollup| !rollup.is_null())
        .map(|rollup| rollup.get("contexts"))
    {
        let contexts = contexts?;
        if !complete(contexts) {
            return None;
        }
        for check in contexts.get("nodes")?.as_array()? {
            if check.get("isRequired").and_then(serde_json::Value::as_bool) != Some(true) {
                continue;
            }
            match required_check_outcome(check) {
                CheckOutcome::Passed => {}
                CheckOutcome::Failed => failing_required_checks += 1,
                CheckOutcome::Pending => pending_required_checks += 1,
            }
        }
    }
    Some(AddressReviewState {
        unresolved_threads,
        changes_requested,
        failing_required_checks,
        pending_required_checks,
    })
}

enum CheckOutcome {
    Passed,
    Failed,
    Pending,
}

fn required_check_outcome(check: &serde_json::Value) -> CheckOutcome {
    let field = |name: &str| check.get(name).and_then(serde_json::Value::as_str);
    match field("__typename") {
        Some("CheckRun") => {
            if field("status") != Some("COMPLETED") {
                return CheckOutcome::Pending;
            }
            match field("conclusion") {
                Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => CheckOutcome::Passed,
                _ => CheckOutcome::Failed,
            }
        }
        Some("StatusContext") => match field("state") {
            Some("SUCCESS") => CheckOutcome::Passed,
            Some("PENDING" | "EXPECTED") => CheckOutcome::Pending,
            _ => CheckOutcome::Failed,
        },
        _ => CheckOutcome::Pending,
    }
}

fn reply_ids(target: &AddressTarget, comments: &[GhComment]) -> Vec<u64> {
    let mut ids: Vec<u64> = comments
        .iter()
        .filter(|comment| match (target.discussion_id, target.review_id) {
            (Some(discussion), _) => comment.in_reply_to_id == Some(discussion),
            (None, Some(review)) => {
                comment.pull_request_review_id == Some(review) && comment.in_reply_to_id.is_some()
            }
            (None, None) => comment.in_reply_to_id.is_some(),
        })
        .map(|comment| comment.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn parse_comments(stdout: &str) -> Option<Vec<GhComment>> {
    if let Ok(comments) = serde_json::from_str::<Vec<GhComment>>(stdout) {
        return Some(comments);
    }
    let mut all = Vec::new();
    for page in serde_json::Deserializer::from_str(stdout).into_iter::<Vec<GhComment>>() {
        all.extend(page.ok()?);
    }
    (!stdout.trim().is_empty()).then_some(all)
}

fn gh(args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    // Read while gh runs. Waiting for exit before draining stdout deadlocks
    // a full pipe on a PR with enough discussion comments.
    let reader = std::thread::spawn(move || {
        let mut stdout = String::new();
        pipe.read_to_string(&mut stdout).map(|_| stdout)
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = reader.join().ok()?.ok()?;
                return status.success().then_some(stdout);
            }
            Ok(None) if started.elapsed() <= timeout => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Ok(None) | Err(_) => {
                if let Err(error) = child.kill() {
                    eprintln!("[queue] could not stop GitHub observation: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("[queue] could not reap GitHub observation: {error}");
                }
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::address_completion_proof::{
        AddressCompletion, AddressProof, judge_address_completion,
    };

    fn snapshot(head: &str, reply_ids: Vec<u64>) -> AddressSnapshot {
        AddressSnapshot {
            head_sha: head.to_string(),
            reply_ids,
            comment_commit: None,
            thread_named: true,
            review_state: None,
        }
    }

    #[test]
    fn inline_hold_fences_new_cycle_and_preserves_other_revisions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "task").unwrap();
        let revision = "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r1";
        let target = address_review_target(revision).unwrap();
        let effect = (revision.to_string(), target.clone());
        let before = snapshot("fa3a3f449986d565f21aeb6e0f792323bc8d1d58", vec![]);
        reserve_inline_execution(&file, "first-cycle", std::slice::from_ref(&effect), |_| {
            Some(before.clone())
        })
        .unwrap();
        let prior = AddressAttempt::inline("first-cycle", revision);
        assert_eq!(
            observe_snapshot(&file, &prior, &target, |_| None).unwrap(),
            AddressObservation::Unseen
        );
        let error = reserve_inline_execution(&file, "replacement-cycle", &[effect], |_| {
            panic!("unknown prior effect must not obtain a fresh baseline")
        })
        .unwrap_err();
        assert!(error.to_string().contains("first-cycle"), "{error:#}");
        assert_eq!(
            load_snapshot(
                &file,
                &AddressAttempt::inline("replacement-cycle", revision),
                &target,
                SnapshotKind::Before,
            )
            .unwrap(),
            None
        );
        let edited = format!("{revision} for a new review request");
        reserve_inline_execution(&file, "replacement-cycle", &[(edited, target)], |_| {
            Some(before.clone())
        })
        .unwrap();
    }

    #[test]
    fn committed_inline_cycle_can_reconcile_after_state_but_unanswered_cycles_cannot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "task").unwrap();
        let conn = state_store::open_state_db(dir.path()).unwrap();
        let document_id = file.canonicalize().unwrap().to_string_lossy().into_owned();
        for (cycle_id, state, response_commit) in [
            ("committed-cycle", "committed", Some("response-commit")),
            ("abandoned-cycle", "abandoned", None),
            ("started-cycle", "preflight_started", None),
        ] {
            conn.execute(
                "INSERT INTO document_cycles
                 (document_id, cycle_id, state, response_commit, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1)",
                (&document_id, cycle_id, state, response_commit),
            )
            .unwrap();
        }

        assert!(inline_cycle_allows_after_observation(&file, None, "committed-cycle").unwrap());
        assert!(!inline_cycle_allows_after_observation(&file, None, "abandoned-cycle").unwrap());
        assert!(!inline_cycle_allows_after_observation(&file, None, "started-cycle").unwrap());
        assert!(
            inline_cycle_allows_after_observation(&file, Some("live-cycle"), "live-cycle").unwrap()
        );
    }

    #[test]
    fn admission_reconciles_committed_inline_effect_before_refusing_redispatch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "task").unwrap();
        let item = "#address https://github.com/sample-org/sample-repo/pull/338 [#q-reconciled]";
        let target = address_review_target(item).unwrap();
        let before = snapshot("12a0c93e", vec![]);
        reserve_inline_execution(
            &file,
            "committed-cycle",
            &[(item.to_string(), target.clone())],
            |_| Some(before.clone()),
        )
        .unwrap();
        let conn = state_store::open_state_db(dir.path()).unwrap();
        let document_id = file.canonicalize().unwrap().to_string_lossy().into_owned();
        conn.execute(
            "INSERT INTO document_cycles
             (document_id, cycle_id, state, response_commit, updated_at)
             VALUES (?1, 'committed-cycle', 'committed', 'response-commit', 1)",
            [&document_id],
        )
        .unwrap();

        let after = snapshot("4c2c1ab7", vec![]);
        let error = reserve_inline_execution(
            &file,
            "replacement-cycle",
            &[(item.to_string(), target.clone())],
            |_| Some(after.clone()),
        )
        .unwrap_err();
        assert!(committed_effect_reconciled(&error));
        assert_eq!(
            reconciled_effect_queue_head_ids(&error),
            Some(vec!["q-reconciled".to_string()]),
        );
        assert!(
            error
                .to_string()
                .contains("reconciled committed prior effect"),
            "{error:#}"
        );
        assert_eq!(
            load_snapshot(
                &file,
                &AddressAttempt::inline("committed-cycle", item),
                &target,
                SnapshotKind::Proven,
            )
            .unwrap(),
            Some(after),
        );
        assert_eq!(
            load_snapshot(
                &file,
                &AddressAttempt::inline("replacement-cycle", item),
                &target,
                SnapshotKind::Before,
            )
            .unwrap(),
            None,
            "reconciliation must not reserve or execute the repeated prompt"
        );
    }

    #[test]
    fn same_target_needs_its_own_before_after_pair_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "task").unwrap();
        let item = "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r1";
        let target = address_review_target(item).unwrap();
        let identity = agent_doc_queue::queue_claim::claim_identity(item);
        let first = AddressAttempt::worker(&identity, "subagent:one", 1, item);
        let second = AddressAttempt::worker(&identity, "subagent:one", 2, item);
        let edited = AddressAttempt::worker(
            &identity,
            "subagent:one",
            1,
            &format!("{item} with new instructions"),
        );
        let inline = AddressAttempt::inline("cycle1", item);
        let later_inline = AddressAttempt::inline("cycle2", item);
        let before = snapshot("fa3a3f449986d565f21aeb6e0f792323bc8d1d58", vec![]);
        let after = snapshot("31b6dd51595a26bc1c7d1c052a1ad8d614a949ff9", vec![23]);

        // A report after restart cannot use a historical reply or invent a baseline.
        assert_eq!(
            observe_snapshot(&file, &first, &target, |_| panic!("no baseline")).unwrap(),
            AddressObservation::MissingBaseline,
        );
        ensure_dispatch_baseline(&file, &first, &target, |_| Some(before.clone())).unwrap();
        ensure_dispatch_baseline(&file, &inline, &target, |_| Some(before.clone())).unwrap();
        ensure_dispatch_baseline(&file, &first, &target, |_| {
            panic!("before already persisted")
        })
        .unwrap();
        assert_eq!(
            observe_snapshot(&file, &first, &target, |_| None).unwrap(),
            AddressObservation::Unseen,
        );
        let observation =
            observe_snapshot(&file, &first, &target, |_| Some(after.clone())).unwrap();
        assert_eq!(
            judge_address_completion(&observation, None),
            AddressCompletion::Proven(AddressProof::HeadChanged {
                from: before.head_sha,
                to: after.head_sha.clone(),
            }),
        );
        assert_eq!(
            observe_snapshot(&file, &first, &target, |_| panic!(
                "proof already persisted"
            ))
            .unwrap(),
            observation,
        );

        for attempt in [&second, &edited, &later_inline] {
            assert_eq!(
                observe_snapshot(&file, attempt, &target, |_| panic!("unrelated baseline"))
                    .unwrap(),
                AddressObservation::MissingBaseline,
            );
            ensure_dispatch_baseline(&file, attempt, &target, |_| Some(after.clone())).unwrap();
            let repeated =
                observe_snapshot(&file, attempt, &target, |_| Some(after.clone())).unwrap();
            assert_eq!(
                judge_address_completion(&repeated, None),
                AddressCompletion::NoEffect {
                    head: after.head_sha.clone()
                },
            );
        }
    }

    #[test]
    fn unrelated_nested_rows_keep_exact_parent_lineage() {
        let content = concat!(
            "<!-- agent:run id=\"main\" subagents -->\n",
            "- ### https://github.com/sample-org/sample-repo/pull/11\n",
            "  - #address\n",
            "- ### https://github.com/sample-org/sample-repo/pull/12\n",
            "  - #address\n",
            "<!-- /agent:run -->\n",
        );
        let targets = address_item_texts(content)
            .iter()
            .filter_map(|item| address_review_target(item))
            .map(|target| target.number)
            .collect::<Vec<_>>();
        assert_eq!(targets, [11, 12]);
    }

    #[test]
    fn newest_operator_reply_changes_revision_but_keeps_its_ancestor_target() {
        let first = format!(
            "https://github.com/sample-org/sample-repo/pull/11: #address\n{} use a separate commit",
            agent_doc_queue::document_queue::LATEST_REPLY_LABEL,
        );
        let second = format!(
            "https://github.com/sample-org/sample-repo/pull/11: #address\n{} investigate https://github.com/sample-org/sample-repo/pull/12#discussion_r1",
            agent_doc_queue::document_queue::LATEST_REPLY_LABEL,
        );
        assert_ne!(revision_hash(&first), revision_hash(&second));
        assert_eq!(
            address_review_target(&first),
            address_review_target(&second)
        );
        assert_eq!(address_review_target(&second).unwrap().number, 11);
    }

    #[test]
    fn reply_ids_follow_the_named_discussion() {
        let target = AddressTarget {
            owner: "sample-org".to_string(),
            repo: "sample-repo".to_string(),
            number: 302,
            discussion_id: Some(10),
            review_id: None,
        };
        let comments = vec![
            GhComment {
                id: 10,
                in_reply_to_id: None,
                commit_id: Some("abc".to_string()),
                pull_request_review_id: None,
            },
            GhComment {
                id: 11,
                in_reply_to_id: Some(10),
                commit_id: None,
                pull_request_review_id: None,
            },
            GhComment {
                id: 12,
                in_reply_to_id: Some(99),
                commit_id: None,
                pull_request_review_id: None,
            },
        ];
        assert_eq!(reply_ids(&target, &comments), vec![11]);
    }

    #[test]
    fn a_fixture_target_is_not_sent_to_the_network() {
        let target = address_review_target(
            "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r1",
        )
        .expect("target");
        assert!(fixture_target(&target));
        assert!(
            remote_target(
                "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r1",
            )
            .is_none()
        );
    }

    fn review_json(threads: &str, decision: &str, reviews: &str, checks: &str) -> String {
        format!(
            r#"{{"data":{{"repository":{{"pullRequest":{{"headRefOid":"a1379d91","reviewDecision":{decision},"latestOpinionatedReviews":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{reviews}]}},"reviewThreads":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{threads}]}},"commits":{{"nodes":[{{"commit":{{"oid":"a1379d91","statusCheckRollup":{checks}}}}}]}}}}}}}}}}"#
        )
    }

    fn rollup(nodes: &str) -> String {
        format!(r#"{{"contexts":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{nodes}]}}}}"#)
    }

    /// `#noopaddress`: the haiven-sdk PR #389 shape (approved, no threads,
    /// optional checks only) reads as nothing to address.
    #[test]
    fn review_state_reads_a_clean_head() {
        let checks = rollup(concat!(
            r#"{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SKIPPED","isRequired":false},"#,
            r#"{"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":null,"isRequired":false},"#,
            r#"{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS","isRequired":true},"#,
            r#"{"__typename":"StatusContext","state":"SUCCESS","isRequired":true}"#,
        ));
        let json = review_json(
            r#"{"isResolved":true}"#,
            r#""APPROVED""#,
            r#"{"state":"APPROVED"}"#,
            &checks,
        );
        let state = parse_review_state(&json, "A1379D91").expect("whole read");
        assert!(state.nothing_to_address(), "{state:?}");
        let no_checks = review_json("", "null", "", "null");
        assert!(
            parse_review_state(&no_checks, "a1379d91")
                .unwrap()
                .nothing_to_address()
        );
    }

    #[test]
    fn review_state_counts_what_still_asks_for_work() {
        let required = |body: &str| rollup(&format!(r#"{{{body},"isRequired":true}}"#));
        let cases = [
            review_json(r#"{"isResolved":false}"#, "null", "", "null"),
            review_json("", r#""CHANGES_REQUESTED""#, "", "null"),
            review_json("", "null", r#"{"state":"CHANGES_REQUESTED"}"#, "null"),
            review_json(
                "",
                "null",
                "",
                &required(r#""__typename":"CheckRun","status":"COMPLETED","conclusion":"FAILURE""#),
            ),
            review_json(
                "",
                "null",
                "",
                &required(r#""__typename":"CheckRun","status":"QUEUED","conclusion":null"#),
            ),
            review_json(
                "",
                "null",
                "",
                &required(r#""__typename":"StatusContext","state":"ERROR""#),
            ),
        ];
        for json in cases {
            let state = parse_review_state(&json, "a1379d91").expect("whole read");
            assert!(!state.nothing_to_address(), "{json}\n{state:?}");
        }
    }

    #[test]
    fn a_partial_or_other_head_review_read_proves_nothing() {
        let clean = review_json("", "null", "", "null");
        assert!(parse_review_state(&clean, "ffffffff").is_none());
        let paged = clean.replacen(r#""hasNextPage":false"#, r#""hasNextPage":true"#, 1);
        assert!(parse_review_state(&paged, "a1379d91").is_none());
        let stale_commit = clean.replace(r#""oid":"a1379d91""#, r#""oid":"0000aaaa""#);
        assert!(parse_review_state(&stale_commit, "a1379d91").is_none());
        assert!(parse_review_state("not json", "a1379d91").is_none());
        assert!(parse_review_state(r#"{"errors":[{}]}"#, "a1379d91").is_none());
    }
}
