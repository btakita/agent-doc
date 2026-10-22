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
            QueueLineageEventKind::Cancelled => Some(event.kind),
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
            other => bail!(
                "unknown lineage event {other:?}; expected created|attached|dirty|committed|pushed|integrated|released|cancelled|retained|removed"
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
    Other,
}

impl IntegrationMethod {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "cherry_pick" | "cherrypick" => Ok(Self::CherryPick),
            "rebase" => Ok(Self::Rebase),
            "merge" => Ok(Self::Merge),
            "fast_forward" | "ff" => Ok(Self::FastForward),
            "other" => Ok(Self::Other),
            other => bail!(
                "unknown integration method {other:?}; expected cherry-pick|rebase|merge|fast-forward|other"
            ),
        }
    }
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
    pub worktree_path: Option<PathBuf>,
    pub branch: Option<String>,
    pub base_sha: Option<String>,
    pub source_sha: Option<String>,
    pub pushed_ref: Option<String>,
    pub integration_sha: Option<String>,
    pub integration_method: Option<IntegrationMethod>,
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
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base_sha: Option<String>,
    pub source_sha: Option<String>,
    pub pushed_ref: Option<String>,
    pub integration_sha: Option<String>,
    pub integration_method: Option<IntegrationMethod>,
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

fn event_from_request(request: RecordRequest) -> Result<QueueLineageEvent> {
    let nonempty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    let event = QueueLineageEvent {
        kind: request.kind,
        at_secs: request.at_secs.unwrap_or_else(now_secs),
        worker_id: nonempty(request.worker_id),
        repo_path: request.repo_path.map(durable_path).transpose()?,
        worktree_path: request.worktree_path.map(durable_path).transpose()?,
        branch: nonempty(request.branch),
        base_sha: nonempty(request.base_sha),
        source_sha: nonempty(request.source_sha),
        pushed_ref: nonempty(request.pushed_ref),
        integration_sha: nonempty(request.integration_sha),
        integration_method: request.integration_method,
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
            {
                bail!(
                    "integrated lineage event requires explicit --source-sha, --integration-sha, and --method"
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
        QueueLineageEventKind::Cancelled
        | QueueLineageEventKind::Retained
        | QueueLineageEventKind::Removed => {}
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Facts {
    worker_id: Option<String>,
    repo_path: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base_sha: Option<String>,
    source_sha: Option<String>,
    pushed_ref: Option<String>,
    integration_sha: Option<String>,
    integration_method: Option<IntegrationMethod>,
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
        keep!(worktree_path);
        keep!(branch);
        keep!(base_sha);
        keep!(source_sha);
        keep!(pushed_ref);
        keep!(integration_sha);
        if event.integration_method.is_some() {
            facts.integration_method = event.integration_method;
        }
        keep!(release_tag);
        if event.release_state.is_some() {
            facts.release_state = event.release_state;
        }
    }
    facts
}

fn verify_event(attempt: &QueueLineageAttempt, event: &QueueLineageEvent) -> Result<()> {
    let prior = facts(attempt);
    let repo = event
        .repo_path
        .as_deref()
        .or(prior.repo_path.as_deref())
        .map(Path::new);
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
            if let (Some(repo), Some(base)) = (repo, event.base_sha.as_deref()) {
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
            let repo = repo.context("commit/push lineage event needs a recorded --repo")?;
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
            let repo = repo.context("integrated lineage event needs a recorded --repo")?;
            verify_integration(
                repo,
                event.source_sha.as_deref().context("missing source SHA")?,
                event
                    .integration_sha
                    .as_deref()
                    .context("missing integration SHA")?,
                event
                    .integration_method
                    .context("missing integration method")?,
            )?;
        }
        QueueLineageEventKind::Released => {
            if matches!(
                event.release_state,
                Some(ReleaseState::Tagged | ReleaseState::Published)
            ) {
                let repo = repo.context("tagged/published release needs a recorded --repo")?;
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
        QueueLineageEventKind::Created | QueueLineageEventKind::Cancelled => {}
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

fn verify_integration(
    repo: &Path,
    source: &str,
    integration: &str,
    method: IntegrationMethod,
) -> Result<()> {
    require_commit(repo, source, "source")?;
    require_commit(repo, integration, "integration")?;
    match method {
        IntegrationMethod::Merge | IntegrationMethod::FastForward => require_ancestor(
            repo,
            source,
            integration,
            "source is not an ancestor of integration",
        ),
        IntegrationMethod::CherryPick | IntegrationMethod::Rebase => {
            let source_patch = patch_id(repo, source)?;
            let integration_patch = patch_id(repo, integration)?;
            if source_patch != integration_patch {
                bail!(
                    "explicit {method:?} mapping has different patch ids: {source_patch} != {integration_patch}"
                );
            }
            Ok(())
        }
        IntegrationMethod::Other => Ok(()),
    }
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
        worktree_path: facts.worktree_path,
        branch: facts.branch,
        base_sha: facts.base_sha,
        source_sha: facts.source_sha,
        pushed_ref: facts.pushed_ref,
        integration_sha: facts.integration_sha,
        integration_method: facts.integration_method,
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
    let Some(repo_raw) = facts.repo_path.as_deref() else {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Degraded,
            text: "repo_unrecorded: record an attached receipt with --repo".to_string(),
        });
        return diagnostics;
    };
    let repo = Path::new(repo_raw);
    if !repo.exists() {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Degraded,
            text: format!("repo_missing: {repo_raw}"),
        });
        return diagnostics;
    }
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
            && require_commit(repo, sha, label).is_err()
        {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!("{label}_commit_missing: {sha}"),
            });
        }
    }
    if let (Some(base), Some(source)) = (facts.base_sha.as_deref(), facts.source_sha.as_deref())
        && require_ancestor(repo, base, source, "").is_err()
    {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Mismatch,
            text: format!("base_not_ancestor_of_source: {base} -> {source}"),
        });
    }
    if let (Some(reference), Some(source)) =
        (facts.pushed_ref.as_deref(), facts.source_sha.as_deref())
    {
        match git_output(repo, &["rev-parse", &format!("{reference}^{{commit}}")]) {
            Ok(actual)
                if actual == source
                    || git_output(repo, &["rev-parse", source]).ok().as_deref()
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
    if let (Some(source), Some(integration), Some(method)) = (
        facts.source_sha.as_deref(),
        facts.integration_sha.as_deref(),
        facts.integration_method,
    ) {
        if method == IntegrationMethod::Other {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Degraded,
                text: "integration_method_other: explicit edge retained but Git equivalence is unverified".to_string(),
            });
        } else if verify_integration(repo, source, integration, method).is_err() {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Mismatch,
                text: format!(
                    "integration_mapping_mismatch: {source} -> {integration} ({method:?})"
                ),
            });
        }
    }
    if let (Some(tag), Some(integration), Some(ReleaseState::Tagged | ReleaseState::Published)) = (
        facts.release_tag.as_deref(),
        facts.integration_sha.as_deref(),
        facts.release_state,
    ) {
        match git_output(repo, &["rev-parse", &format!("{tag}^{{commit}}")]) {
            Ok(actual)
                if actual == integration
                    || git_output(repo, &["rev-parse", integration])
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
            "  repo={} worktree={} branch={} base={}\n",
            attempt.repo_path.as_deref().unwrap_or("-"),
            attempt.worktree_path.as_deref().unwrap_or("-"),
            attempt.branch.as_deref().unwrap_or("-"),
            attempt.base_sha.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!(
            "  source={} pushed_ref={} integration={} method={}\n",
            attempt.source_sha.as_deref().unwrap_or("-"),
            attempt.pushed_ref.as_deref().unwrap_or("-"),
            attempt.integration_sha.as_deref().unwrap_or("-"),
            attempt
                .integration_method
                .map(|method| format!("{method:?}").to_ascii_lowercase())
                .unwrap_or_else(|| "-".to_string())
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
        record(
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
    }
}
