//! Durable queue-attempt worktree and release lineage (`#queue-lineage`).
//!
//! Queue Markdown is a projection and worktrees are disposable. This ledger is
//! the durable join between one exact queue identity/generation and the Git
//! objects that carried it through dispatch, integration, and release.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_queue::queue_claim::{QueueClaim, claim_identity};
use agent_doc_queue::reported_closeout::{
    ReportedRepoEvidence, ReportedRepoFacts, SinkContainment,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const QUEUE_LINEAGE_STATE_KIND: &str = "queue_lineage_v1";
pub const QUEUE_LINEAGE_CONTRACT_VERSION: &str = "agent-doc-queue-lineage-v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueLineageLedger {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    #[serde(default)]
    pub attempts: Vec<QueueLineageAttempt>,
}

fn contract_version() -> String {
    QUEUE_LINEAGE_CONTRACT_VERSION.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueLineageAttempt {
    /// Current queue identity. Earlier identities remain in `identity_history`
    /// when a claimed row follows an operator edit.
    pub identity: QueueItemIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identity_history: Vec<QueueItemIdentity>,
    pub item_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_text_history: Vec<String>,
    pub generation: u64,
    pub owner: String,
    #[serde(default)]
    pub events: Vec<QueueLineageEvent>,
}

impl QueueLineageAttempt {
    fn matches_identity(&self, identity: &QueueItemIdentity) -> bool {
        &self.identity == identity || self.identity_history.iter().any(|old| old == identity)
    }

    fn terminal_kind(&self) -> Option<QueueLineageEventKind> {
        self.events.iter().rev().find_map(|event| match event.kind {
            QueueLineageEventKind::Cancelled | QueueLineageEventKind::AdmissionRefused => {
                Some(event.kind)
            }
            QueueLineageEventKind::Released
                if event.release_state == Some(ReleaseState::Published) =>
            {
                Some(event.kind)
            }
            _ => None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueLineageEventKind {
    #[default]
    Created,
    Attached,
    Dirty,
    Committed,
    Pushed,
    Integrated,
    Released,
    Cancelled,
    Retained,
    Removed,
    /// Explicit, durable declaration that this attempt changed no repository,
    /// worktree, or branch (`#reportedcloseoutfacts`). An attempt with no
    /// repository facts is prose-only only when it carries this receipt.
    ProseOnly,
    /// Binary-owned terminal receipt: claim admission was rolled back because
    /// the projector settled without the attempt's marker on its visible row
    /// (`#waitingitemclaimmarker`). The detail names the blocker. Subagent
    /// dispatch holds the identity until an explicit newer claim, so one
    /// deterministic failure never becomes a claim/release loop.
    AdmissionRefused,
}

impl QueueLineageEventKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "created" => Ok(Self::Created),
            "attached" => Ok(Self::Attached),
            "dirty" => Ok(Self::Dirty),
            "committed" => Ok(Self::Committed),
            "pushed" => Ok(Self::Pushed),
            "integrated" => Ok(Self::Integrated),
            "released" => Ok(Self::Released),
            "cancelled" | "canceled" => Ok(Self::Cancelled),
            "retained" => Ok(Self::Retained),
            "removed" => Ok(Self::Removed),
            "prose-only" | "prose_only" => Ok(Self::ProseOnly),
            other => bail!(
                "unknown lineage event {other:?}; expected created|attached|dirty|committed|pushed|integrated|released|cancelled|retained|removed|prose-only"
            ),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Attached => "attached",
            Self::Dirty => "dirty",
            Self::Committed => "committed",
            Self::Pushed => "pushed",
            Self::Integrated => "integrated",
            Self::Released => "released",
            Self::Cancelled => "cancelled",
            Self::Retained => "retained",
            Self::Removed => "removed",
            Self::ProseOnly => "prose_only",
            Self::AdmissionRefused => "admission_refused",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMethod {
    CherryPick,
    Rebase,
    Merge,
    FastForward,
    /// A coordinator integrated the source by hand-resolving a conflict, so
    /// the landed commit's patch id differs from the source's and the
    /// cherry-pick/rebase equivalence cannot prove it. The proof is weaker but
    /// still Git-checked: the integration commit is reachable from the sink
    /// ref, it names the source (its message carries the source SHA, as
    /// `git cherry-pick -x` writes it, or the receipt's explicit `--resolves`
    /// names the same commit), both are single-parent commits, and the
    /// integration touches every file the source touched.
    ConflictResolved,
    /// Legacy receipt shape retained for deserialization only. New receipts
    /// must use a Git-verifiable integration method.
    Other,
}

impl IntegrationMethod {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "cherry_pick" | "cherrypick" => Ok(Self::CherryPick),
            "rebase" => Ok(Self::Rebase),
            "merge" => Ok(Self::Merge),
            "fast_forward" | "ff" => Ok(Self::FastForward),
            "conflict_resolved" | "conflictresolved" | "resolved" => Ok(Self::ConflictResolved),
            "other" => bail!(
                "integration method `other` is not verifiable; expected {INTEGRATION_METHODS}"
            ),
            other => bail!("unknown integration method {other:?}; expected {INTEGRATION_METHODS}"),
        }
    }

    /// The `--method` spelling `lineage-record` accepts.
    pub const fn as_cli_str(self) -> &'static str {
        match self {
            Self::CherryPick => "cherry-pick",
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::FastForward => "fast-forward",
            Self::ConflictResolved => "conflict-resolved",
            Self::Other => "other",
        }
    }
}

/// Every `--method` value `lineage-record` accepts for new evidence.
pub const INTEGRATION_METHODS: &str = "cherry-pick|rebase|merge|fast-forward|conflict-resolved";

/// Shortest abbreviation of the source SHA an integration commit message may
/// use to name it for a `conflict-resolved` receipt. Ten hex digits is what
/// `git log --oneline` prints in this repository, and is long enough that a
/// chance match in prose is not a plausible source of false proof.
const MIN_NAMED_SOURCE_ABBREV: usize = 10;

/// A `cherry-pick`/`rebase` receipt refused because the integration commit's
/// patch id differs from the source's: the integration changed the patch,
/// typically by resolving a conflict. Typed so a caller can recommend the
/// `conflict-resolved` method instead of reporting a generic mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchIdMismatch {
    pub method: IntegrationMethod,
    pub source_sha: String,
    pub integration_sha: String,
    pub source_patch: String,
    pub integration_patch: String,
}

impl std::fmt::Display for PatchIdMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "explicit {:?} mapping has different patch ids: {} != {}. When integrating {} needed a hand-resolved conflict, {} is not patch-equivalent and `--method {}` cannot prove it; record `--method conflict-resolved --resolves {}` instead (the integration commit must be reachable from --sink-ref, name the source, and touch every file the source touched)",
            self.method,
            self.source_patch,
            self.integration_patch,
            self.source_sha,
            self.integration_sha,
            self.method.as_cli_str(),
            self.source_sha,
        )
    }
}

impl std::error::Error for PatchIdMismatch {}

/// Whether `err` (anywhere in its context chain) is a cherry-pick/rebase
/// patch-id refusal.
pub fn is_patch_id_mismatch(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<PatchIdMismatch>().is_some())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseState {
    Planned,
    Tagged,
    Published,
    Failed,
}

impl ReleaseState {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "planned" => Ok(Self::Planned),
            "tagged" => Ok(Self::Tagged),
            "published" => Ok(Self::Published),
            "failed" => Ok(Self::Failed),
            other => {
                bail!("unknown release state {other:?}; expected planned|tagged|published|failed")
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueLineageEvent {
    pub kind: QueueLineageEventKind,
    pub at_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    /// Repository that owns `source_sha`; workers may use a separate clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_repo_path: Option<String>,
    /// Repository containing the designated integration ref and commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sink_repo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_method: Option<IntegrationMethod>,
    /// `conflict-resolved` only: the source commit the integration commit
    /// explicitly resolves, when its message does not name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolves_sha: Option<String>,
    /// Explicit branch/ref that owns the integrated result. The integration
    /// commit must remain reachable from this ref; a side-branch commit alone
    /// is not durable integration proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sink_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_state: Option<ReleaseState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RecordRequest {
    pub kind: QueueLineageEventKind,
    pub at_secs: Option<u64>,
    pub worker_id: Option<String>,
    pub repo_path: Option<PathBuf>,
    pub source_repo_path: Option<PathBuf>,
    pub sink_repo_path: Option<PathBuf>,
    pub worktree_path: Option<PathBuf>,
    pub branch: Option<String>,
    pub base_sha: Option<String>,
    pub source_sha: Option<String>,
    pub pushed_ref: Option<String>,
    pub integration_sha: Option<String>,
    pub integration_method: Option<IntegrationMethod>,
    /// `--resolves`: see [`QueueLineageEvent::resolves_sha`].
    pub resolves_sha: Option<String>,
    pub sink_ref: Option<String>,
    pub release_tag: Option<String>,
    pub release_state: Option<ReleaseState>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueLineageReport {
    pub contract_version: String,
    pub document_path: String,
    pub attempts: Vec<QueueLineageAttemptReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueLineageAttemptReport {
    pub identity: QueueItemIdentity,
    pub item_text: String,
    pub generation: u64,
    pub owner: String,
    pub state: String,
    pub verification: String,
    pub worker_id: Option<String>,
    pub repo_path: Option<String>,
    pub source_repo_path: Option<String>,
    pub sink_repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base_sha: Option<String>,
    pub source_sha: Option<String>,
    pub pushed_ref: Option<String>,
    pub integration_sha: Option<String>,
    pub integration_method: Option<IntegrationMethod>,
    pub sink_ref: Option<String>,
    pub release_tag: Option<String>,
    pub release_state: Option<ReleaseState>,
    pub diagnostics: Vec<String>,
    pub events: Vec<QueueLineageEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DiagnosticLevel {
    Degraded,
    Mismatch,
}

#[derive(Debug, Clone)]
struct Diagnostic {
    level: DiagnosticLevel,
    text: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn state_identity(file: &Path) -> Result<(PathBuf, String, String)> {
    let root = agent_doc_fs::find_project_root(file).ok_or_else(|| {
        anyhow::anyhow!(
            "{} is not inside an agent-doc project (no .agent-doc root); queue lineage needs state.db",
            file.display()
        )
    })?;
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok((root, hash, canonical.to_string_lossy().into_owned()))
}

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<QueueLineageLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        QUEUE_LINEAGE_STATE_KIND,
    )?
    else {
        return Ok(QueueLineageLedger {
            contract_version: contract_version(),
            attempts: Vec::new(),
        });
    };
    let ledger: QueueLineageLedger =
        serde_json::from_str(&record.payload_json).context("parse queue lineage ledger")?;
    if ledger.contract_version != QUEUE_LINEAGE_CONTRACT_VERSION {
        bail!(
            "unsupported queue lineage contract {:?}",
            ledger.contract_version
        );
    }
    Ok(ledger)
}

pub fn load(file: &Path) -> Result<QueueLineageLedger> {
    let (root, document_hash, _) = state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

fn mutate<T>(file: &Path, apply: impl FnOnce(&mut QueueLineageLedger) -> Result<T>) -> Result<T> {
    let (root, document_hash, canonical_path) = state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue lineage transaction")?;
    let result = (|| {
        let mut ledger = load_from_conn(&conn, &document_hash)?;
        let value = apply(&mut ledger)?;
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash,
                state_kind: QUEUE_LINEAGE_STATE_KIND.to_string(),
                canonical_path,
                payload_json: serde_json::to_string(&ledger)
                    .context("serialize queue lineage ledger")?,
                updated_at_secs: now_secs(),
            },
        )?;
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue lineage transaction")?;
            Ok(value)
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

pub fn max_generation(file: &Path, item: &str) -> Result<u64> {
    let identity = claim_identity(item);
    Ok(load(file)?
        .attempts
        .iter()
        .filter(|attempt| attempt.matches_identity(&identity))
        .map(|attempt| attempt.generation)
        .max()
        .unwrap_or(0))
}

/// `#consumedclaimmismatch`: whether durable lineage records this exact
/// attempt (`identity`, `owner`, `generation`) as integrated, with no later
/// cancellation. The integration event was validated against the repository
/// when it was recorded; this read is the cheap terminality check used once a
/// struck row's live claim is gone. Absence or a generation-0 legacy attempt
/// fails closed.
pub fn attempt_recorded_integrated(
    ledger: &QueueLineageLedger,
    identity: &agent_doc_element_queue::QueueItemIdentity,
    owner: &str,
    generation: u64,
) -> bool {
    generation > 0
        && ledger.attempts.iter().any(|attempt| {
            attempt.generation == generation
                && attempt.owner == owner
                && attempt.matches_identity(identity)
                && attempt
                    .events
                    .iter()
                    .rposition(|event| event.kind == QueueLineageEventKind::Integrated)
                    .is_some_and(|integrated| {
                        attempt.events[integrated..].iter().all(|event| {
                            !matches!(
                                event.kind,
                                QueueLineageEventKind::Cancelled
                                    | QueueLineageEventKind::AdmissionRefused
                            )
                        })
                    })
        })
}

/// Whether durable lineage proves this exact attempt never advanced past
/// creation. Absence, ambiguity, a bound worker, or any later event all fail
/// closed so callers cannot turn unknown execution into duplicate work.
pub fn attempt_never_started(ledger: &QueueLineageLedger, claim: &QueueClaim) -> bool {
    claim.state == agent_doc_queue::queue_claim::QueueClaimState::Lost
        && claim.worker.is_none()
        && ledger.attempts.iter().any(|attempt| {
            attempt.generation == claim.generation
                && attempt.matches_identity(&claim.identity)
                && !attempt.events.is_empty()
                && attempt
                    .events
                    .iter()
                    .all(|event| event.kind == QueueLineageEventKind::Created)
        })
}

/// Work identities whose newest durable generation was created but never
/// bound to a worker or recorded any later effect. This remains the retry
/// authority after an explicit no-worker requeue removes the live claim rather
/// than retaining a synthetic `Lost` attempt.
pub fn latest_unstarted_identities(
    ledger: &QueueLineageLedger,
) -> Vec<agent_doc_element_queue::QueueItemIdentity> {
    let newest =
        ledger
            .attempts
            .iter()
            .fold(std::collections::HashMap::new(), |mut newest, attempt| {
                newest
                    .entry(attempt.identity.clone())
                    .and_modify(|generation: &mut u64| {
                        *generation = (*generation).max(attempt.generation)
                    })
                    .or_insert(attempt.generation);
                newest
            });
    ledger
        .attempts
        .iter()
        .filter(|attempt| {
            newest.get(&attempt.identity) == Some(&attempt.generation)
                && !attempt.events.is_empty()
                && attempt
                    .events
                    .iter()
                    .all(|event| event.kind == QueueLineageEventKind::Created)
        })
        .map(|attempt| attempt.identity.clone())
        .collect()
}

/// Terminalize one exact unstarted attempt whose claim admission was rolled
/// back for a deterministic projection blocker (`#waitingitemclaimmarker`).
pub fn record_admission_refused(
    file: &Path,
    claim: &QueueClaim,
    blocker: &str,
) -> Result<QueueLineageAttempt> {
    record_claim(file, claim)?;
    record(
        file,
        &claim.item_text,
        claim.generation,
        RecordRequest {
            kind: QueueLineageEventKind::AdmissionRefused,
            detail: Some(blocker.to_string()),
            ..RecordRequest::default()
        },
    )
}

/// Work identities whose NEWEST durable generation ended in a refused claim
/// admission, with the recorded blocker. Subagent dispatch never re-offers
/// them: the same binary would refuse the same row again, and re-offering it
/// every cycle was the claim/release thrash. An explicit `queue claim` creates
/// a newer generation and lifts the hold.
pub fn admission_refused_identities(
    ledger: &QueueLineageLedger,
) -> Vec<(agent_doc_element_queue::QueueItemIdentity, String)> {
    let mut newest = std::collections::HashMap::new();
    for attempt in &ledger.attempts {
        for identity in std::iter::once(&attempt.identity).chain(&attempt.identity_history) {
            newest
                .entry(identity.clone())
                .and_modify(|generation: &mut u64| {
                    *generation = (*generation).max(attempt.generation)
                })
                .or_insert(attempt.generation);
        }
    }
    ledger
        .attempts
        .iter()
        .filter(|attempt| newest.get(&attempt.identity) == Some(&attempt.generation))
        .filter_map(|attempt| {
            attempt
                .events
                .iter()
                .rev()
                .find(|event| event.kind == QueueLineageEventKind::AdmissionRefused)
                .map(|event| {
                    (
                        attempt.identity.clone(),
                        event
                            .detail
                            .clone()
                            .unwrap_or_else(|| "admission_refused".to_string()),
                    )
                })
        })
        .collect()
}

/// Idempotently establish the durable attempt after `queue claim`.
pub fn record_claim(file: &Path, claim: &QueueClaim) -> Result<QueueLineageAttempt> {
    mutate(file, |ledger| {
        let current_identity = claim.identity.clone();
        let followed_identities = claim
            .followed_from
            .iter()
            .map(|text| claim_identity(text))
            .collect::<Vec<_>>();
        let existing = ledger.attempts.iter_mut().find(|attempt| {
            attempt.generation == claim.generation
                && (attempt.matches_identity(&current_identity)
                    || followed_identities
                        .iter()
                        .any(|identity| attempt.matches_identity(identity)))
        });
        if let Some(attempt) = existing {
            if attempt.owner != claim.owner {
                bail!(
                    "queue lineage owner mismatch for generation {}: stored {:?}, claim {:?}",
                    claim.generation,
                    attempt.owner,
                    claim.owner
                );
            }
            if attempt.identity != current_identity {
                if !attempt.identity_history.contains(&attempt.identity) {
                    attempt.identity_history.push(attempt.identity.clone());
                }
                attempt.identity = current_identity;
            }
            if attempt.item_text != claim.item_text {
                if !attempt.item_text_history.contains(&attempt.item_text) {
                    attempt.item_text_history.push(attempt.item_text.clone());
                }
                attempt.item_text = claim.item_text.clone();
            }
            for text in &claim.followed_from {
                if text != &attempt.item_text && !attempt.item_text_history.contains(text) {
                    attempt.item_text_history.push(text.clone());
                }
            }
            return Ok(attempt.clone());
        }
        if ledger.attempts.iter().any(|attempt| {
            attempt.matches_identity(&current_identity) && attempt.generation == claim.generation
        }) {
            bail!(
                "queue lineage identity/generation collision for {:?} generation {}",
                claim.item_text,
                claim.generation
            );
        }
        let event = QueueLineageEvent {
            kind: QueueLineageEventKind::Created,
            at_secs: claim.claimed_at_secs,
            detail: Some(format!("owner={}", claim.owner)),
            ..QueueLineageEvent::default()
        };
        let attempt = QueueLineageAttempt {
            identity: current_identity,
            identity_history: followed_identities,
            item_text: claim.item_text.clone(),
            item_text_history: claim.followed_from.clone(),
            generation: claim.generation,
            owner: claim.owner.clone(),
            events: vec![event],
        };
        ledger.attempts.push(attempt.clone());
        Ok(attempt)
    })
}

/// Idempotently bind the worker from `queue start`. Worktree details can be
/// enriched later by an explicit `attached` receipt.
pub fn record_started(file: &Path, claim: &QueueClaim) -> Result<QueueLineageAttempt> {
    record_claim(file, claim)?;
    let worker = claim
        .worker
        .as_ref()
        .context("started queue claim has no worker binding")?;
    record(
        file,
        &claim.item_text,
        claim.generation,
        RecordRequest {
            kind: QueueLineageEventKind::Attached,
            at_secs: Some(worker.started_at_secs),
            worker_id: Some(worker.worker_id.clone()),
            ..RecordRequest::default()
        },
    )
}

/// Bridge used by queue-removal cancellation: preserve the exact attempt, then
/// terminalize only that generation. A later re-add receives a newer
/// generation and cannot inherit this receipt.
pub fn record_cancelled_claim(
    file: &Path,
    claim: &QueueClaim,
    cancelled_at_secs: u64,
    detail: Option<String>,
) -> Result<QueueLineageAttempt> {
    record_claim(file, claim)?;
    record(
        file,
        &claim.item_text,
        claim.generation,
        RecordRequest {
            kind: QueueLineageEventKind::Cancelled,
            at_secs: Some(cancelled_at_secs),
            detail,
            ..RecordRequest::default()
        },
    )
}

pub fn record(
    file: &Path,
    item: &str,
    generation: u64,
    request: RecordRequest,
) -> Result<QueueLineageAttempt> {
    let identity = claim_identity(item);
    mutate(file, |ledger| {
        let newest = ledger
            .attempts
            .iter()
            .filter(|attempt| attempt.matches_identity(&identity))
            .map(|attempt| attempt.generation)
            .max()
            .unwrap_or(0);
        let attempt = ledger
            .attempts
            .iter_mut()
            .find(|attempt| {
                attempt.generation == generation && attempt.matches_identity(&identity)
            })
            .with_context(|| {
                format!(
                    "no queue lineage attempt for {item:?} generation {generation}; run `queue claim` first"
                )
            })?;
        let event = event_from_request(request)?;
        if attempt.events.contains(&event) {
            return Ok(attempt.clone());
        }
        let cleanup_only = matches!(
            event.kind,
            QueueLineageEventKind::Retained | QueueLineageEventKind::Removed
        );
        if generation < newest && !cleanup_only {
            bail!("stale queue lineage generation {generation}; current generation is {newest}");
        }
        if let Some(terminal) = attempt.terminal_kind()
            && !cleanup_only
        {
            bail!(
                "queue lineage generation {generation} is terminal ({}); refusing {}",
                terminal.as_str(),
                event.kind.as_str()
            );
        }
        validate_transition(attempt, &event)?;
        verify_event(attempt, &event)?;
        attempt.events.push(event);
        Ok(attempt.clone())
    })
}

/// Record the explicit prose-only declaration for one live claim's attempt
/// (`queue report-complete --prose-only`, `#reportedcloseoutfacts`). The
/// attempt is established or re-keyed from the claim first, so a claim that
/// followed an admission edit still lands on its own attempt.
pub fn record_prose_only(
    file: &Path,
    claim: &QueueClaim,
    detail: &str,
) -> Result<QueueLineageAttempt> {
    record_claim(file, claim)?;
    record(
        file,
        &claim.item_text,
        claim.generation,
        RecordRequest {
            kind: QueueLineageEventKind::ProseOnly,
            worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
            detail: Some(detail.to_string()),
            ..RecordRequest::default()
        },
    )
}

/// [`record`] for an explicit `queue lineage-record`: when a live claim names
/// this `(item, generation)` (by its current text or a text it followed), the
/// attempt is first established or re-keyed from that claim, then the receipt
/// is recorded against the claim's own text. Without this, a claim that
/// followed an admission edit (`[#q-…]`, a publication annotation) left its
/// lineage keyed by the old text and every receipt for the current row failed
/// with "no queue lineage attempt" (sdk-dev 2026-11-04, openquestions).
pub fn record_following_live_claim(
    file: &Path,
    item: &str,
    generation: u64,
    request: RecordRequest,
) -> Result<QueueLineageAttempt> {
    let identity = claim_identity(item);
    let claims = crate::queue_claim::load_ledger(file)?;
    let live = claims.claims.iter().find(|claim| {
        claim.generation == generation
            && (claim.identity == identity
                || claim
                    .followed_from
                    .iter()
                    .any(|text| claim_identity(text) == identity))
    });
    match live {
        Some(claim) => {
            record_claim(file, claim)?;
            record(file, &claim.item_text, generation, request)
        }
        None => record(file, item, generation, request),
    }
}

fn event_from_request(request: RecordRequest) -> Result<QueueLineageEvent> {
    let nonempty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    let event = QueueLineageEvent {
        kind: request.kind,
        at_secs: request.at_secs.unwrap_or_else(now_secs),
        worker_id: nonempty(request.worker_id),
        repo_path: request.repo_path.map(durable_path).transpose()?,
        source_repo_path: request.source_repo_path.map(durable_path).transpose()?,
        sink_repo_path: request.sink_repo_path.map(durable_path).transpose()?,
        worktree_path: request.worktree_path.map(durable_path).transpose()?,
        branch: nonempty(request.branch),
        base_sha: nonempty(request.base_sha),
        source_sha: nonempty(request.source_sha),
        pushed_ref: nonempty(request.pushed_ref),
        integration_sha: nonempty(request.integration_sha),
        integration_method: request.integration_method,
        resolves_sha: nonempty(request.resolves_sha),
        sink_ref: nonempty(request.sink_ref),
        release_tag: nonempty(request.release_tag),
        release_state: request.release_state,
        detail: nonempty(request.detail),
    };
    Ok(event)
}

fn durable_path(path: PathBuf) -> Result<String> {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("resolve current directory for queue lineage path")?
            .join(path)
    };
    let durable = absolute.canonicalize().unwrap_or(absolute);
    Ok(durable.to_string_lossy().into_owned())
}

fn validate_transition(attempt: &QueueLineageAttempt, event: &QueueLineageEvent) -> Result<()> {
    let has = |kind| attempt.events.iter().any(|existing| existing.kind == kind);
    if event.resolves_sha.is_some() && event.kind != QueueLineageEventKind::Integrated {
        bail!(
            "--resolves applies only to an `integrated` lineage event with `--method conflict-resolved`"
        );
    }
    match event.kind {
        QueueLineageEventKind::Created => bail!("created is binary-owned by `queue claim`"),
        QueueLineageEventKind::Attached => {
            if event.worker_id.is_none() {
                bail!("attached lineage event requires --worker-id");
            }
        }
        QueueLineageEventKind::Dirty => {
            if !has(QueueLineageEventKind::Attached) {
                bail!("dirty lineage event requires an attached receipt");
            }
        }
        QueueLineageEventKind::Committed => {
            if event.source_sha.is_none() {
                bail!("committed lineage event requires --source-sha");
            }
        }
        QueueLineageEventKind::Pushed => {
            if event.source_sha.is_none() || event.pushed_ref.is_none() {
                bail!("pushed lineage event requires --source-sha and --pushed-ref");
            }
        }
        QueueLineageEventKind::Integrated => {
            if event.source_sha.is_none()
                || event.integration_sha.is_none()
                || event.integration_method.is_none()
                || event.sink_ref.is_none()
            {
                bail!(
                    "integrated lineage event requires explicit --source-sha, --integration-sha, --method, and designated --sink-ref"
                );
            }
            if event.integration_method == Some(IntegrationMethod::Other) {
                bail!(
                    "integrated lineage event refuses unverifiable method `other`; use {INTEGRATION_METHODS}"
                );
            }
            if event.resolves_sha.is_some()
                && event.integration_method != Some(IntegrationMethod::ConflictResolved)
            {
                bail!(
                    "--resolves names the source of a conflict-resolved integration; it applies only to `--method conflict-resolved`"
                );
            }
        }
        QueueLineageEventKind::Released => {
            if !has(QueueLineageEventKind::Integrated) {
                bail!("released lineage event requires an explicit integrated receipt");
            }
            if event.release_tag.is_none() || event.release_state.is_none() {
                bail!("released lineage event requires --release-tag and --release-state");
            }
            if let Some(previous) = attempt
                .events
                .iter()
                .rev()
                .find(|existing| existing.kind == QueueLineageEventKind::Released)
            {
                if previous.release_tag != event.release_tag {
                    bail!("release tag cannot change within one queue lineage generation");
                }
                let rank = |state| match state {
                    ReleaseState::Planned => Some(0),
                    ReleaseState::Tagged => Some(1),
                    ReleaseState::Published => Some(2),
                    ReleaseState::Failed => None,
                };
                if let (Some(previous), Some(next)) = (
                    previous.release_state.and_then(rank),
                    event.release_state.and_then(rank),
                ) && next < previous
                {
                    bail!("release state cannot regress within one queue lineage generation");
                }
            }
        }
        QueueLineageEventKind::ProseOnly => {
            if event_carries_repo_facts(event) {
                bail!(
                    "prose-only lineage event declares that the attempt changed no repository; it cannot carry repository, worktree, branch, or commit facts"
                );
            }
            if attempt_carries_repo_work(attempt) {
                bail!(
                    "queue lineage generation {} already records repository work; a prose-only declaration cannot erase it. Record its integration instead",
                    attempt.generation
                );
            }
        }
        QueueLineageEventKind::Cancelled
        | QueueLineageEventKind::AdmissionRefused
        | QueueLineageEventKind::Retained
        | QueueLineageEventKind::Removed => {}
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Facts {
    worker_id: Option<String>,
    repo_path: Option<String>,
    source_repo_path: Option<String>,
    sink_repo_path: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base_sha: Option<String>,
    source_sha: Option<String>,
    pushed_ref: Option<String>,
    integration_sha: Option<String>,
    integration_method: Option<IntegrationMethod>,
    resolves_sha: Option<String>,
    sink_ref: Option<String>,
    release_tag: Option<String>,
    release_state: Option<ReleaseState>,
}

fn facts(attempt: &QueueLineageAttempt) -> Facts {
    let mut facts = Facts::default();
    for event in &attempt.events {
        macro_rules! keep {
            ($field:ident) => {
                if event.$field.is_some() {
                    facts.$field = event.$field.clone();
                }
            };
        }
        keep!(worker_id);
        keep!(repo_path);
        keep!(source_repo_path);
        keep!(sink_repo_path);
        keep!(worktree_path);
        keep!(branch);
        keep!(base_sha);
        keep!(source_sha);
        keep!(pushed_ref);
        keep!(integration_sha);
        if event.integration_method.is_some() {
            facts.integration_method = event.integration_method;
            // `--resolves` belongs to the integration edge it was recorded
            // with; a later edge of another method must not inherit it.
            facts.resolves_sha = event.resolves_sha.clone();
        }
        keep!(sink_ref);
        keep!(release_tag);
        if event.release_state.is_some() {
            facts.release_state = event.release_state;
        }
    }
    facts
}

fn verify_event(attempt: &QueueLineageAttempt, event: &QueueLineageEvent) -> Result<()> {
    let prior = facts(attempt);
    let legacy_repo = event
        .repo_path
        .as_deref()
        .or(prior.repo_path.as_deref())
        .map(Path::new);
    let source_repo = event
        .source_repo_path
        .as_deref()
        .or(prior.source_repo_path.as_deref())
        .map(Path::new)
        .or(legacy_repo);
    let sink_repo = event
        .sink_repo_path
        .as_deref()
        .or(prior.sink_repo_path.as_deref())
        .map(Path::new)
        .or(legacy_repo);
    let worktree = event
        .worktree_path
        .as_deref()
        .or(prior.worktree_path.as_deref())
        .map(Path::new);
    match event.kind {
        QueueLineageEventKind::Attached => {
            if let Some(path) = worktree {
                if !path.exists() {
                    bail!("attached worktree does not exist: {}", path.display());
                }
                if let Some(branch) = event.branch.as_deref() {
                    let actual = git_output(path, &["branch", "--show-current"])?;
                    if actual != branch {
                        bail!(
                            "worktree branch mismatch: recorded {branch:?}, Git reports {actual:?}"
                        );
                    }
                }
            }
            if let (Some(repo), Some(base)) = (source_repo, event.base_sha.as_deref()) {
                require_commit(repo, base, "base")?;
            }
        }
        QueueLineageEventKind::Dirty => {
            let path = worktree.context("dirty lineage event needs a recorded --worktree")?;
            let status = git_output(path, &["status", "--porcelain", "--untracked-files=all"])?;
            if status.trim().is_empty() {
                bail!("worktree is clean; refusing a dirty lineage receipt");
            }
        }
        QueueLineageEventKind::Committed | QueueLineageEventKind::Pushed => {
            let repo = source_repo.context(
                "commit/push lineage event needs a recorded --source-repo (or legacy --repo)",
            )?;
            let source = event
                .source_sha
                .as_deref()
                .or(prior.source_sha.as_deref())
                .context("commit/push lineage event needs --source-sha")?;
            require_commit(repo, source, "source")?;
            if let Some(base) = event.base_sha.as_deref().or(prior.base_sha.as_deref()) {
                require_ancestor(repo, base, source, "base is not an ancestor of source")?;
            }
            if event.kind == QueueLineageEventKind::Pushed {
                let pushed_ref = event.pushed_ref.as_deref().context("missing pushed ref")?;
                let resolved =
                    git_output(repo, &["rev-parse", &format!("{pushed_ref}^{{commit}}")])?;
                let source = git_output(repo, &["rev-parse", &format!("{source}^{{commit}}")])?;
                if resolved != source {
                    bail!("pushed ref {pushed_ref:?} resolves to {resolved}, not source {source}");
                }
            }
        }
        QueueLineageEventKind::Integrated => {
            let source_repo = source_repo.context(
                "integrated lineage event needs a recorded --source-repo (or legacy --repo)",
            )?;
            let sink_repo = sink_repo.context(
                "integrated lineage event needs a recorded --sink-repo (or legacy --repo)",
            )?;
            verify_integration(
                source_repo,
                sink_repo,
                event.source_sha.as_deref().context("missing source SHA")?,
                event
                    .integration_sha
                    .as_deref()
                    .context("missing integration SHA")?,
                event
                    .integration_method
                    .context("missing integration method")?,
                event.resolves_sha.as_deref(),
                event
                    .sink_ref
                    .as_deref()
                    .context("missing designated sink ref")?,
            )?;
        }
        QueueLineageEventKind::Released => {
            if matches!(
                event.release_state,
                Some(ReleaseState::Tagged | ReleaseState::Published)
            ) {
                let repo = sink_repo.context(
                    "tagged/published release needs a recorded --sink-repo (or legacy --repo)",
                )?;
                let tag = event
                    .release_tag
                    .as_deref()
                    .context("missing release tag")?;
                let integration = prior
                    .integration_sha
                    .as_deref()
                    .context("release has no integrated SHA")?;
                let tagged = git_output(repo, &["rev-parse", &format!("{tag}^{{commit}}")])?;
                let integration =
                    git_output(repo, &["rev-parse", &format!("{integration}^{{commit}}")])?;
                if tagged != integration {
                    bail!("release tag {tag:?} resolves to {tagged}, not {integration}");
                }
            }
        }
        QueueLineageEventKind::Removed => {
            if let Some(path) = worktree
                && path.exists()
            {
                bail!(
                    "worktree still exists at {}; refusing removed receipt",
                    path.display()
                );
            }
        }
        QueueLineageEventKind::Retained => {
            if let Some(path) = worktree
                && !path.exists()
            {
                bail!(
                    "worktree does not exist at {}; refusing retained receipt",
                    path.display()
                );
            }
        }
        QueueLineageEventKind::Created
        | QueueLineageEventKind::Cancelled
        | QueueLineageEventKind::AdmissionRefused
        | QueueLineageEventKind::ProseOnly => {}
    }
    Ok(())
}

fn require_commit(repo: &Path, sha: &str, label: &str) -> Result<()> {
    git_output(repo, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .with_context(|| format!("{label} commit {sha:?} is unavailable"))?;
    Ok(())
}

fn require_ancestor(repo: &Path, ancestor: &str, descendant: &str, message: &str) -> Result<()> {
    let status = Command::new("git")
        .current_dir(repo)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()
        .context("spawn git merge-base")?;
    if !status.success() {
        bail!("{message}: {ancestor} -> {descendant}");
    }
    Ok(())
}

/// Revalidate one stored integration receipt against the repository and its
/// designated sink as they exist now. A receipt that was valid when recorded
/// can cease to prove integration after a force-move or deleted sink ref, so
/// queue synchronization must not project it as recovered integration evidence
/// without this live check.
pub fn stored_integration_is_verified(attempt: &QueueLineageAttempt, event_index: usize) -> bool {
    let Some(event) = attempt.events.get(event_index) else {
        return false;
    };
    if event.kind != QueueLineageEventKind::Integrated {
        return false;
    }
    let mut prior = attempt.clone();
    prior.events.truncate(event_index);
    validate_transition(&prior, event)
        .and_then(|()| verify_event(&prior, event))
        .is_ok()
}

fn verify_integration(
    source_repo: &Path,
    sink_repo: &Path,
    source: &str,
    integration: &str,
    method: IntegrationMethod,
    resolves: Option<&str>,
    sink_ref: &str,
) -> Result<()> {
    require_commit(source_repo, source, "source")?;
    require_commit(sink_repo, integration, "integration")?;
    match method {
        IntegrationMethod::Merge | IntegrationMethod::FastForward => require_ancestor(
            sink_repo,
            source,
            integration,
            "source is not an ancestor of integration",
        ),
        IntegrationMethod::CherryPick | IntegrationMethod::Rebase => {
            let source_patch = patch_id(source_repo, source)?;
            let integration_patch = patch_id(sink_repo, integration)?;
            if source_patch != integration_patch {
                return Err(PatchIdMismatch {
                    method,
                    source_sha: source.to_string(),
                    integration_sha: integration.to_string(),
                    source_patch,
                    integration_patch,
                }
                .into());
            }
            Ok(())
        }
        IntegrationMethod::ConflictResolved => {
            verify_conflict_resolution(source_repo, sink_repo, source, integration, resolves)
        }
        IntegrationMethod::Other => bail!(
            "integration method `other` is not verifiable; record a cherry-pick, rebase, merge, fast-forward, or conflict-resolved edge"
        ),
    }?;
    let sink_commit = git_output(sink_repo, &["rev-parse", &format!("{sink_ref}^{{commit}}")])
        .with_context(|| format!("designated integration sink ref is unavailable: {sink_ref}"))?;
    require_ancestor(
        sink_repo,
        integration,
        &sink_commit,
        "integration commit is not reachable from the designated sink ref",
    )
}

/// The `conflict-resolved` edge: the integration commit names the source and
/// covers every file it touched. Sink reachability is checked by the caller,
/// as for every method.
fn verify_conflict_resolution(
    source_repo: &Path,
    sink_repo: &Path,
    source: &str,
    integration: &str,
    resolves: Option<&str>,
) -> Result<()> {
    let source_full = git_output(source_repo, &["rev-parse", &format!("{source}^{{commit}}")])?;
    let integration_full = git_output(
        sink_repo,
        &["rev-parse", &format!("{integration}^{{commit}}")],
    )?;
    if source_full == integration_full {
        bail!(
            "conflict-resolved integration {integration} is the source commit itself; record it with `--method fast-forward`"
        );
    }
    require_single_parent(source_repo, &source_full, "source")?;
    require_single_parent(sink_repo, &integration_full, "integration")?;
    match resolves {
        Some(resolves) => {
            let named = git_output(
                source_repo,
                &["rev-parse", &format!("{resolves}^{{commit}}")],
            )
            .with_context(|| format!("--resolves commit {resolves:?} is unavailable"))?;
            if named != source_full {
                bail!(
                    "--resolves {resolves} names {named}, not source commit {source_full}; a conflict-resolved receipt must name its own source"
                );
            }
        }
        None => {
            let message = git_output(sink_repo, &["log", "-1", "--format=%B", &integration_full])?;
            if !message_names_commit(&message, &source_full) {
                bail!(
                    "conflict-resolved integration {integration_full} does not name source commit {source_full}: its message carries no SHA of it (at least {MIN_NAMED_SOURCE_ABBREV} hex digits, e.g. the `(cherry picked from commit …)` line `git cherry-pick -x` writes). Pass `--resolves {source_full}` to name it explicitly"
                );
            }
        }
    }
    let source_files = touched_files(source_repo, &source_full)?;
    if source_files.is_empty() {
        bail!(
            "source commit {source_full} touches no file, so no integration commit can be shown to carry it"
        );
    }
    let integration_files = touched_files(sink_repo, &integration_full)?;
    let uncovered: Vec<&String> = source_files.difference(&integration_files).collect();
    if !uncovered.is_empty() {
        let shown: Vec<&str> = uncovered
            .iter()
            .take(10)
            .map(|path| path.as_str())
            .collect();
        bail!(
            "conflict-resolved integration {integration_full} does not touch {} of the {} file(s) source commit {source_full} touched ({}{}); it cannot carry that change. Integrate the missing paths, or record the commit that does",
            uncovered.len(),
            source_files.len(),
            shown.join(", "),
            if uncovered.len() > shown.len() {
                ", …"
            } else {
                ""
            },
        );
    }
    Ok(())
}

/// Whether a commit message names `full_sha` by its full SHA or by an
/// abbreviation of at least [`MIN_NAMED_SOURCE_ABBREV`] hex digits.
fn message_names_commit(message: &str, full_sha: &str) -> bool {
    let full_sha = full_sha.to_ascii_lowercase();
    message
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|token| token.len() >= MIN_NAMED_SOURCE_ABBREV && token.len() <= full_sha.len())
        .any(|token| full_sha.starts_with(&token.to_ascii_lowercase()))
}

/// A conflict-resolved edge compares one commit's change against its parent;
/// a merge (or root) commit has no single change to compare.
fn require_single_parent(repo: &Path, sha: &str, label: &str) -> Result<()> {
    let parents = git_output(repo, &["rev-list", "--parents", "-n", "1", sha])?;
    let count = parents.split_whitespace().count().saturating_sub(1);
    if count != 1 {
        bail!(
            "conflict-resolved {label} commit {sha} has {count} parents; the edge compares single-parent commits. Record a merge with `--method merge`"
        );
    }
    Ok(())
}

/// Every path a single-parent commit changed, renames split into their delete
/// and add so a path is compared literally.
fn touched_files(repo: &Path, sha: &str) -> Result<BTreeSet<String>> {
    let listing = git_output(
        repo,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--no-renames",
            "-z",
            sha,
        ],
    )?;
    Ok(listing
        .split('\0')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

fn patch_id(repo: &Path, sha: &str) -> Result<String> {
    let diff = Command::new("git")
        .current_dir(repo)
        .args(["show", "--pretty=format:", "--no-ext-diff", sha])
        .output()
        .context("spawn git show for patch-id")?;
    if !diff.status.success() {
        bail!("git show failed for {sha}");
    }
    let mut child = Command::new("git")
        .current_dir(repo)
        .args(["patch-id", "--stable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git patch-id")?;
    child
        .stdin
        .take()
        .context("open git patch-id stdin")?
        .write_all(&diff.stdout)
        .context("write patch to git patch-id")?;
    let output = child.wait_with_output().context("wait for git patch-id")?;
    if !output.status.success() {
        bail!("git patch-id failed for {sha}");
    }
    String::from_utf8(output.stdout)
        .context("git patch-id output is not UTF-8")?
        .split_whitespace()
        .next()
        .map(str::to_string)
        .context("git patch-id returned no id")
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("spawn git {} in {}", args.join(" "), repo.display()))?;
    if !output.status.success() {
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn query(
    file: &Path,
    item: Option<&str>,
    generation: Option<u64>,
) -> Result<QueueLineageReport> {
    let ledger = load(file)?;
    let identity = item.map(claim_identity);
    let mut attempts = ledger
        .attempts
        .iter()
        .filter(|attempt| {
            identity
                .as_ref()
                .is_none_or(|identity| attempt.matches_identity(identity))
                && generation.is_none_or(|generation| attempt.generation == generation)
        })
        .map(report_attempt)
        .collect::<Vec<_>>();
    attempts.sort_by(|a, b| {
        a.item_text
            .cmp(&b.item_text)
            .then(a.generation.cmp(&b.generation))
    });
    Ok(QueueLineageReport {
        contract_version: ledger.contract_version,
        document_path: file
            .canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .to_string_lossy()
            .into_owned(),
        attempts,
    })
}

/// Fail closed when a reported attempt carried repository work but has no
/// mechanically verified `Integrated` receipt (`#subagentintegrationproof`),
/// or when it records neither repository facts nor an explicit prose-only
/// declaration (`#reportedcloseoutfacts`): absence of repository facts is not
/// evidence of prose.
pub fn require_integrated_repo_work(file: &Path, item: &str, generation: u64) -> Result<()> {
    require_integrated_repo_work_inner(file, item, generation, None, true)
}

/// [`require_integrated_repo_work`] for a live claim: its lineage attempt may
/// still be keyed by a text the claim followed (`#claimfollowsedit`, an
/// admitted `[#q-…]` id or a publication annotation), so the claim's own
/// followed texts resolve it, fenced to the claim's owner.
pub fn require_integrated_claim_repo_work(file: &Path, claim: &QueueClaim) -> Result<()> {
    require_integrated_repo_work_inner(
        file,
        &claim.item_text,
        claim.generation,
        Some(ClaimAliases::of(claim)),
        true,
    )
}

/// Apply [`require_integrated_repo_work`] when durable lineage exists for an
/// older released attempt. A legacy prose-only release receipt may predate the
/// lineage ledger entirely; absence therefore remains explicitly prose-only at
/// this compatibility boundary. A recorded attempt does not predate the
/// ledger, so it still needs verified integration proof or an explicit
/// prose-only declaration (`#reportedcloseoutfacts`).
pub fn require_integrated_repo_work_if_recorded(
    file: &Path,
    item: &str,
    generation: u64,
) -> Result<()> {
    require_integrated_repo_work_inner(file, item, generation, None, false)
}

/// Earlier texts of one live claim, fenced to its owner. A followed text's
/// identity is matched only on an attempt the same owner made: generations are
/// per identity, so an unrelated older attempt can share the generation.
#[derive(Clone, Copy)]
struct ClaimAliases<'a> {
    owner: &'a str,
    texts: &'a [String],
}

impl<'a> ClaimAliases<'a> {
    fn of(claim: &'a QueueClaim) -> Self {
        Self {
            owner: &claim.owner,
            texts: &claim.followed_from,
        }
    }
}

/// The lineage attempt a reported `(item, generation)` names.
enum ResolvedReportedAttempt<'a> {
    Missing,
    Ambiguous,
    One(&'a QueueLineageAttempt),
}

fn resolve_reported_attempt<'a>(
    ledger: &'a QueueLineageLedger,
    item: &str,
    generation: u64,
    aliases: Option<ClaimAliases<'_>>,
) -> ResolvedReportedAttempt<'a> {
    let identity = claim_identity(item);
    // Admission can add a binary-owned `[#q-...]` after claim/start recorded
    // an authored nested lineage (`parent: leaf`). That projection is the
    // same attempt, not fresh work. It is a fallback only: two siblings that
    // both followed `<parent>: #address` share that authored identity, and
    // OR-ing it with the post-admission identity made every one of them
    // ambiguous. Exact nest-vector text, then exact identity, wins first.
    let authored_item =
        agent_doc_queue::queue_response::normalized_authored_queue_prompt_text(item);
    let authored_identity = claim_identity(&authored_item);
    let owner = aliases.map(|aliases| aliases.owner);
    let item_key = item.trim();
    let generation_matches = ledger
        .attempts
        .iter()
        .filter(|attempt| attempt.generation == generation)
        .collect::<Vec<_>>();

    if let Some(resolved) = fence_attempt_matches(
        generation_matches
            .iter()
            .copied()
            .filter(|attempt| attempt.item_text.trim() == item_key),
        owner,
    ) {
        return resolved;
    }
    if let Some(resolved) = fence_attempt_matches(
        generation_matches
            .iter()
            .copied()
            .filter(|attempt| attempt.matches_identity(&identity)),
        owner,
    ) {
        return resolved;
    }
    // One-way authored alias. Used only when the exact text and the exact
    // identity name nothing, so a pre-admission claim still resolves after
    // its `[#q-…]` is admitted.
    if authored_identity != identity
        && let Some(resolved) = fence_attempt_matches(
            generation_matches
                .iter()
                .copied()
                .filter(|attempt| attempt.matches_identity(&authored_identity)),
            owner,
        )
    {
        return resolved;
    }
    if let Some(aliases) = aliases {
        let followed = aliases
            .texts
            .iter()
            .map(|text| claim_identity(text))
            .filter(|alias| alias != &identity && alias != &authored_identity)
            .collect::<Vec<_>>();
        if !followed.is_empty()
            && let Some(resolved) = fence_attempt_matches(
                generation_matches.iter().copied().filter(|attempt| {
                    attempt.owner == aliases.owner
                        && followed.iter().any(|alias| attempt.matches_identity(alias))
                }),
                None,
            )
        {
            return resolved;
        }
    }
    ResolvedReportedAttempt::Missing
}

/// `None` when nothing matched, so the caller can try a weaker alias.
/// A known owner that excludes every match is `Missing` and must not fall
/// through onto a sibling.
fn fence_attempt_matches<'a>(
    matches: impl Iterator<Item = &'a QueueLineageAttempt>,
    owner: Option<&str>,
) -> Option<ResolvedReportedAttempt<'a>> {
    let matches = matches.collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }
    let scoped = match owner {
        Some(owner) => matches
            .iter()
            .copied()
            .filter(|attempt| attempt.owner == owner)
            .collect::<Vec<_>>(),
        None => matches,
    };
    if scoped.is_empty() {
        return Some(ResolvedReportedAttempt::Missing);
    }
    if scoped.len() == 1 {
        return Some(ResolvedReportedAttempt::One(scoped[0]));
    }
    Some(ResolvedReportedAttempt::Ambiguous)
}

/// Whether one receipt records repository work.
fn event_carries_repo_facts(event: &QueueLineageEvent) -> bool {
    event.repo_path.is_some()
        || event.source_repo_path.is_some()
        || event.sink_repo_path.is_some()
        || event.worktree_path.is_some()
        || event.branch.is_some()
        || event.base_sha.is_some()
        || event.source_sha.is_some()
        || event.pushed_ref.is_some()
        || event.integration_sha.is_some()
        || event.resolves_sha.is_some()
        || event.sink_ref.is_some()
        || matches!(
            event.kind,
            QueueLineageEventKind::Dirty
                | QueueLineageEventKind::Committed
                | QueueLineageEventKind::Pushed
                | QueueLineageEventKind::Integrated
        )
}

/// Whether an attempt recorded repository work.
fn attempt_carries_repo_work(attempt: &QueueLineageAttempt) -> bool {
    attempt.events.iter().any(event_carries_repo_facts)
}

/// Whether an attempt carries the explicit prose-only declaration. An
/// attempt with no repository facts and no declaration is NOT prose: a started
/// worker that never recorded its commit looks exactly like that
/// (`#reportedcloseoutfacts`, sdk-dev 2026-11-04 c631tsjs).
fn attempt_declares_prose_only(attempt: &QueueLineageAttempt) -> bool {
    attempt
        .events
        .iter()
        .any(|event| event.kind == QueueLineageEventKind::ProseOnly)
}

fn latest_integrated(attempt: &QueueLineageAttempt) -> Option<&QueueLineageEvent> {
    attempt
        .events
        .iter()
        .rev()
        .find(|event| event.kind == QueueLineageEventKind::Integrated)
}

fn reported_repo_facts(attempt: &QueueLineageAttempt) -> ReportedRepoFacts {
    let facts = facts(attempt);
    ReportedRepoFacts {
        source_repo: facts.source_repo_path.or_else(|| facts.repo_path.clone()),
        sink_repo: facts.sink_repo_path.or(facts.repo_path),
        source_sha: facts.source_sha,
        sink_ref: facts.sink_ref,
        pushed_ref: facts.pushed_ref,
    }
}

/// Whether the designated sink ref already reaches the worker's commit, read
/// from the sink repository as it is now.
fn sink_containment(facts: &ReportedRepoFacts) -> Option<SinkContainment> {
    let source = facts.source_sha.as_deref()?;
    let sink_ref = facts.sink_ref.as_deref()?;
    let sink_repo = facts
        .sink_repo
        .as_deref()
        .or(facts.source_repo.as_deref())?;
    let repo = Path::new(sink_repo);
    if !repo.exists() {
        return Some(SinkContainment::Unverifiable {
            reason: format!("sink repository {sink_repo} does not exist"),
        });
    }
    if require_commit(repo, source, "source").is_err() {
        return Some(SinkContainment::SourceAbsentFromSink);
    }
    if git_output(
        repo,
        &["rev-parse", "--verify", &format!("{sink_ref}^{{commit}}")],
    )
    .is_err()
    {
        return Some(SinkContainment::Unverifiable {
            reason: format!("designated sink ref {sink_ref} is unavailable in {sink_repo}"),
        });
    }
    let is_ancestor = |ancestor: &str, descendant: &str| -> Result<bool, String> {
        let status = Command::new("git")
            .current_dir(repo)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| format!("spawn git merge-base in {sink_repo}: {err}"))?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            code => Err(format!("git merge-base exited {code:?} in {sink_repo}")),
        }
    };
    let unverifiable = |reason| Some(SinkContainment::Unverifiable { reason });
    match is_ancestor(source, sink_ref) {
        Ok(true) => return Some(SinkContainment::Contains),
        Ok(false) => {}
        Err(reason) => return unverifiable(reason),
    }
    // `git merge-base A B` exits 1 with no output when the two share no
    // commit: no ancestry-based integration can land the source on that ref.
    let shared = Command::new("git")
        .current_dir(repo)
        .args(["merge-base", source, sink_ref])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match shared.map(|status| status.code()) {
        Ok(Some(0)) => {}
        Ok(Some(1)) => return Some(SinkContainment::NoSharedHistory),
        Ok(code) => return unverifiable(format!("git merge-base exited {code:?} in {sink_repo}")),
        Err(err) => return unverifiable(format!("spawn git merge-base in {sink_repo}: {err}")),
    }
    let fast_forward = match is_ancestor(sink_ref, source) {
        Ok(fast_forward) => fast_forward,
        Err(reason) => return unverifiable(reason),
    };
    Some(SinkContainment::DoesNotContain {
        fast_forward,
        checked_out_worktree: checked_out_worktree(repo, sink_ref),
    })
}

/// The worktree of `repo` that has the local branch `sink_ref` checked out.
/// `git push` refuses to update such a branch, so integration runs there.
/// A remote-tracking ref is never checked out. An unreadable worktree list is
/// reported and treated as "not checked out": the push it leaves in the plan
/// fails safe (a refused ref update changes nothing).
fn checked_out_worktree(repo: &Path, sink_ref: &str) -> Option<String> {
    if sink_ref.starts_with("refs/") && !sink_ref.starts_with("refs/heads/") {
        return None;
    }
    let branch = format!(
        "refs/heads/{}",
        sink_ref.strip_prefix("refs/heads/").unwrap_or(sink_ref)
    );
    let listing = match git_output(repo, &["worktree", "list", "--porcelain"]) {
        Ok(listing) => listing,
        Err(err) => {
            eprintln!(
                "[queue-lineage] WARNING: could not list worktrees of {} to find a checkout of {sink_ref}: {err:#}",
                repo.display()
            );
            return None;
        }
    };
    let mut worktree = None;
    for line in listing.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktree = Some(path.to_string());
        } else if line.strip_prefix("branch ") == Some(branch.as_str()) {
            return worktree;
        }
    }
    None
}

/// Classify one reported attempt's lineage for its coordinator closeout
/// (`#reportedcloseout`), with the same attempt resolution, prose-only rule,
/// and live integration verification [`require_integrated_repo_work`]
/// enforces, so the predicted closeout is the one `queue consume` accepts.
pub fn reported_repo_evidence(
    file: &Path,
    item: &str,
    generation: u64,
) -> Result<ReportedRepoEvidence> {
    Ok(reported_repo_evidence_resolved(
        &load(file)?,
        item,
        generation,
        None,
    ))
}

/// [`reported_repo_evidence`] for a live claim over an already-loaded lineage
/// ledger, so a caller classifying several reported attempts reads state.db
/// once. Resolves through the claim's followed texts exactly as
/// [`require_integrated_claim_repo_work`] does.
pub fn reported_claim_evidence_in(
    ledger: &QueueLineageLedger,
    claim: &QueueClaim,
) -> ReportedRepoEvidence {
    reported_repo_evidence_resolved(
        ledger,
        &claim.item_text,
        claim.generation,
        Some(ClaimAliases::of(claim)),
    )
}

fn reported_repo_evidence_resolved(
    ledger: &QueueLineageLedger,
    item: &str,
    generation: u64,
    aliases: Option<ClaimAliases<'_>>,
) -> ReportedRepoEvidence {
    match resolve_reported_attempt(ledger, item, generation, aliases) {
        ResolvedReportedAttempt::Missing => ReportedRepoEvidence::LineageMissing,
        ResolvedReportedAttempt::Ambiguous => ReportedRepoEvidence::LineageAmbiguous,
        ResolvedReportedAttempt::One(attempt) if !attempt_carries_repo_work(attempt) => {
            if attempt_declares_prose_only(attempt) {
                ReportedRepoEvidence::ProseOnly
            } else {
                ReportedRepoEvidence::Undeclared
            }
        }
        ResolvedReportedAttempt::One(attempt) => {
            let facts = reported_repo_facts(attempt);
            match latest_integrated(attempt) {
                Some(integrated) => match verify_event(attempt, integrated) {
                    Ok(()) => ReportedRepoEvidence::IntegratedVerified,
                    Err(err) => ReportedRepoEvidence::IntegratedUnverified {
                        facts,
                        patch_id_mismatch: is_patch_id_mismatch(&err),
                        reason: format!("{err:#}"),
                    },
                },
                None => {
                    let sink = facts
                        .missing()
                        .is_empty()
                        .then(|| sink_containment(&facts))
                        .flatten();
                    ReportedRepoEvidence::Unintegrated { facts, sink }
                }
            }
        }
    }
}

fn require_integrated_repo_work_inner(
    file: &Path,
    item: &str,
    generation: u64,
    aliases: Option<ClaimAliases<'_>>,
    require_attempt: bool,
) -> Result<()> {
    let ledger = load(file)?;
    let attempt = match resolve_reported_attempt(&ledger, item, generation, aliases) {
        ResolvedReportedAttempt::Ambiguous => bail!(
            "ambiguous queue lineage attempts for {item:?} generation {generation} after resolving its admitted prompt-id projection; keep the reported row live and inspect `agent-doc queue lineage {} --generation {generation}`",
            file.display(),
        ),
        ResolvedReportedAttempt::Missing => {
            if !require_attempt {
                return Ok(());
            }
            return Err(anyhow::anyhow!(
                "no queue lineage attempt for {item:?} generation {generation}, so prose-only versus repository work is unknown; keep the reported row live and record which it was: `agent-doc queue lineage-record {} --item {:?} --generation {generation} --event prose-only` when the worker changed no repository, else `--event committed` with its --source-repo, --source-sha, --sink-repo, and --sink-ref (`agent-doc queue claims` prints both)",
                file.display(),
                item,
            ));
        }
        ResolvedReportedAttempt::One(attempt) => attempt,
    };
    if !attempt_carries_repo_work(attempt) {
        if attempt_declares_prose_only(attempt) {
            return Ok(());
        }
        bail!(
            "reported attempt for {item:?} generation {generation} records neither repository work nor a prose-only declaration; a worker that never recorded its commit looks the same, so the row stays live (#reportedcloseoutfacts). Record which it was: `agent-doc queue lineage-record {} --item {:?} --generation {generation} --event prose-only` when the worker changed no repository, else `--event committed --source-repo <repo> --source-sha <sha> --sink-repo <repo> --sink-ref <ref>` (`agent-doc queue claims` prints both)",
            file.display(),
            item,
        );
    }
    let integrated = latest_integrated(attempt)
        .ok_or_else(|| {
            let prior = facts(attempt);
            let source_repo = prior.source_repo_path.as_deref()
                .or(prior.repo_path.as_deref()).unwrap_or("<source-repo>");
            let sink_repo = prior.sink_repo_path.as_deref()
                .or(prior.repo_path.as_deref()).unwrap_or("<sink-repo>");
            let source = prior.source_sha.as_deref().unwrap_or("<source-sha>");
            anyhow::anyhow!(
                "reported repository work for {item:?} generation {generation} is not integrated. A dirty worktree, commit, or side branch is not terminal. Integrate it into the designated sink, then record proof with `agent-doc queue lineage-record {} --item {:?} --generation {generation} --event integrated --source-repo {:?} --sink-repo {:?} --source-sha {source} --integration-sha <sink-commit-sha> --sink-ref <designated-sink-ref> --method <merge|fast-forward|cherry-pick|rebase|conflict-resolved>` (`conflict-resolved` also takes `--resolves <source-sha>` when the integration commit's message does not name the source); keep the row live until that command succeeds",
                file.display(),
                item,
                source_repo,
                sink_repo,
            )
        })?;
    verify_event(attempt, integrated).with_context(|| {
        format!(
            "recorded integration proof for {item:?} generation {generation} no longer verifies; keep the row live and repair its designated-sink lineage"
        )
    })
}

fn report_attempt(attempt: &QueueLineageAttempt) -> QueueLineageAttemptReport {
    let facts = facts(attempt);
    let diagnostics = diagnose(attempt, &facts);
    let verification = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.level)
        .max()
        .map(|level| match level {
            DiagnosticLevel::Degraded => "degraded",
            DiagnosticLevel::Mismatch => "mismatch",
        })
        .unwrap_or("verified")
        .to_string();
    let state = attempt
        .events
        .last()
        .map(|event| event.kind.as_str())
        .unwrap_or("created")
        .to_string();
    QueueLineageAttemptReport {
        identity: attempt.identity.clone(),
        item_text: attempt.item_text.clone(),
        generation: attempt.generation,
        owner: attempt.owner.clone(),
        state,
        verification,
        worker_id: facts.worker_id,
        repo_path: facts.repo_path,
        source_repo_path: facts.source_repo_path,
        sink_repo_path: facts.sink_repo_path,
        worktree_path: facts.worktree_path,
        branch: facts.branch,
        base_sha: facts.base_sha,
        source_sha: facts.source_sha,
        pushed_ref: facts.pushed_ref,
        integration_sha: facts.integration_sha,
        integration_method: facts.integration_method,
        sink_ref: facts.sink_ref,
        release_tag: facts.release_tag,
        release_state: facts.release_state,
        diagnostics: diagnostics.into_iter().map(|entry| entry.text).collect(),
        events: attempt.events.clone(),
    }
}

fn diagnose(attempt: &QueueLineageAttempt, facts: &Facts) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let removed = attempt
        .events
        .iter()
        .any(|event| event.kind == QueueLineageEventKind::Removed);
    let Some(source_repo_raw) = facts
        .source_repo_path
        .as_deref()
        .or(facts.repo_path.as_deref())
    else {
        // A declared prose-only result has no repository by construction.
        if attempt_declares_prose_only(attempt) && !attempt_carries_repo_work(attempt) {
            return diagnostics;
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Degraded,
            text: "source_repo_unrecorded: record repository work with --source-repo, or declare a prose-only result with --event prose-only".to_string(),
        });
        return diagnostics;
    };
    let source_repo = Path::new(source_repo_raw);
    if !source_repo.exists() {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Degraded,
            text: format!("source_repo_missing: {source_repo_raw}"),
        });
        return diagnostics;
    }
    let sink_repo_raw = facts
        .sink_repo_path
        .as_deref()
        .or(facts.repo_path.as_deref())
        .unwrap_or(source_repo_raw);
    let sink_repo = Path::new(sink_repo_raw);
    if let Some(worktree) = facts.worktree_path.as_deref() {
        let path = Path::new(worktree);
        if !path.exists() && !removed {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Degraded,
                text: format!("worktree_missing_without_removed_receipt: {worktree}"),
            });
        } else if path.exists()
            && let Some(expected) = facts.branch.as_deref()
            && let Ok(actual) = git_output(path, &["branch", "--show-current"])
            && actual != expected
        {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!("branch_mismatch: expected {expected}, found {actual}"),
            });
        }
    }
    for (label, sha) in [
        ("base", facts.base_sha.as_deref()),
        ("source", facts.source_sha.as_deref()),
        ("integration", facts.integration_sha.as_deref()),
    ] {
        if let Some(sha) = sha
            && require_commit(
                if label == "integration" {
                    sink_repo
                } else {
                    source_repo
                },
                sha,
                label,
            )
            .is_err()
        {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!("{label}_commit_missing: {sha}"),
            });
        }
    }
    if let (Some(base), Some(source)) = (facts.base_sha.as_deref(), facts.source_sha.as_deref())
        && require_ancestor(source_repo, base, source, "").is_err()
    {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Mismatch,
            text: format!("base_not_ancestor_of_source: {base} -> {source}"),
        });
    }
    if let (Some(reference), Some(source)) =
        (facts.pushed_ref.as_deref(), facts.source_sha.as_deref())
    {
        match git_output(
            source_repo,
            &["rev-parse", &format!("{reference}^{{commit}}")],
        ) {
            Ok(actual)
                if actual == source
                    || git_output(source_repo, &["rev-parse", source])
                        .ok()
                        .as_deref()
                        == Some(actual.as_str()) => {}
            Ok(actual) => diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!("pushed_ref_drift: {reference} -> {actual}, expected {source}"),
            }),
            Err(_) => diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Degraded,
                text: format!("pushed_ref_unavailable: {reference}"),
            }),
        }
    }
    if let (Some(source), Some(integration), Some(method), Some(sink_ref)) = (
        facts.source_sha.as_deref(),
        facts.integration_sha.as_deref(),
        facts.integration_method,
        facts.sink_ref.as_deref(),
    ) {
        if method == IntegrationMethod::Other {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: "integration_method_other: legacy edge is not integration proof".to_string(),
            });
        } else if verify_integration(
            source_repo,
            sink_repo,
            source,
            integration,
            method,
            facts.resolves_sha.as_deref(),
            sink_ref,
        )
        .is_err()
        {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!(
                    "integration_mapping_mismatch: {source} -> {integration} ({method:?}) sink={sink_ref}"
                ),
            });
        }
    } else if facts.integration_sha.is_some() || facts.integration_method.is_some() {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Mismatch,
            text:
                "integration_sink_missing: legacy integrated receipt is not designated-sink proof"
                    .to_string(),
        });
    }
    if let (Some(tag), Some(integration), Some(ReleaseState::Tagged | ReleaseState::Published)) = (
        facts.release_tag.as_deref(),
        facts.integration_sha.as_deref(),
        facts.release_state,
    ) {
        match git_output(sink_repo, &["rev-parse", &format!("{tag}^{{commit}}")]) {
            Ok(actual)
                if actual == integration
                    || git_output(sink_repo, &["rev-parse", integration])
                        .ok()
                        .as_deref()
                        == Some(actual.as_str()) => {}
            Ok(actual) => diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!("release_tag_drift: {tag} -> {actual}, expected {integration}"),
            }),
            Err(_) => diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Degraded,
                text: format!("release_tag_unavailable: {tag}"),
            }),
        }
    }
    diagnostics
}

pub fn render_text(report: &QueueLineageReport) -> String {
    if report.attempts.is_empty() {
        return format!("{}: no queue lineage receipts", report.document_path);
    }
    report
        .attempts
        .iter()
        .map(|attempt| {
            let mut fields = vec![
                format!("g{}", attempt.generation),
                format!("{}/{}", attempt.state, attempt.verification),
            ];
            if let Some(tag) = attempt.release_tag.as_deref() {
                let state = attempt
                    .release_state
                    .map(|state| format!("{state:?}").to_ascii_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                fields.push(format!("release={tag}/{state}"));
            } else if let Some(integration) = attempt.integration_sha.as_deref() {
                fields.push(format!("integration={}", short_sha(integration)));
                if let Some(sink_ref) = attempt.sink_ref.as_deref() {
                    fields.push(format!("sink={sink_ref}"));
                }
            } else if let Some(source) = attempt.source_sha.as_deref() {
                fields.push(format!("source={}", short_sha(source)));
            } else if let Some(branch) = attempt.branch.as_deref() {
                fields.push(format!("branch={branch}"));
            } else if let Some(worker) = attempt.worker_id.as_deref() {
                fields.push(format!("worker={worker}"));
            }
            if !attempt.diagnostics.is_empty() {
                fields.push(format!(
                    "diagnostics={} (use --verbose)",
                    attempt.diagnostics.len()
                ));
            }
            format!(
                "{} | {}",
                compact_item_text(&attempt.item_text),
                fields.join(" | ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Full evidence and diagnostics for explicit operator inspection.
pub fn render_verbose_text(report: &QueueLineageReport) -> String {
    if report.attempts.is_empty() {
        return format!("{}: no queue lineage receipts", report.document_path);
    }
    let mut out = String::new();
    for attempt in &report.attempts {
        out.push_str(&format!(
            "{} generation {} [{}; {}]\n",
            attempt.item_text, attempt.generation, attempt.state, attempt.verification
        ));
        out.push_str(&format!(
            "  owner={} worker={}\n",
            attempt.owner,
            attempt.worker_id.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!(
            "  repo={} source_repo={} sink_repo={} worktree={} branch={} base={}\n",
            attempt.repo_path.as_deref().unwrap_or("-"),
            attempt.source_repo_path.as_deref().unwrap_or("-"),
            attempt.sink_repo_path.as_deref().unwrap_or("-"),
            attempt.worktree_path.as_deref().unwrap_or("-"),
            attempt.branch.as_deref().unwrap_or("-"),
            attempt.base_sha.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!(
            "  source={} pushed_ref={} integration={} method={} sink_ref={}\n",
            attempt.source_sha.as_deref().unwrap_or("-"),
            attempt.pushed_ref.as_deref().unwrap_or("-"),
            attempt.integration_sha.as_deref().unwrap_or("-"),
            attempt
                .integration_method
                .map(|method| method.as_cli_str().to_string())
                .unwrap_or_else(|| "-".to_string()),
            attempt.sink_ref.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!(
            "  release={} state={}\n",
            attempt.release_tag.as_deref().unwrap_or("-"),
            attempt
                .release_state
                .map(|state| format!("{state:?}").to_ascii_lowercase())
                .unwrap_or_else(|| "-".to_string())
        ));
        for diagnostic in &attempt.diagnostics {
            out.push_str(&format!("  ! {diagnostic}\n"));
        }
    }
    out.trim_end().to_string()
}

fn compact_item_text(value: &str) -> String {
    const MAX_CHARS: usize = 72;
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

/// Compact, non-authoritative Markdown hint suitable for queue projection.
/// The durable ledger and `queue lineage` query remain authoritative.
pub fn markdown_hint(report: &QueueLineageAttemptReport) -> String {
    let mut parts = vec![format!("lineage:g{}", report.generation)];
    if let Some(source) = report.source_sha.as_deref() {
        parts.push(format!("src:{}", short_sha(source)));
    }
    if let Some(integration) = report.integration_sha.as_deref() {
        parts.push(format!("int:{}", short_sha(integration)));
    }
    if let Some(sink_ref) = report.sink_ref.as_deref() {
        parts.push(format!("sink:{sink_ref}"));
    }
    if let Some(tag) = report.release_tag.as_deref() {
        parts.push(format!("release:{tag}"));
    }
    format!("[{}]", parts.join(" "))
}

fn short_sha(value: &str) -> &str {
    value.get(..value.len().min(10)).unwrap_or(value)
}

pub fn identities_for_item(file: &Path, item: &str) -> Result<BTreeSet<u64>> {
    let identity = claim_identity(item);
    Ok(load(file)?
        .attempts
        .into_iter()
        .filter(|attempt| attempt.matches_identity(&identity))
        .map(|attempt| attempt.generation)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_claim::{QueueClaimLedger, QueueClaimState};
    use tempfile::TempDir;

    struct RepoFixture {
        _dir: TempDir,
        root: PathBuf,
        doc: PathBuf,
    }

    impl RepoFixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("repo");
            std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
            git(&root, &["init", "-q"]);
            git(&root, &["config", "user.email", "test@example.com"]);
            git(&root, &["config", "user.name", "Lineage Test"]);
            std::fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
            let doc = root.join("session.md");
            std::fs::write(
                &doc,
                "<!-- agent:queue -->\n- work\n<!-- /agent:queue -->\n",
            )
            .unwrap();
            git(&root, &["add", ".gitignore", "session.md"]);
            git(&root, &["commit", "-qm", "base"]);
            Self {
                _dir: dir,
                root,
                doc,
            }
        }

        fn head(&self) -> String {
            git_output(&self.root, &["rev-parse", "HEAD"]).unwrap()
        }

        fn commit_file(&self, name: &str, content: &str, message: &str) -> String {
            std::fs::write(self.root.join(name), content).unwrap();
            git(&self.root, &["add", name]);
            git(&self.root, &["commit", "-qm", message]);
            self.head()
        }
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_claim(file: &Path, item: &str, owner: &str, worker: Option<&str>) -> QueueClaim {
        let floor = max_generation(file, item).unwrap();
        let mut claims = QueueClaimLedger::default();
        claims.claim_in_session_with_generation_floor(item, owner, 10, 100, None, floor);
        let mut claim = claims.claims.remove(0);
        if let Some(worker) = worker {
            claim.state = QueueClaimState::Running;
            claim.worker = Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
                worker_id: worker.to_string(),
                started_at_secs: 11,
            });
        }
        claim
    }

    fn attach_request(fixture: &RepoFixture, worker: &str, base: &str) -> RecordRequest {
        RecordRequest {
            kind: QueueLineageEventKind::Attached,
            worker_id: Some(worker.to_string()),
            repo_path: Some(fixture.root.clone()),
            worktree_path: Some(fixture.root.clone()),
            branch: Some(git_output(&fixture.root, &["branch", "--show-current"]).unwrap()),
            base_sha: Some(base.to_string()),
            ..RecordRequest::default()
        }
    }

    #[test]
    fn integration_guard_resolves_an_admitted_nested_projection_without_losing_repo_proof() {
        let authored = "parent work: nested prose";
        let projected = "parent work: nested prose [#q-a1b2c3d4e5f6]";

        let prose = RepoFixture::new();
        let claim = make_claim(&prose.doc, authored, "subagent:prose", Some("worker:prose"));
        record_started(&prose.doc, &claim).unwrap();
        // `#reportedcloseoutfacts`: no repository facts is not prose until declared.
        assert!(require_integrated_repo_work(&prose.doc, projected, claim.generation).is_err());
        record_prose_only(&prose.doc, &claim, "nested prose result").unwrap();
        require_integrated_repo_work(&prose.doc, projected, claim.generation).unwrap();

        let repo = RepoFixture::new();
        let base = repo.head();
        let claim = make_claim(&repo.doc, authored, "subagent:repo", Some("worker:repo"));
        record_started(&repo.doc, &claim).unwrap();
        record(
            &repo.doc,
            authored,
            claim.generation,
            attach_request(&repo, "worker:repo", &base),
        )
        .unwrap();
        let source = repo.commit_file("work.txt", "work\n", "worker work");
        record(
            &repo.doc,
            authored,
            claim.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_sha: Some(source),
                ..RecordRequest::default()
            },
        )
        .unwrap();

        let refusal = require_integrated_repo_work(&repo.doc, projected, claim.generation)
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("is not integrated"), "{refusal}");
    }

    /// Two `#address [#q-…]` leaves under one parent, plus generation 1 of the
    /// same `[#q-…]` under a different parent. Authored history collides;
    /// the nest vector and the owner each name one attempt.
    #[test]
    fn shared_address_q_id_resolves_by_nest_vector_then_owner() {
        use agent_doc_queue::queue_claim::claim_identity;

        let fixture = RepoFixture::new();
        let b1 = "https://example.test/merge_requests/2: #address [#q-3f7fdc3da31f]";
        let b2 = "https://example.test/merge_requests/2: #address [#q-096acdc5caca]";
        let a1 = "https://example.test/merge_requests/1: #address [#q-3f7fdc3da31f]";
        let leaf = "#address [#q-3f7fdc3da31f]";
        let authored = agent_doc_queue::queue_response::normalized_authored_queue_prompt_text(b1);
        let authored_identity = claim_identity(&authored);
        assert_eq!(
            authored_identity,
            claim_identity(
                &agent_doc_queue::queue_response::normalized_authored_queue_prompt_text(b2)
            )
        );
        let shared = claim_identity(leaf);

        fn push(
            file: &std::path::Path,
            item_text: &str,
            identity: agent_doc_element_queue::QueueItemIdentity,
            history: Vec<agent_doc_element_queue::QueueItemIdentity>,
            owner: &str,
            prose: bool,
        ) {
            super::mutate(file, |ledger| {
                let mut events = vec![QueueLineageEvent {
                    kind: QueueLineageEventKind::Created,
                    at_secs: 10,
                    detail: Some(format!("owner={owner}")),
                    ..QueueLineageEvent::default()
                }];
                if prose {
                    events.push(QueueLineageEvent {
                        kind: QueueLineageEventKind::ProseOnly,
                        at_secs: 20,
                        detail: Some("addressed".into()),
                        ..QueueLineageEvent::default()
                    });
                }
                ledger.attempts.push(QueueLineageAttempt {
                    identity,
                    identity_history: history,
                    item_text: item_text.to_string(),
                    item_text_history: Vec::new(),
                    generation: 1,
                    owner: owner.to_string(),
                    events,
                });
                Ok(())
            })
            .unwrap();
        }
        push(
            &fixture.doc,
            b1,
            claim_identity(b1),
            vec![authored_identity.clone(), shared.clone()],
            "subagent:addr-b1",
            true,
        );
        push(
            &fixture.doc,
            b2,
            claim_identity(b2),
            vec![authored_identity],
            "subagent:addr-b2",
            false,
        );
        push(
            &fixture.doc,
            a1,
            shared,
            Vec::new(),
            "subagent:addr-a1",
            false,
        );

        require_integrated_repo_work(&fixture.doc, b1, 1).unwrap();
        let b2_err = require_integrated_repo_work(&fixture.doc, b2, 1)
            .unwrap_err()
            .to_string();
        assert!(
            !b2_err.contains("ambiguous"),
            "unique nest vector was ambiguous: {b2_err}"
        );
        assert!(
            b2_err.contains("neither repository work nor a prose-only"),
            "{b2_err}"
        );
        let a_err = require_integrated_repo_work(&fixture.doc, a1, 1)
            .unwrap_err()
            .to_string();
        assert!(!a_err.contains("ambiguous"), "{a_err}");
        assert!(
            a_err.contains("neither repository work nor a prose-only"),
            "{a_err}"
        );

        let mut b1_claim = make_claim(&fixture.doc, leaf, "subagent:addr-b1", None);
        b1_claim.generation = 1;
        require_integrated_claim_repo_work(&fixture.doc, &b1_claim).unwrap();

        let mut a1_claim = make_claim(&fixture.doc, leaf, "subagent:addr-a1", None);
        a1_claim.generation = 1;
        let fenced = require_integrated_claim_repo_work(&fixture.doc, &a1_claim)
            .unwrap_err()
            .to_string();
        assert!(!fenced.contains("ambiguous"), "{fenced}");
        assert!(
            fenced.contains("neither repository work nor a prose-only"),
            "{fenced}"
        );

        let unfenced = require_integrated_repo_work(&fixture.doc, leaf, 1)
            .unwrap_err()
            .to_string();
        assert!(
            unfenced.contains("ambiguous"),
            "shared q-id without an owner fence should stay ambiguous: {unfenced}"
        );
    }

    /// A still-visible unresolvable `#address [#q-…]` row does not reject an
    /// unrelated document change. Striking that row still demands integration
    /// proof for that attempt alone.
    #[test]
    fn unresolvable_nested_row_does_not_block_an_unrelated_successor() {
        use agent_doc_queue::queue_claim::claim_identity;

        let fixture = RepoFixture::new();
        let before = concat!(
            "<!-- agent:queue -->\n",
            "- https://example.test/merge_requests/2\n",
            "  - #address [#q-3f7fdc3da31f]\n",
            "  - #address [#q-096acdc5caca]\n",
            "- do [#other]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&fixture.doc, before).unwrap();
        let heads =
            agent_doc_queue::queue_continuation::all_queue_head_texts(before).expect("queue graph");
        let b1 = heads
            .iter()
            .find(|head| head.contains("[#q-3f7fdc3da31f]"))
            .expect("b1 head")
            .clone();
        let b2 = heads
            .iter()
            .find(|head| head.contains("[#q-096acdc5caca]"))
            .expect("b2 head")
            .clone();
        assert!(
            b1.contains("merge_requests/2") && b2.contains("merge_requests/2"),
            "lineage heads lost their parent: {heads:?}"
        );
        assert_ne!(b1, b2);

        let mut claims = QueueClaimLedger::default();
        for (head, owner) in [(&b1, "subagent:addr-b1"), (&b2, "subagent:addr-b2")] {
            let mut claim = make_claim(&fixture.doc, head, owner, None);
            claim.state = QueueClaimState::Reported;
            claim.generation = 1;
            claims.claims.push(claim);
            super::mutate(&fixture.doc, |ledger| {
                ledger.attempts.push(QueueLineageAttempt {
                    identity: claim_identity(head),
                    identity_history: vec![claim_identity(
                        &agent_doc_queue::queue_response::normalized_authored_queue_prompt_text(
                            head,
                        ),
                    )],
                    item_text: head.clone(),
                    item_text_history: Vec::new(),
                    generation: 1,
                    owner: owner.to_string(),
                    events: vec![QueueLineageEvent {
                        kind: QueueLineageEventKind::Created,
                        at_secs: 10,
                        detail: Some(format!("owner={owner}")),
                        ..QueueLineageEvent::default()
                    }],
                });
                Ok(())
            })
            .unwrap();
        }
        crate::queue_claim::store_ledger_for_test(&fixture.doc, &claims).unwrap();

        let unrelated = before.replacen("- do [#other]\n", "", 1);
        crate::queue_consume::require_integrated_reported_repo_work_for_successor(
            &fixture.doc,
            before,
            &unrelated,
        )
        .expect("a live unresolvable sibling must not block an unrelated change");

        let struck = before.replacen("  - #address [#q-3f7fdc3da31f]\n", "", 1);
        let err = crate::queue_consume::require_integrated_reported_repo_work_for_successor(
            &fixture.doc,
            before,
            &struck,
        )
        .unwrap_err()
        .to_string();
        assert!(
            !err.contains("ambiguous"),
            "striking one row was ambiguous: {err}"
        );
        assert!(
            err.contains("neither repository work nor a prose-only"),
            "{err}"
        );
        assert!(err.contains(&b1), "{err}");
    }

    #[test]
    fn verifies_cherry_pick_across_distinct_git_object_databases() {
        let fixture = RepoFixture::new();
        let base = fixture.head();
        let worker_repo = fixture._dir.path().join("worker-clone");
        git(
            fixture._dir.path(),
            &[
                "clone",
                "-q",
                fixture.root.to_str().unwrap(),
                worker_repo.to_str().unwrap(),
            ],
        );
        git(
            &worker_repo,
            &["config", "user.email", "worker@example.com"],
        );
        git(&worker_repo, &["config", "user.name", "Worker"]);
        std::fs::write(worker_repo.join("result.txt"), "integrated result\n").unwrap();
        git(&worker_repo, &["add", "result.txt"]);
        git(&worker_repo, &["commit", "-qm", "worker result"]);
        let source = git_output(&worker_repo, &["rev-parse", "HEAD"]).unwrap();

        fixture.commit_file("sink-only.txt", "sink\n", "sink advance");
        std::fs::write(fixture.root.join("result.txt"), "integrated result\n").unwrap();
        git(&fixture.root, &["add", "result.txt"]);
        git(&fixture.root, &["commit", "-qm", "integrate worker result"]);
        let integration = fixture.head();
        let sink_ref = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
        assert!(require_commit(&fixture.root, &source, "source").is_err());

        let claim = make_claim(
            &fixture.doc,
            "cross clone work",
            "subagent:cross-clone",
            Some("worker:cross-clone"),
        );
        record_started(&fixture.doc, &claim).unwrap();
        record(
            &fixture.doc,
            &claim.item_text,
            claim.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_repo_path: Some(worker_repo.clone()),
                sink_repo_path: Some(fixture.root.clone()),
                base_sha: Some(base),
                source_sha: Some(source.clone()),
                sink_ref: Some(sink_ref.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        record(
            &fixture.doc,
            &claim.item_text,
            claim.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source),
                integration_sha: Some(integration),
                integration_method: Some(IntegrationMethod::CherryPick),
                sink_ref: Some(sink_ref),
                ..RecordRequest::default()
            },
        )
        .unwrap();

        require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation).unwrap();
        let report = query(&fixture.doc, Some(&claim.item_text), Some(claim.generation)).unwrap();
        assert_eq!(
            report.attempts[0].source_repo_path.as_deref(),
            Some(worker_repo.to_string_lossy().as_ref())
        );
        assert_eq!(
            report.attempts[0].sink_repo_path.as_deref(),
            Some(fixture.root.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn integration_guard_is_scoped_to_one_attempt_not_historical_project_debt() {
        let fixture = RepoFixture::new();
        let repo_claim = make_claim(
            &fixture.doc,
            "old repository work",
            "subagent:old",
            Some("worker:old"),
        );
        record_started(&fixture.doc, &repo_claim).unwrap();
        record(
            &fixture.doc,
            &repo_claim.item_text,
            repo_claim.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_repo_path: Some(fixture.root.clone()),
                sink_repo_path: Some(fixture.root.clone()),
                source_sha: Some(fixture.head()),
                sink_ref: Some("master".to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        assert!(
            require_integrated_repo_work(
                &fixture.doc,
                &repo_claim.item_text,
                repo_claim.generation
            )
            .is_err()
        );

        let prose_claim = make_claim(
            &fixture.doc,
            "current prose work",
            "subagent:current",
            Some("worker:current"),
        );
        record_started(&fixture.doc, &prose_claim).unwrap();
        record_prose_only(&fixture.doc, &prose_claim, "current prose result").unwrap();
        require_integrated_repo_work(&fixture.doc, &prose_claim.item_text, prose_claim.generation)
            .unwrap();
    }

    #[test]
    fn restart_persistence_generation_fence_cancellation_and_outputs() {
        let fixture = RepoFixture::new();
        let first = make_claim(&fixture.doc, "work", "subagent:first", Some("worker:first"));
        record_started(&fixture.doc, &first).unwrap();
        record_cancelled_claim(
            &fixture.doc,
            &first,
            12,
            Some("operator removed active row".to_string()),
        )
        .unwrap();
        let cancelled_update = record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_sha: Some(fixture.head()),
                ..RecordRequest::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(cancelled_update.contains("terminal (cancelled)"));

        // A new process/load still sees generation one, and identical text is
        // allocated generation two after the active claim disappeared.
        assert_eq!(load(&fixture.doc).unwrap().attempts.len(), 1);
        let second = make_claim(
            &fixture.doc,
            "work",
            "subagent:second",
            Some("worker:second"),
        );
        assert_eq!(second.generation, 2);
        record_started(&fixture.doc, &second).unwrap();
        assert!(
            record(
                &fixture.doc,
                "work",
                1,
                RecordRequest {
                    kind: QueueLineageEventKind::Committed,
                    source_sha: Some(fixture.head()),
                    ..RecordRequest::default()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("stale queue lineage generation")
        );

        let report = query(&fixture.doc, None, None).unwrap();
        assert_eq!(report.attempts.len(), 2);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("subagent:first"));
        assert!(json.contains("subagent:second"));
        let text = render_text(&report);
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("work | g1 | cancelled/"));
        assert!(text.contains("work | g2 | attached/"));
        assert!(!text.contains("owner="));
        let verbose = render_verbose_text(&report);
        assert!(verbose.contains("work generation 1 [cancelled"));
        assert!(verbose.contains("work generation 2 [attached"));
        assert!(verbose.contains("owner=subagent:first"));
    }

    #[test]
    fn verifies_real_ancestry_push_release_and_deleted_worktree_diagnostics() {
        let fixture = RepoFixture::new();
        let base = fixture.head();
        let claim = make_claim(&fixture.doc, "work", "subagent:git", Some("worker:git"));
        record_started(&fixture.doc, &claim).unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            attach_request(&fixture, "worker:git", &base),
        )
        .unwrap();
        let source = fixture.commit_file("source.txt", "source\n", "source");
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_sha: Some(source.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        git(
            &fixture.root,
            &["update-ref", "refs/remotes/origin/feature", &source],
        );
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Pushed,
                source_sha: Some(source.clone()),
                pushed_ref: Some("refs/remotes/origin/feature".to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let integration = fixture.commit_file("integration.txt", "integration\n", "integration");
        let sink_ref = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source.clone()),
                integration_sha: Some(integration.clone()),
                integration_method: Some(IntegrationMethod::FastForward),
                sink_ref: Some(sink_ref.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Released,
                release_tag: Some("v1".to_string()),
                release_state: Some(ReleaseState::Planned),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        git(&fixture.root, &["tag", "v1", &integration]);
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Released,
                release_tag: Some("v1".to_string()),
                release_state: Some(ReleaseState::Tagged),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Released,
                release_tag: Some("v1".to_string()),
                release_state: Some(ReleaseState::Published),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let report = query(&fixture.doc, Some("work"), Some(1)).unwrap();
        assert_eq!(report.attempts[0].verification, "verified");
        assert_eq!(
            report.attempts[0].integration_sha.as_deref(),
            Some(integration.as_str())
        );
        assert_eq!(
            report.attempts[0].sink_ref.as_deref(),
            Some(sink_ref.as_str())
        );

        // Worktree disappearance degrades the live check without erasing or
        // invalidating the durable receipt. An explicit removed receipt clears
        // that diagnostic.
        let worktree = fixture._dir.path().join("throwaway-worktree");
        let worktree_arg = worktree.to_string_lossy().into_owned();
        git(
            &fixture.root,
            &["worktree", "add", "-q", &worktree_arg, &integration],
        );
        let cleanup = make_claim(
            &fixture.doc,
            "cleanup",
            "subagent:cleanup",
            Some("worker:cleanup"),
        );
        record_started(&fixture.doc, &cleanup).unwrap();
        record(
            &fixture.doc,
            "cleanup",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Attached,
                worker_id: Some("worker:cleanup".to_string()),
                repo_path: Some(fixture.root.clone()),
                worktree_path: Some(worktree.clone()),
                base_sha: Some(integration.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        git(
            &fixture.root,
            &["worktree", "remove", "--force", &worktree_arg],
        );
        let missing = query(&fixture.doc, Some("cleanup"), Some(1)).unwrap();
        assert!(
            missing.attempts[0]
                .diagnostics
                .iter()
                .any(|d| d.contains("worktree_missing_without_removed_receipt"))
        );
        record(
            &fixture.doc,
            "cleanup",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Removed,
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let removed = query(&fixture.doc, Some("cleanup"), Some(1)).unwrap();
        assert!(
            !removed.attempts[0]
                .diagnostics
                .iter()
                .any(|d| d.contains("worktree_missing"))
        );
    }

    #[test]
    fn explicit_cherry_pick_mapping_uses_patch_equivalence_and_never_inference() {
        let fixture = RepoFixture::new();
        let base = fixture.head();
        let base_branch = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
        git(&fixture.root, &["switch", "-qc", "source"]);
        let source = fixture.commit_file("change.txt", "same patch\n", "source patch");
        git(&fixture.root, &["switch", "-q", &base_branch]);
        fixture.commit_file("main.txt", "diverge\n", "main divergence");
        git(&fixture.root, &["cherry-pick", "--no-edit", &source]);
        let integration = fixture.head();
        assert_ne!(source, integration);

        let claim = make_claim(&fixture.doc, "work", "subagent:pick", Some("worker:pick"));
        record_started(&fixture.doc, &claim).unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            attach_request(&fixture, "worker:pick", &base),
        )
        .unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_sha: Some(source.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source.clone()),
                integration_sha: Some(integration.clone()),
                integration_method: Some(IntegrationMethod::CherryPick),
                sink_ref: Some(base_branch.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let report = query(&fixture.doc, Some("work"), Some(1)).unwrap();
        assert_eq!(
            report.attempts[0].source_sha.as_deref(),
            Some(source.as_str())
        );
        assert_eq!(
            report.attempts[0].integration_sha.as_deref(),
            Some(integration.as_str())
        );
        assert_eq!(
            report.attempts[0].integration_method,
            Some(IntegrationMethod::CherryPick)
        );
        assert_eq!(
            report.attempts[0].sink_ref.as_deref(),
            Some(base_branch.as_str())
        );
    }

    /// A source commit and a coordinator's conflict-resolved port of it onto
    /// a sink branch that changed the same line: the ported commit's patch id
    /// differs from the source's, exactly like 9a4a8d4ea4 -> 443070536d.
    struct ConflictFixture {
        fixture: RepoFixture,
        base: String,
        sink: String,
        source: String,
    }

    impl ConflictFixture {
        fn new() -> Self {
            let fixture = RepoFixture::new();
            fixture.commit_file("shared.txt", "original\n", "shared base");
            let base = fixture.head();
            let sink = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            git(&fixture.root, &["switch", "-qc", "source"]);
            std::fs::write(fixture.root.join("added.txt"), "new file\n").unwrap();
            git(&fixture.root, &["add", "added.txt"]);
            let source = fixture.commit_file("shared.txt", "worker line\n", "worker change");
            git(&fixture.root, &["switch", "-q", &sink]);
            fixture.commit_file("shared.txt", "sink line\n", "sink moved the same line");
            Self {
                fixture,
                base,
                sink,
                source,
            }
        }

        /// Land the source on the checked-out branch with the conflict
        /// resolved by hand: both files, merged content, `message`.
        fn resolve_onto_current(&self, message: &str, include_added: bool) -> String {
            let root = &self.fixture.root;
            std::fs::write(root.join("shared.txt"), "sink line\nworker line\n").unwrap();
            git(root, &["add", "shared.txt"]);
            if include_added {
                std::fs::write(root.join("added.txt"), "new file\n").unwrap();
                git(root, &["add", "added.txt"]);
            }
            git(root, &["commit", "-qm", message]);
            self.fixture.head()
        }

        fn start_attempt(&self) {
            let doc = &self.fixture.doc;
            let claim = make_claim(doc, "work", "subagent:port", Some("worker:port"));
            record_started(doc, &claim).unwrap();
            record(
                doc,
                "work",
                1,
                attach_request(&self.fixture, "worker:port", &self.base),
            )
            .unwrap();
            record(
                doc,
                "work",
                1,
                RecordRequest {
                    kind: QueueLineageEventKind::Committed,
                    source_sha: Some(self.source.clone()),
                    ..RecordRequest::default()
                },
            )
            .unwrap();
        }

        fn integrate(
            &self,
            integration: &str,
            method: IntegrationMethod,
            resolves: Option<&str>,
        ) -> Result<QueueLineageAttempt> {
            record(
                &self.fixture.doc,
                "work",
                1,
                RecordRequest {
                    kind: QueueLineageEventKind::Integrated,
                    source_sha: Some(self.source.clone()),
                    integration_sha: Some(integration.to_string()),
                    integration_method: Some(method),
                    resolves_sha: resolves.map(str::to_string),
                    sink_ref: Some(self.sink.clone()),
                    ..RecordRequest::default()
                },
            )
        }
    }

    /// `#lineageconflictresolved`: a coordinator that resolved a conflict
    /// while integrating a worker commit records honest proof with
    /// `--method conflict-resolved`; cherry-pick and rebase stay strict, and
    /// the new method still refuses an unreachable, unnamed, or non-covering
    /// commit.
    #[test]
    fn conflict_resolved_integration_requires_a_reachable_named_covering_commit() {
        let conflict = ConflictFixture::new();
        let root = conflict.fixture.root.clone();
        let source = conflict.source.clone();
        let sink = conflict.sink.clone();
        conflict.start_attempt();

        // Off the sink: a named, covering port on a side branch.
        git(&root, &["switch", "-qc", "side"]);
        let side = conflict.resolve_onto_current("side port", true);
        git(&root, &["switch", "-q", &sink]);
        // On the sink, but missing one of the source's files.
        git(&root, &["switch", "-qc", "partial"]);
        let partial = conflict.resolve_onto_current("partial port", false);
        git(&root, &["switch", "-q", &sink]);
        // The honest landed commit: reachable, covering, message names nothing.
        let landed = conflict.resolve_onto_current("feat: land the worker change", true);
        git(&root, &["merge", "-q", "--no-edit", "partial"]);
        let sink_head = conflict.fixture.head();

        let strict = conflict
            .integrate(&landed, IntegrationMethod::CherryPick, None)
            .unwrap_err();
        assert!(is_patch_id_mismatch(&strict), "{strict:#}");
        let text = format!("{strict:#}");
        assert!(text.contains("different patch ids"), "{text}");
        assert!(
            text.contains(&format!("--method conflict-resolved --resolves {source}")),
            "the refusal names the honest method: {text}"
        );
        let rebase = conflict
            .integrate(&landed, IntegrationMethod::Rebase, None)
            .unwrap_err();
        assert!(is_patch_id_mismatch(&rebase), "{rebase:#}");
        let resolves_on_pick = conflict
            .integrate(&landed, IntegrationMethod::CherryPick, Some(&source))
            .unwrap_err();
        assert!(
            format!("{resolves_on_pick:#}").contains("only to `--method conflict-resolved`"),
            "{resolves_on_pick:#}"
        );

        let unnamed = conflict
            .integrate(&landed, IntegrationMethod::ConflictResolved, None)
            .unwrap_err();
        assert!(
            format!("{unnamed:#}").contains("does not name source commit"),
            "{unnamed:#}"
        );
        assert!(!is_patch_id_mismatch(&unnamed));
        let wrong_name = conflict
            .integrate(
                &landed,
                IntegrationMethod::ConflictResolved,
                Some(&conflict.base),
            )
            .unwrap_err();
        assert!(
            format!("{wrong_name:#}").contains("must name its own source"),
            "{wrong_name:#}"
        );
        let unreachable = conflict
            .integrate(&side, IntegrationMethod::ConflictResolved, Some(&source))
            .unwrap_err();
        assert!(
            format!("{unreachable:#}").contains("not reachable from the designated sink ref"),
            "{unreachable:#}"
        );
        let uncovered = conflict
            .integrate(&partial, IntegrationMethod::ConflictResolved, Some(&source))
            .unwrap_err();
        assert!(
            format!("{uncovered:#}").contains("added.txt"),
            "{uncovered:#}"
        );
        let merge_commit = conflict
            .integrate(
                &sink_head,
                IntegrationMethod::ConflictResolved,
                Some(&source),
            )
            .unwrap_err();
        assert!(
            format!("{merge_commit:#}").contains("has 2 parents"),
            "{merge_commit:#}"
        );
        let itself = conflict
            .integrate(&source, IntegrationMethod::ConflictResolved, Some(&source))
            .unwrap_err();
        assert!(
            format!("{itself:#}").contains("is the source commit itself"),
            "{itself:#}"
        );
        // No refusal left a receipt behind.
        assert!(
            matches!(
                reported_repo_evidence(&conflict.fixture.doc, "work", 1).unwrap(),
                ReportedRepoEvidence::Unintegrated { .. }
            ),
            "a refused integration records nothing"
        );

        let attempt = conflict
            .integrate(&landed, IntegrationMethod::ConflictResolved, Some(&source))
            .unwrap();
        let integrated = latest_integrated(&attempt).unwrap();
        assert_eq!(integrated.resolves_sha.as_deref(), Some(source.as_str()));
        assert_eq!(
            reported_repo_evidence(&conflict.fixture.doc, "work", 1).unwrap(),
            ReportedRepoEvidence::IntegratedVerified
        );
        require_integrated_repo_work(&conflict.fixture.doc, "work", 1).unwrap();
        let report = query(&conflict.fixture.doc, Some("work"), Some(1)).unwrap();
        assert_eq!(report.attempts[0].verification, "verified", "{report:#?}");
        assert_eq!(
            report.attempts[0].integration_method,
            Some(IntegrationMethod::ConflictResolved)
        );
        assert!(
            render_verbose_text(&report).contains("method=conflict-resolved"),
            "{}",
            render_verbose_text(&report)
        );
    }

    /// The integration commit's own message can name the source in place of
    /// `--resolves`, by full SHA or an abbreviation of at least ten digits.
    #[test]
    fn conflict_resolved_integration_accepts_a_message_that_names_the_source() {
        let conflict = ConflictFixture::new();
        conflict.start_attempt();
        let abbrev = &conflict.source[..MIN_NAMED_SOURCE_ABBREV];
        let landed = conflict.resolve_onto_current(
            &format!("feat: land the worker change\n\nResolves-conflict-of: {abbrev}"),
            true,
        );
        conflict
            .integrate(&landed, IntegrationMethod::ConflictResolved, None)
            .unwrap();
        assert_eq!(
            reported_repo_evidence(&conflict.fixture.doc, "work", 1).unwrap(),
            ReportedRepoEvidence::IntegratedVerified
        );

        let full = conflict.source.clone();
        assert!(message_names_commit(
            &format!("x\n\n(cherry picked from commit {full})"),
            &full
        ));
        assert!(message_names_commit(&full.to_ascii_uppercase(), &full));
        assert!(
            !message_names_commit(
                &format!("see {}", &full[..MIN_NAMED_SOURCE_ABBREV - 1]),
                &full
            ),
            "a short abbreviation is not a name"
        );
        assert!(!message_names_commit(&format!("{full}0"), &full));
        assert!(!message_names_commit("deadbeefdeadbeef", &full));
    }

    #[test]
    fn integration_requires_a_reachable_designated_sink_and_verifiable_method() {
        let fixture = RepoFixture::new();
        let base = fixture.head();
        let sink_ref = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
        git(&fixture.root, &["switch", "-qc", "side-integration"]);
        let source = fixture.commit_file("source.txt", "source\n", "source");
        let integration = fixture.commit_file("integration.txt", "integration\n", "integration");

        let claim = make_claim(
            &fixture.doc,
            "work",
            "subagent:sink-proof",
            Some("worker:sink-proof"),
        );
        record_started(&fixture.doc, &claim).unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            attach_request(&fixture, "worker:sink-proof", &base),
        )
        .unwrap();
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                source_sha: Some(source.clone()),
                ..RecordRequest::default()
            },
        )
        .unwrap();

        let missing_sink = record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source.clone()),
                integration_sha: Some(integration.clone()),
                integration_method: Some(IntegrationMethod::FastForward),
                ..RecordRequest::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            missing_sink.contains("designated --sink-ref"),
            "{missing_sink}"
        );

        let unreachable_sink = record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source),
                integration_sha: Some(integration),
                integration_method: Some(IntegrationMethod::FastForward),
                sink_ref: Some(sink_ref),
                ..RecordRequest::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            unreachable_sink
                .contains("integration commit is not reachable from the designated sink ref"),
            "{unreachable_sink}"
        );

        let source = fixture.commit_file("valid-source.txt", "valid\n", "valid source");
        let integration = fixture.commit_file(
            "valid-integration.txt",
            "valid integration\n",
            "valid integration",
        );
        git(
            &fixture.root,
            &[
                "update-ref",
                "refs/heads/designated-sink",
                integration.as_str(),
            ],
        );
        record(
            &fixture.doc,
            "work",
            1,
            RecordRequest {
                kind: QueueLineageEventKind::Integrated,
                source_sha: Some(source),
                integration_sha: Some(integration),
                integration_method: Some(IntegrationMethod::FastForward),
                sink_ref: Some("refs/heads/designated-sink".to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        let attempt = &load(&fixture.doc).unwrap().attempts[0];
        let integrated_index = attempt
            .events
            .iter()
            .position(|event| event.kind == QueueLineageEventKind::Integrated)
            .unwrap();
        assert!(stored_integration_is_verified(attempt, integrated_index));

        // A force-move of the designated sink invalidates the receipt for
        // recovery/closeout even though both commit objects still exist.
        git(
            &fixture.root,
            &["update-ref", "refs/heads/designated-sink", &base],
        );
        let attempt = &load(&fixture.doc).unwrap().attempts[0];
        assert!(!stored_integration_is_verified(attempt, integrated_index));
        let report = query(&fixture.doc, Some("work"), Some(1)).unwrap();
        assert_eq!(report.attempts[0].verification, "mismatch");
        assert!(
            report.attempts[0]
                .diagnostics
                .iter()
                .any(|entry| entry.contains("integration_mapping_mismatch"))
        );

        let other = IntegrationMethod::parse("other").unwrap_err().to_string();
        assert!(other.contains("not verifiable"), "{other}");
    }

    /// `#reportedcloseout`: each reported-claim shape's lineage classifies to
    /// the closeout `queue consume` will accept, and the emitted
    /// integration-proof command is one the binary's own verification takes.
    mod reported_closeout {
        use super::*;
        use agent_doc_queue::reported_closeout::{
            ReportedCloseoutAction, ReportedCloseoutShape, reported_claim_closeout,
        };

        fn reported(mut claim: QueueClaim) -> QueueClaim {
            claim.state = QueueClaimState::Reported;
            claim
        }

        fn shape(fixture: &RepoFixture, claim: &QueueClaim) -> ReportedCloseoutShape {
            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            reported_claim_closeout("doc.md", claim, &evidence)
                .unwrap()
                .shape
        }

        fn record_commit(fixture: &RepoFixture, claim: &QueueClaim, source: &str, sink_ref: &str) {
            record(
                &fixture.doc,
                &claim.item_text,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::Committed,
                    source_repo_path: Some(fixture.root.clone()),
                    sink_repo_path: Some(fixture.root.clone()),
                    source_sha: Some(source.to_string()),
                    sink_ref: Some(sink_ref.to_string()),
                    ..RecordRequest::default()
                },
            )
            .unwrap();
        }

        #[test]
        fn missing_lineage_attempt_is_named_not_guessed_prose() {
            let fixture = RepoFixture::new();
            let claim = reported(make_claim(
                &fixture.doc,
                "never recorded",
                "subagent:x",
                Some("worker:x"),
            ));
            assert_eq!(
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap(),
                ReportedRepoEvidence::LineageMissing
            );
            assert_eq!(shape(&fixture, &claim), ReportedCloseoutShape::MissingFacts);
        }

        fn declare_prose_only(
            fixture: &RepoFixture,
            claim: &QueueClaim,
        ) -> Result<QueueLineageAttempt> {
            record(
                &fixture.doc,
                &claim.item_text,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::parse("prose-only").unwrap(),
                    ..RecordRequest::default()
                },
            )
        }

        /// `#reportedcloseoutfacts`: a started attempt with no repository
        /// facts is prose only once it carries the explicit declaration.
        /// Before that it is indistinguishable from unrecorded repository
        /// work, and `queue consume`'s gate refuses it.
        #[test]
        fn started_attempt_without_repo_facts_is_missing_facts_until_declared_prose_only() {
            let fixture = RepoFixture::new();
            let claim = reported(make_claim(
                &fixture.doc,
                "review the PR",
                "subagent:review",
                Some("worker:review"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            assert_eq!(
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap(),
                ReportedRepoEvidence::Undeclared
            );
            assert_eq!(shape(&fixture, &claim), ReportedCloseoutShape::MissingFacts);
            let refusal =
                require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation)
                    .unwrap_err()
                    .to_string();
            assert!(
                refusal.contains("neither repository work nor a prose-only declaration"),
                "{refusal}"
            );
            assert!(refusal.contains("--event prose-only"), "{refusal}");

            // Replay the emitted declaration step's exact event.
            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            let closeout = reported_claim_closeout("doc.md", &claim, &evidence).unwrap();
            let declare = closeout
                .steps
                .iter()
                .find(|step| step.action == ReportedCloseoutAction::DeclareProseOnly)
                .unwrap();
            assert_eq!(declare.argv.last().map(String::as_str), Some("prose-only"));
            declare_prose_only(&fixture, &claim).unwrap();

            assert_eq!(
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap(),
                ReportedRepoEvidence::ProseOnly
            );
            assert_eq!(shape(&fixture, &claim), ReportedCloseoutShape::ProseConsume);
            require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            let report =
                query(&fixture.doc, Some(&claim.item_text), Some(claim.generation)).unwrap();
            assert_eq!(report.attempts[0].state, "prose_only");
            assert_eq!(
                report.attempts[0].verification, "verified",
                "{:?}",
                report.attempts[0].diagnostics
            );
        }

        #[test]
        fn prose_only_declaration_cannot_erase_recorded_repository_work() {
            let fixture = RepoFixture::new();
            let claim = reported(make_claim(
                &fixture.doc,
                "fix it",
                "subagent:fix",
                Some("worker:fix"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            let branch = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            let source = fixture.commit_file("fix.txt", "fix\n", "worker fix");
            record_commit(&fixture, &claim, &source, &branch);
            let refusal = declare_prose_only(&fixture, &claim)
                .unwrap_err()
                .to_string();
            assert!(
                refusal.contains("already records repository work"),
                "{refusal}"
            );
            assert!(
                require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation)
                    .is_err()
            );

            let carrying = record(
                &fixture.doc,
                &claim.item_text,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::ProseOnly,
                    source_sha: Some(source),
                    ..RecordRequest::default()
                },
            )
            .unwrap_err()
            .to_string();
            assert!(carrying.contains("cannot carry repository"), "{carrying}");
        }

        /// The c631tsjs shape over a real repository: the worker committed on
        /// a side branch, but lineage only has its `attached` receipt. The
        /// closeout must not be prose; recording the work it names turns it
        /// into an ordinary integration closeout.
        #[test]
        fn unrecorded_side_branch_commit_is_never_consumed_as_prose() {
            let fixture = RepoFixture::new();
            let main = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            let claim = reported(make_claim(
                &fixture.doc,
                "convert ts to js",
                "subagent:c631tsjs",
                Some("worker:tsjs"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            git(&fixture.root, &["checkout", "-qb", "feat/ts-to-js-dts"]);
            let source = fixture.commit_file("sdk.js", "js\n", "convert ts to js");
            git(&fixture.root, &["checkout", "-q", &main]);

            let closeout = reported_claim_closeout(
                "doc.md",
                &claim,
                &reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap(),
            )
            .unwrap();
            assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
            assert_eq!(closeout.missing_facts, ["repo_work_or_prose_declaration"]);
            assert!(
                closeout.next_action_argv().is_none(),
                "nothing may strike the row first"
            );
            assert!(
                require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation)
                    .is_err()
            );

            // The repository_work branch, with its placeholders filled.
            let step = closeout
                .steps
                .iter()
                .find(|step| step.action == ReportedCloseoutAction::RecordRepositoryWork)
                .unwrap();
            assert_eq!(step.alternative.as_deref(), Some("repository_work"));
            assert!(
                step.argv
                    .windows(2)
                    .any(|pair| pair == ["--event", "committed"])
            );
            record_commit(&fixture, &claim, &source, &main);
            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            assert!(
                matches!(
                    &evidence,
                    ReportedRepoEvidence::Unintegrated {
                        sink: Some(SinkContainment::DoesNotContain {
                            fast_forward: true,
                            ..
                        }),
                        ..
                    }
                ),
                "{evidence:?}"
            );
            assert!(
                require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation)
                    .is_err()
            );
        }

        /// The openquestions shape: the claim followed an admission edit
        /// (`[#q-…]` and a publication annotation were added to its row), but
        /// lineage still keys the attempt by the original text. The claim's
        /// followed texts resolve it; `lineage-record` re-keys it.
        #[test]
        fn claim_that_followed_an_admission_edit_resolves_its_original_lineage() {
            let fixture = RepoFixture::new();
            let original = "Draft the open questions in a md file.";
            let mut claim = reported(make_claim(
                &fixture.doc,
                original,
                "subagent:openquestions",
                Some("worker:oq"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            let current = "Draft the open questions in a md file.  [#q-21230b62be31]⟦local⟧";
            claim.identity = claim_identity(current);
            claim.item_text = current.to_string();
            claim.followed_from = vec![original.to_string(), current.to_string()];
            let ledger = load(&fixture.doc).unwrap();

            assert_eq!(
                reported_repo_evidence(&fixture.doc, current, claim.generation).unwrap(),
                ReportedRepoEvidence::LineageMissing,
                "the current text alone does not name the attempt"
            );
            assert_eq!(
                reported_claim_evidence_in(&ledger, &claim),
                ReportedRepoEvidence::Undeclared
            );
            // A followed text is honoured only for the same owner.
            let mut stranger = claim.clone();
            stranger.owner = "subagent:other".to_string();
            assert_eq!(
                reported_claim_evidence_in(&ledger, &stranger),
                ReportedRepoEvidence::LineageMissing
            );

            // `lineage-record` with the row's current text re-keys the attempt
            // from the live claim instead of failing "no queue lineage attempt".
            assert!(declare_prose_only(&fixture, &claim).is_err());
            let mut claims = agent_doc_queue::queue_claim::QueueClaimLedger::default();
            claims.claims.push(claim.clone());
            crate::queue_claim::store_ledger_for_test(&fixture.doc, &claims).unwrap();
            let attempt = record_following_live_claim(
                &fixture.doc,
                current,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::ProseOnly,
                    ..RecordRequest::default()
                },
            )
            .unwrap();
            assert_eq!(attempt.item_text, current);
            assert_eq!(
                reported_repo_evidence(&fixture.doc, current, claim.generation).unwrap(),
                ReportedRepoEvidence::ProseOnly
            );
            require_integrated_claim_repo_work(&fixture.doc, &claim).unwrap();
        }

        /// `lineage-record` for an attempt with no lineage at all establishes
        /// it from the live claim, so the missing-lineage branch is runnable.
        #[test]
        fn missing_lineage_is_established_from_the_live_claim() {
            let fixture = RepoFixture::new();
            let claim = reported(make_claim(
                &fixture.doc,
                "never recorded",
                "subagent:x",
                Some("worker:x"),
            ));
            let mut claims = agent_doc_queue::queue_claim::QueueClaimLedger::default();
            claims.claims.push(claim.clone());
            crate::queue_claim::store_ledger_for_test(&fixture.doc, &claims).unwrap();
            record_following_live_claim(
                &fixture.doc,
                &claim.item_text,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::ProseOnly,
                    ..RecordRequest::default()
                },
            )
            .unwrap();
            assert_eq!(shape(&fixture, &claim), ReportedCloseoutShape::ProseConsume);
        }

        /// c106telem's refused push: the sink moved past the worker's base.
        /// The emitted cherry-pick branch runs in the checked-out sink
        /// worktree and its record step is the one the binary verifies.
        #[test]
        fn non_fast_forward_sink_closes_through_the_emitted_cherry_pick_branch() {
            let fixture = RepoFixture::new();
            let main = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            git(&fixture.root, &["checkout", "-qb", "side"]);
            let source = fixture.commit_file("side.txt", "side\n", "side work");
            git(&fixture.root, &["checkout", "-q", &main]);
            fixture.commit_file("main.txt", "main moved\n", "main moves on");
            let claim = reported(make_claim(
                &fixture.doc,
                "side work",
                "subagent:side",
                Some("worker:side"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            record_commit(&fixture, &claim, &source, &main);

            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            let worktree = fixture.root.canonicalize().unwrap();
            match &evidence {
                ReportedRepoEvidence::Unintegrated {
                    sink:
                        Some(SinkContainment::DoesNotContain {
                            fast_forward: false,
                            checked_out_worktree: Some(path),
                        }),
                    ..
                } => assert_eq!(Path::new(path).canonicalize().unwrap(), worktree),
                other => panic!("{other:?}"),
            }
            let closeout = reported_claim_closeout("doc.md", &claim, &evidence).unwrap();
            assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationRequired);
            assert!(
                closeout
                    .steps
                    .iter()
                    .all(|step| step.action != ReportedCloseoutAction::Integrate)
            );
            let cherry = closeout
                .steps
                .iter()
                .find(|step| step.action == ReportedCloseoutAction::CherryPickOntoSink)
                .unwrap();
            assert!(cherry.concrete, "{cherry:?}");
            let status = Command::new(&cherry.argv[0])
                .args(&cherry.argv[1..])
                .status()
                .unwrap();
            assert!(status.success(), "{:?}", cherry.argv);

            let record_step = closeout
                .steps
                .iter()
                .find(|step| {
                    step.action == ReportedCloseoutAction::RecordIntegration
                        && step.alternative.as_deref() == Some("cherry_pick")
                })
                .unwrap();
            let flag = |name: &str| {
                let index = record_step.argv.iter().position(|arg| arg == name).unwrap();
                record_step.argv[index + 1].clone()
            };
            assert_eq!(flag("--integration-sha"), "<sink-commit-sha>");
            record(
                &fixture.doc,
                &flag("--item"),
                flag("--generation").parse().unwrap(),
                RecordRequest {
                    kind: QueueLineageEventKind::Integrated,
                    source_repo_path: Some(PathBuf::from(flag("--source-repo"))),
                    sink_repo_path: Some(PathBuf::from(flag("--sink-repo"))),
                    source_sha: Some(flag("--source-sha")),
                    integration_sha: Some(fixture.head()),
                    integration_method: Some(IntegrationMethod::parse(&flag("--method")).unwrap()),
                    sink_ref: Some(flag("--sink-ref")),
                    ..RecordRequest::default()
                },
            )
            .unwrap();
            require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            assert_eq!(
                shape(&fixture, &claim),
                ReportedCloseoutShape::IntegratedConsume
            );
        }

        #[test]
        fn source_with_no_history_in_common_with_the_sink_is_a_sink_mismatch() {
            let fixture = RepoFixture::new();
            let main = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            git(&fixture.root, &["checkout", "-q", "--orphan", "contracts"]);
            git(&fixture.root, &["rm", "-rq", "--cached", "."]);
            let source = fixture.commit_file("contracts.txt", "pin\n", "contracts artifact");
            git(&fixture.root, &["checkout", "-qf", &main]);
            let claim = reported(make_claim(
                &fixture.doc,
                "publish telemetry",
                "subagent:c106telem",
                Some("worker:t"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            record_commit(&fixture, &claim, &source, &main);
            assert_eq!(
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap(),
                ReportedRepoEvidence::Unintegrated {
                    facts: ReportedRepoFacts {
                        source_repo: Some(
                            fixture.root.canonicalize().unwrap().display().to_string()
                        ),
                        sink_repo: Some(fixture.root.canonicalize().unwrap().display().to_string()),
                        source_sha: Some(source.clone()),
                        sink_ref: Some(main.clone()),
                        pushed_ref: None,
                    },
                    sink: Some(SinkContainment::NoSharedHistory),
                }
            );
            assert_eq!(shape(&fixture, &claim), ReportedCloseoutShape::SinkMismatch);
        }

        #[test]
        fn repo_work_contained_in_the_sink_emits_a_record_step_the_binary_accepts() {
            let fixture = RepoFixture::new();
            let branch = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            let claim = reported(make_claim(
                &fixture.doc,
                "fix the bug",
                "subagent:fix",
                Some("worker:fix"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            let source = fixture.commit_file("fix.txt", "fix\n", "worker fix");
            record_commit(&fixture, &claim, &source, &branch);

            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            assert!(
                matches!(
                    &evidence,
                    ReportedRepoEvidence::Unintegrated {
                        sink: Some(SinkContainment::Contains),
                        ..
                    }
                ),
                "{evidence:?}"
            );
            let closeout = reported_claim_closeout("doc.md", &claim, &evidence).unwrap();
            assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationProof);
            assert!(
                require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation)
                    .is_err()
            );

            // Run the verify step's exact argv, then apply the record step's
            // exact facts through the binary's own verified record path.
            let verify = &closeout.steps[0];
            assert_eq!(verify.action, ReportedCloseoutAction::VerifyIntegration);
            assert_eq!(verify.argv[0], "git");
            let status = Command::new("git")
                .args(&verify.argv[1..])
                .status()
                .unwrap();
            assert!(status.success(), "{:?}", verify.argv);
            let step = &closeout.steps[1];
            assert_eq!(step.action, ReportedCloseoutAction::RecordIntegration);
            let flag = |name: &str| {
                let index = step.argv.iter().position(|arg| arg == name).unwrap();
                step.argv[index + 1].clone()
            };
            assert_eq!(flag("--method"), "fast-forward");
            record(
                &fixture.doc,
                &flag("--item"),
                flag("--generation").parse().unwrap(),
                RecordRequest {
                    kind: QueueLineageEventKind::Integrated,
                    source_repo_path: Some(PathBuf::from(flag("--source-repo"))),
                    sink_repo_path: Some(PathBuf::from(flag("--sink-repo"))),
                    source_sha: Some(flag("--source-sha")),
                    integration_sha: Some(flag("--integration-sha")),
                    integration_method: Some(IntegrationMethod::parse(&flag("--method")).unwrap()),
                    sink_ref: Some(flag("--sink-ref")),
                    ..RecordRequest::default()
                },
            )
            .unwrap();

            require_integrated_repo_work(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            assert_eq!(
                shape(&fixture, &claim),
                ReportedCloseoutShape::IntegratedConsume
            );
        }

        #[test]
        fn repo_work_outside_the_sink_requires_integration_first() {
            let fixture = RepoFixture::new();
            let main = git_output(&fixture.root, &["branch", "--show-current"]).unwrap();
            git(&fixture.root, &["checkout", "-qb", "side"]);
            let source = fixture.commit_file("side.txt", "side\n", "side work");
            git(&fixture.root, &["checkout", "-q", &main]);
            let claim = reported(make_claim(
                &fixture.doc,
                "side work",
                "subagent:side",
                Some("worker:side"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            record_commit(&fixture, &claim, &source, &main);

            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            assert!(
                matches!(
                    &evidence,
                    ReportedRepoEvidence::Unintegrated {
                        sink: Some(SinkContainment::DoesNotContain {
                            fast_forward: true,
                            ..
                        }),
                        ..
                    }
                ),
                "{evidence:?}"
            );
            let closeout = reported_claim_closeout("doc.md", &claim, &evidence).unwrap();
            assert_eq!(closeout.shape, ReportedCloseoutShape::IntegrationRequired);
            assert_eq!(closeout.steps[0].action, ReportedCloseoutAction::Integrate);
        }

        #[test]
        fn repo_work_without_a_sink_ref_names_the_missing_fact() {
            let fixture = RepoFixture::new();
            let claim = reported(make_claim(
                &fixture.doc,
                "unsunk work",
                "subagent:u",
                Some("worker:u"),
            ));
            record_started(&fixture.doc, &claim).unwrap();
            record(
                &fixture.doc,
                &claim.item_text,
                claim.generation,
                RecordRequest {
                    kind: QueueLineageEventKind::Committed,
                    source_repo_path: Some(fixture.root.clone()),
                    source_sha: Some(fixture.head()),
                    ..RecordRequest::default()
                },
            )
            .unwrap();
            let evidence =
                reported_repo_evidence(&fixture.doc, &claim.item_text, claim.generation).unwrap();
            let closeout = reported_claim_closeout("doc.md", &claim, &evidence).unwrap();
            assert_eq!(closeout.shape, ReportedCloseoutShape::MissingFacts);
            assert_eq!(closeout.missing_facts, ["sink_ref"]);
        }
    }
}
