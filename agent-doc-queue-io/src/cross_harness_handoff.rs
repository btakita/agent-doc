//! Persist cross-harness handoffs and owning-session inbox messages
//! (`#crossagenthandoff`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::cross_harness_handoff::{
    CROSS_HARNESS_HANDOFF_STATE_KIND, CROSS_HARNESS_INBOX_STATE_KIND, HandoffKey, HandoffLedger,
    HandoffRecord, InboxLedger, ListedWorktree, LocationParts, OwningSessionMessage, PacketClaim,
    ResumeDecision, ResumeQuery, SpawnDelivery, WorktreeHint, decide_resume, fill_location,
    handoff_packet_header, match_listed_worktree, missing_location, normalize_handoff_target,
    parse_worktree_porcelain, push_inbox, query_from_parts, recovery_from_hand_back,
    render_handoff_packet, resume_command, spawn_delivery, upsert_record,
};
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result, bail};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

fn document_session(content: &str) -> Result<String> {
    agent_doc_frontmatter::frontmatter::session_id_from_content(content)
        .map(|session| session.trim().to_string())
        .filter(|session| !session.is_empty())
        .context("document has no agent_doc_session; cross-harness handoff keys on it")
}

fn canon_harness(raw: Option<String>) -> Option<String> {
    raw.map(|value| agent_doc_queue::worker_quota::quota_harness_key(&value))
        .filter(|value| !value.is_empty())
}

fn load_ledger(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<HandoffLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        CROSS_HARNESS_HANDOFF_STATE_KIND,
    )?
    else {
        return Ok(HandoffLedger::default());
    };
    serde_json::from_str(&record.payload_json).context("parse queue cross-harness handoffs")
}

fn load_inbox(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<InboxLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        CROSS_HARNESS_INBOX_STATE_KIND,
    )?
    else {
        return Ok(InboxLedger::default());
    };
    serde_json::from_str(&record.payload_json).context("parse queue cross-harness inbox")
}

fn write_state(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
    canonical_path: &str,
    state_kind: &str,
    payload_json: &str,
) -> Result<()> {
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash: document_hash.to_string(),
            state_kind: state_kind.to_string(),
            canonical_path: canonical_path.to_string(),
            payload_json: payload_json.to_string(),
            updated_at_secs: now_secs(),
        },
    )
}

fn mutate_both<T>(
    file: &Path,
    mutate: impl FnOnce(&mut HandoffLedger, &mut InboxLedger) -> Result<T>,
) -> Result<T> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        bail!("queue handoff needs a project root for {}", file.display());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue cross-harness handoff transaction")?;
    let result = (|| {
        let mut handoffs = load_ledger(&conn, &document_hash)?;
        let mut inbox = load_inbox(&conn, &document_hash)?;
        let value = mutate(&mut handoffs, &mut inbox)?;
        write_state(
            &conn,
            &document_hash,
            &canonical_path,
            CROSS_HARNESS_HANDOFF_STATE_KIND,
            &serde_json::to_string(&handoffs).context("serialize queue cross-harness handoffs")?,
        )?;
        write_state(
            &conn,
            &document_hash,
            &canonical_path,
            CROSS_HARNESS_INBOX_STATE_KIND,
            &serde_json::to_string(&inbox).context("serialize queue cross-harness inbox")?,
        )?;
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue cross-harness handoff transaction")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!(
                    "[queue-claim] WARNING: cross-harness handoff rollback failed after {err:#}: {rollback}"
                );
            }
            Err(err)
        }
    }
}

fn load_both(file: &Path) -> Result<(HandoffLedger, InboxLedger)> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok((HandoffLedger::default(), InboxLedger::default()));
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok((HandoffLedger::default(), InboxLedger::default()));
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    Ok((
        load_ledger(&conn, &document_hash)?,
        load_inbox(&conn, &document_hash)?,
    ))
}

/// Fields `queue handoff` and `queue start` record for one attempt.
#[derive(Debug, Clone)]
pub struct HandoffInput {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
    pub owner_harness: Option<String>,
    pub worker_harness: Option<String>,
    pub worker_conversation_id: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
}

fn record_input(
    file: &Path,
    content: &str,
    input: HandoffInput,
    recorded_at_secs: u64,
) -> Result<HandoffRecord> {
    let session_id = document_session(content)?;
    let Some((_, _, document_path)) = state_identity(file)? else {
        bail!("queue handoff needs a project root for {}", file.display());
    };
    let key = HandoffKey {
        session_id,
        document_path,
        item: input.item,
        owner: input.owner,
        generation: input.generation,
        worker_id: input.worker_id,
    }
    .normalized();
    if !key.is_complete() {
        bail!("queue handoff needs session, document, item, owner, generation, and worker id");
    }
    let incoming = HandoffRecord {
        key,
        owner_harness: canon_harness(input.owner_harness),
        worker_harness: canon_harness(input.worker_harness),
        worker_conversation_id: input
            .worker_conversation_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        worktree: input.worktree,
        branch: input.branch,
        head: input.head,
        recorded_at_secs,
    };
    mutate_both(file, |handoffs, _inbox| {
        upsert_record(&mut handoffs.records, incoming.clone());
        let stored = handoffs
            .records
            .iter()
            .find(|record| record.key == incoming.key)
            .cloned()
            .context("cross-harness handoff missing after upsert")?;
        Ok(stored)
    })
}

/// Record an explicit handoff. A missing session id is an error.
pub fn record(file: &Path, content: &str, input: HandoffInput) -> Result<HandoffRecord> {
    record_input(file, content, input, now_secs())
}

/// Record the location `queue start` was given. No location flags is a no-op.
pub fn record_start_location(
    file: &Path,
    content: &str,
    claim: &QueueClaim,
    worktree: Option<&Path>,
    branch: Option<&str>,
    head: Option<&str>,
) -> Result<HandoffRecord> {
    let worker_id = claim
        .worker
        .as_ref()
        .map(|worker| worker.worker_id.clone())
        .unwrap_or_default();
    record(
        file,
        content,
        HandoffInput {
            item: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            worker_id,
            owner_harness: claim
                .owner_session
                .as_ref()
                .and_then(|session| session.harness.clone()),
            worker_harness: claim
                .dispatch_policy
                .as_ref()
                .and_then(|policy| policy.harness.clone()),
            worker_conversation_id: None,
            worktree: worktree.map(|path| path.display().to_string()),
            branch: branch.map(str::to_string),
            head: head.map(str::to_string),
        },
    )
}

/// Owned resume result. A conversation id never yields [`StoredResume::Recovered`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredResume {
    Recovered {
        worktree: String,
        branch: String,
        head: String,
        record: HandoffRecord,
    },
    Incomplete {
        missing: Vec<&'static str>,
        record: HandoffRecord,
    },
    Missing,
    ConversationRefused {
        harness: String,
        conversation_id: String,
        others: Vec<HandoffRecord>,
    },
}

fn stored_decision(decision: ResumeDecision<'_>) -> StoredResume {
    match decision {
        ResumeDecision::Recovered {
            worktree,
            branch,
            head,
            record,
        } => StoredResume::Recovered {
            worktree: worktree.to_string(),
            branch: branch.to_string(),
            head: head.to_string(),
            record: record.clone(),
        },
        ResumeDecision::Incomplete { missing, record } => StoredResume::Incomplete {
            missing,
            record: record.clone(),
        },
        ResumeDecision::Missing => StoredResume::Missing,
        ResumeDecision::ConversationRefused {
            harness,
            conversation_id,
            others,
        } => StoredResume::ConversationRefused {
            harness,
            conversation_id,
            others: others.into_iter().cloned().collect(),
        },
    }
}

/// Resume by the document's session and the five remaining key fields.
pub fn resume_by_key(
    file: &Path,
    content: &str,
    item: &str,
    owner: &str,
    generation: u64,
    worker_id: &str,
) -> Result<StoredResume> {
    let session_id = document_session(content)?;
    let Some((_, _, document_path)) = state_identity(file)? else {
        bail!("queue resume needs a project root for {}", file.display());
    };
    let key = HandoffKey {
        session_id,
        document_path,
        item: item.to_string(),
        owner: owner.to_string(),
        generation,
        worker_id: worker_id.to_string(),
    }
    .normalized();
    let (handoffs, _) = load_both(file)?;
    Ok(stored_decision(decide_resume(
        &handoffs.records,
        ResumeQuery::Key(key),
    )))
}

/// A harness conversation id does not continue a worker.
pub fn refuse_conversation(
    file: &Path,
    harness: &str,
    conversation_id: &str,
) -> Result<StoredResume> {
    let (handoffs, _) = load_both(file)?;
    let query = query_from_parts(None, Some(harness), Some(conversation_id))
        .map_err(|err| anyhow::anyhow!(err))?;
    Ok(stored_decision(decide_resume(&handoffs.records, query)))
}

/// Result of `queue control`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlPost {
    Native { tool: &'static str },
    Stored(OwningSessionMessage),
}

/// Fields `queue control` posts for one spawn request.
#[derive(Debug, Clone)]
pub struct ControlInput {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
    pub from_harness: String,
    pub owner_harness: String,
    pub message: String,
}

/// Store a spawn request when the owning session is a different harness.
pub fn post_control(file: &Path, content: &str, input: ControlInput) -> Result<ControlPost> {
    let message = input.message.trim();
    if message.is_empty() {
        bail!("queue control needs a non-empty --message");
    }
    match spawn_delivery(&input.owner_harness, &input.from_harness) {
        SpawnDelivery::Native { tool } => Ok(ControlPost::Native { tool }),
        SpawnDelivery::OwningSessionInbox => {
            let session_id = document_session(content)?;
            let Some((_, _, document_path)) = state_identity(file)? else {
                bail!("queue control needs a project root for {}", file.display());
            };
            let stored = OwningSessionMessage {
                session_id,
                document_path,
                item: input.item.trim().to_string(),
                owner: input.owner.trim().to_string(),
                generation: input.generation,
                worker_id: input.worker_id.trim().to_string(),
                from_harness: agent_doc_queue::worker_quota::quota_harness_key(&input.from_harness),
                owner_harness: agent_doc_queue::worker_quota::quota_harness_key(
                    &input.owner_harness,
                ),
                message: message.to_string(),
                recorded_at_secs: now_secs(),
            };
            mutate_both(file, |_handoffs, inbox| {
                push_inbox(&mut inbox.messages, stored.clone());
                Ok(ControlPost::Stored(stored))
            })
        }
    }
}

/// Inbox messages stored for this document.
pub fn list_inbox(file: &Path) -> Result<Vec<OwningSessionMessage>> {
    let (_, inbox) = load_both(file)?;
    Ok(inbox.messages)
}

/// Record a non-terminal goal hand-back so another harness can resume it, and
/// tell the owning session when that session is a different harness.
///
/// A document with no session id, or a claim with no worker, records nothing.
pub fn record_goal_requeue(
    file: &Path,
    content: &str,
    claim: &QueueClaim,
    response: &str,
) -> Result<()> {
    let Some(session_id) = agent_doc_frontmatter::frontmatter::session_id_from_content(content)
        .map(|session| session.trim().to_string())
        .filter(|session| !session.is_empty())
    else {
        return Ok(());
    };
    let Some(worker_id) = claim
        .worker
        .as_ref()
        .map(|worker| worker.worker_id.trim().to_string())
        .filter(|worker| !worker.is_empty())
    else {
        return Ok(());
    };
    let Some((_, _, document_path)) = state_identity(file)? else {
        return Ok(());
    };
    let location = recovery_from_hand_back(response);
    let owner_harness = canon_harness(
        claim
            .owner_session
            .as_ref()
            .and_then(|session| session.harness.clone()),
    );
    let worker_harness = canon_harness(
        claim
            .dispatch_policy
            .as_ref()
            .and_then(|policy| policy.harness.clone()),
    );
    let incoming = HandoffRecord {
        key: HandoffKey {
            session_id: session_id.clone(),
            document_path: document_path.clone(),
            item: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            worker_id: worker_id.clone(),
        }
        .normalized(),
        owner_harness: owner_harness.clone(),
        worker_harness: worker_harness.clone(),
        worker_conversation_id: None,
        worktree: location.worktree,
        branch: location.branch,
        head: location.head,
        recorded_at_secs: now_secs(),
    };
    let inbox_message = match (&owner_harness, &worker_harness) {
        (Some(owner_harness), Some(worker_harness))
            if matches!(
                spawn_delivery(owner_harness, worker_harness),
                SpawnDelivery::OwningSessionInbox
            ) =>
        {
            Some(OwningSessionMessage {
                session_id,
                document_path,
                item: claim.item_text.clone(),
                owner: claim.owner.clone(),
                generation: claim.generation,
                worker_id,
                from_harness: worker_harness.clone(),
                owner_harness: owner_harness.clone(),
                message: format!(
                    "prose hand-back does not begin with REVIEW-CLEAN or BLOCKED. The row stays open in the inbox. Spawn the next worker from this session. Resume with agent-doc queue resume --item {:?} --owner {} --generation {} --worker {}",
                    claim.item_text,
                    claim.owner,
                    claim.generation,
                    claim
                        .worker
                        .as_ref()
                        .map(|worker| worker.worker_id.as_str())
                        .unwrap_or(""),
                ),
                recorded_at_secs: now_secs(),
            })
        }
        _ => None,
    };
    mutate_both(file, |handoffs, inbox| {
        upsert_record(&mut handoffs.records, incoming);
        if let Some(message) = inbox_message {
            push_inbox(&mut inbox.messages, message);
        }
        Ok(())
    })
}

/// One claim `queue handoff --all` recorded (`#autohandoff`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoHandoffClaim {
    pub record: HandoffRecord,
    pub resume_command: String,
    pub missing: Vec<&'static str>,
}

/// Result of `queue handoff --all`. An empty `claims` writes no exchange packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoHandoffReport {
    pub to: Option<String>,
    pub header: String,
    pub packet: String,
    pub claims: Vec<AutoHandoffClaim>,
    pub skipped_without_worker: usize,
}

/// Record every in-flight claim. Location comes from the start receipt, then
/// lineage, then `git worktree list` of the source repo.
pub fn handoff_all(file: &Path, content: &str, to: Option<&str>) -> Result<AutoHandoffReport> {
    handoff_all_with(file, content, to, git_worktree_list)
}

/// [`handoff_all`] with an injected worktree list, so tests do not spawn git.
pub fn handoff_all_with(
    file: &Path,
    content: &str,
    to: Option<&str>,
    mut list_worktrees: impl FnMut(&Path) -> Result<Vec<ListedWorktree>>,
) -> Result<AutoHandoffReport> {
    let to = to.and_then(normalize_handoff_target);
    let ledger = crate::queue_claim::load_ledger(file)?;
    let lineage = crate::queue_lineage::load(file)?;
    let (handoffs, _) = load_both(file)?;
    let mut inflight: Vec<&agent_doc_queue::queue_claim::QueueClaim> = ledger
        .claims
        .iter()
        .filter(|claim| {
            matches!(
                claim.state,
                agent_doc_queue::queue_claim::QueueClaimState::Running
                    | agent_doc_queue::queue_claim::QueueClaimState::QuotaBlocked
            )
        })
        .collect();
    inflight.sort_by(|left, right| {
        (&left.owner, left.generation, &left.item_text).cmp(&(
            &right.owner,
            right.generation,
            &right.item_text,
        ))
    });
    let mut skipped_without_worker = 0usize;
    let mut prepared = Vec::new();
    for claim in inflight {
        let Some(worker_id) = worker_id_for(claim, &lineage) else {
            skipped_without_worker += 1;
            continue;
        };
        let existing = handoffs.records.iter().find(|record| {
            record.key.item.trim() == claim.item_text.trim()
                && record.key.owner.trim() == claim.owner.trim()
                && record.key.generation == claim.generation
                && record.key.worker_id.trim() == worker_id
        });
        let start = LocationParts {
            worktree: existing.and_then(|record| record.worktree.clone()),
            branch: existing.and_then(|record| record.branch.clone()),
            head: existing.and_then(|record| record.head.clone()),
        };
        let (lineage_location, source_repo) = lineage_location_for(claim, &lineage);
        let location = fill_location(start, lineage_location);
        prepared.push(Prepared {
            claim,
            worker_id: worker_id.to_string(),
            location,
            source_repo,
        });
    }
    let needs_list = prepared.iter().any(|item| location_missing(&item.location));
    let catalog = if needs_list {
        load_catalog(file, &lineage, &handoffs, &mut list_worktrees)
    } else {
        Vec::new()
    };
    let file_arg = file.display().to_string();
    let mut claims = Vec::new();
    for mut item in prepared {
        if location_missing(&item.location)
            && let Some(matched) = match_claim_worktree_owned(&catalog, &item)
        {
            item.location = fill_location(
                item.location,
                LocationParts {
                    worktree: Some(matched.path),
                    branch: matched.branch,
                    head: matched.head,
                },
            );
        }
        let recorded = record(
            file,
            content,
            HandoffInput {
                item: item.claim.item_text.clone(),
                owner: item.claim.owner.clone(),
                generation: item.claim.generation,
                worker_id: item.worker_id.clone(),
                owner_harness: item
                    .claim
                    .owner_session
                    .as_ref()
                    .and_then(|session| session.harness.clone()),
                worker_harness: item
                    .claim
                    .dispatch_policy
                    .as_ref()
                    .and_then(|policy| policy.harness.clone()),
                worker_conversation_id: None,
                worktree: item.location.worktree.clone(),
                branch: item.location.branch.clone(),
                head: item.location.head.clone(),
            },
        )?;
        let missing = missing_location(&recorded);
        let resume = resume_command(
            &file_arg,
            &recorded.key.item,
            &recorded.key.owner,
            recorded.key.generation,
            &recorded.key.worker_id,
        );
        claims.push(AutoHandoffClaim {
            record: recorded,
            resume_command: resume,
            missing,
        });
    }
    if claims.is_empty() {
        return Ok(AutoHandoffReport {
            to,
            header: String::new(),
            packet: String::new(),
            claims,
            skipped_without_worker,
        });
    }
    let packet_claims: Vec<PacketClaim> = claims
        .iter()
        .map(|claim| PacketClaim {
            owner: claim.record.key.owner.clone(),
            item: claim.record.key.item.clone(),
            generation: claim.record.key.generation,
            worker_id: claim.record.key.worker_id.clone(),
            worktree: claim.record.worktree.clone(),
            branch: claim.record.branch.clone(),
            head: claim.record.head.clone(),
            resume_command: claim.resume_command.clone(),
            missing: claim.missing.clone(),
        })
        .collect();
    let header = handoff_packet_header(to.as_deref());
    let packet = render_handoff_packet(to.as_deref(), &packet_claims);
    Ok(AutoHandoffReport {
        to,
        header,
        packet,
        claims,
        skipped_without_worker,
    })
}

struct Prepared<'a> {
    claim: &'a agent_doc_queue::queue_claim::QueueClaim,
    worker_id: String,
    location: LocationParts,
    source_repo: Option<String>,
}

fn location_missing(location: &LocationParts) -> bool {
    filled_text(location.worktree.as_deref()).is_none()
        || filled_text(location.branch.as_deref()).is_none()
        || filled_text(location.head.as_deref()).is_none()
}

fn filled_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

fn worker_id_for<'a>(
    claim: &'a agent_doc_queue::queue_claim::QueueClaim,
    lineage: &'a crate::queue_lineage::QueueLineageLedger,
) -> Option<&'a str> {
    if let Some(worker) = claim
        .worker
        .as_ref()
        .map(|worker| worker.worker_id.trim())
        .filter(|worker| !worker.is_empty())
    {
        return Some(worker);
    }
    lineage_for(lineage, claim).and_then(|attempt| {
        attempt
            .events
            .iter()
            .rev()
            .find_map(|event| filled_text(event.worker_id.as_deref()))
    })
}

fn lineage_for<'a>(
    lineage: &'a crate::queue_lineage::QueueLineageLedger,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
) -> Option<&'a crate::queue_lineage::QueueLineageAttempt> {
    lineage.attempts.iter().find(|attempt| {
        attempt.generation == claim.generation
            && attempt.owner == claim.owner
            && (attempt.identity == claim.identity
                || attempt
                    .identity_history
                    .iter()
                    .any(|identity| identity == &claim.identity))
    })
}

fn lineage_location_for(
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    lineage: &crate::queue_lineage::QueueLineageLedger,
) -> (LocationParts, Option<String>) {
    let Some(attempt) = lineage_for(lineage, claim) else {
        return (LocationParts::default(), None);
    };
    let mut location = LocationParts::default();
    let mut source_repo = None;
    for event in &attempt.events {
        if event.kind == crate::queue_lineage::QueueLineageEventKind::Integrating {
            continue;
        }
        if let Some(path) = filled_text(event.worktree_path.as_deref()) {
            location.worktree = Some(path.to_string());
        }
        if let Some(branch) = filled_text(event.branch.as_deref()) {
            location.branch = Some(branch.to_string());
        }
        if let Some(head) = filled_text(event.source_sha.as_deref()) {
            location.head = Some(head.to_string());
        }
        if let Some(repo) = filled_text(event.source_repo_path.as_deref()) {
            source_repo = Some(repo.to_string());
        } else if let Some(repo) = filled_text(event.repo_path.as_deref()) {
            source_repo = Some(repo.to_string());
        }
    }
    (location, source_repo)
}

struct CatalogEntry {
    repo: PathBuf,
    entries: Vec<ListedWorktree>,
}

fn worktree_pool(
    catalog: &[CatalogEntry],
    source: Option<&str>,
) -> (Vec<ListedWorktree>, Option<String>) {
    if let Some(source) = filled_text(source)
        && let Some(entry) = catalog.iter().find(|entry| {
            entry.repo.as_os_str() == std::ffi::OsStr::new(source)
                || entry.entries.iter().any(|listed| listed.path == source)
        })
    {
        let name = entry
            .repo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        return (entry.entries.clone(), name);
    }
    let combined = catalog
        .iter()
        .flat_map(|entry| entry.entries.iter().cloned())
        .collect();
    (combined, None)
}

fn match_claim_worktree_owned(
    catalog: &[CatalogEntry],
    item: &Prepared<'_>,
) -> Option<ListedWorktree> {
    let (pool, repo_name) = worktree_pool(catalog, item.source_repo.as_deref());
    match_listed_worktree(
        &pool,
        WorktreeHint {
            worktree: filled_text(item.location.worktree.as_deref()),
            branch: filled_text(item.location.branch.as_deref()),
            owner: &item.claim.owner,
            source_repo_name: repo_name.as_deref(),
        },
    )
    .cloned()
}

struct Candidate {
    path: PathBuf,
    explicit: bool,
}

fn push_candidate(candidates: &mut Vec<Candidate>, path: &Path, explicit: bool) {
    if path.as_os_str().is_empty() || !path.is_dir() {
        return;
    }
    if candidates.iter().any(|have| have.path == path) {
        return;
    }
    candidates.push(Candidate {
        path: path.to_path_buf(),
        explicit,
    });
}

fn gitmodules_checkouts(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("path = ")
                .or_else(|| line.strip_prefix("path="))
        })
        .map(|path| root.join(path.trim()))
        .filter(|path| path.is_dir())
        .collect()
}

fn load_catalog(
    file: &Path,
    lineage: &crate::queue_lineage::QueueLineageLedger,
    handoffs: &HandoffLedger,
    list_worktrees: &mut impl FnMut(&Path) -> Result<Vec<ListedWorktree>>,
) -> Vec<CatalogEntry> {
    let mut candidates = Vec::new();
    for attempt in &lineage.attempts {
        for event in &attempt.events {
            for path in [
                event.source_repo_path.as_deref(),
                event.repo_path.as_deref(),
                event.worktree_path.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                push_candidate(&mut candidates, Path::new(path), true);
            }
        }
    }
    for record in &handoffs.records {
        if let Some(path) = record.worktree.as_deref() {
            push_candidate(&mut candidates, Path::new(path), true);
        }
    }
    if let Some(root) = agent_doc_fs::find_project_root(file) {
        push_candidate(&mut candidates, &root, false);
        for checkout in gitmodules_checkouts(&root) {
            push_candidate(&mut candidates, &checkout, false);
        }
    }
    let mut catalog = Vec::new();
    for candidate in candidates {
        match list_worktrees(&candidate.path) {
            Ok(entries) => catalog.push(CatalogEntry {
                repo: candidate.path,
                entries,
            }),
            Err(err) => {
                if candidate.explicit {
                    eprintln!(
                        "[queue] handoff worktree list skipped {}: {err:#}",
                        candidate.path.display()
                    );
                }
            }
        }
    }
    catalog
}

fn git_worktree_list(repo: &Path) -> Result<Vec<ListedWorktree>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("git worktree list in {}", repo.display()))?;
    if !output.status.success() {
        bail!(
            "git worktree list in {} failed: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(parse_worktree_porcelain(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SESSION: &str = "78c70863-fed7-40a8-b703-8261eeb8da9c";
    const ITEM: &str =
        "https://github.com/haiven-dev/haiven-sdk/pull/286: #address then #adversarial-pr";
    const OWNER: &str = "subagent:addr-286";
    const WORKER: &str = "a7e8d9d85c28f8da8";
    const GROK_CONVERSATION: &str = "01a1e0e1-6a34-7a12-bd15-5fa98f34c9e5";

    fn doc(dir: &Path) -> std::path::PathBuf {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let path = dir.join("sdk-dev.md");
        fs::write(
            &path,
            format!(
                "---\nagent_doc_session: {SESSION}\nagent_doc_format: template\n---\n\n# sdk\n"
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn sqlite_resume_recovers_the_claude_worker_and_refuses_the_grok_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let file = doc(dir.path());
        let content = fs::read_to_string(&file).unwrap();
        record(
            &file,
            &content,
            HandoffInput {
                item: ITEM.to_string(),
                owner: OWNER.to_string(),
                generation: 1,
                worker_id: WORKER.to_string(),
                owner_harness: Some("grok".to_string()),
                worker_harness: Some("claude".to_string()),
                worker_conversation_id: None,
                worktree: Some(
                    "/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/demooverlay"
                        .to_string(),
                ),
                branch: Some("feat/demo-upstream-overlay".to_string()),
                head: Some("177f46b753f55f7b37c22706a615ad687544a041".to_string()),
            },
        )
        .unwrap();
        record(
            &file,
            &content,
            HandoffInput {
                item: ITEM.to_string(),
                owner: OWNER.to_string(),
                generation: 1,
                worker_id: WORKER.to_string(),
                owner_harness: None,
                worker_harness: None,
                worker_conversation_id: None,
                worktree: None,
                branch: None,
                head: None,
            },
        )
        .unwrap();
        match resume_by_key(&file, &content, ITEM, OWNER, 1, WORKER).unwrap() {
            StoredResume::Recovered {
                worktree,
                branch,
                head,
                ..
            } => {
                assert_eq!(
                    worktree,
                    "/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/demooverlay"
                );
                assert_eq!(branch, "feat/demo-upstream-overlay");
                assert_eq!(head, "177f46b753f55f7b37c22706a615ad687544a041");
            }
            other => panic!("resume keeps the earlier location: {other:?}"),
        }
        match refuse_conversation(&file, "grok", GROK_CONVERSATION).unwrap() {
            StoredResume::ConversationRefused {
                conversation_id,
                others,
                ..
            } => {
                assert_eq!(conversation_id, GROK_CONVERSATION);
                assert_eq!(others[0].key.worker_id, WORKER);
                assert_ne!(others[0].key.worker_id, GROK_CONVERSATION);
            }
            other => panic!("grok conversation does not continue the worker: {other:?}"),
        }
    }

    #[test]
    fn control_reaches_a_different_harness_and_stays_native_for_the_same_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = doc(dir.path());
        let content = fs::read_to_string(&file).unwrap();
        let stored = post_control(
            &file,
            &content,
            ControlInput {
                item: ITEM.to_string(),
                owner: OWNER.to_string(),
                generation: 1,
                worker_id: WORKER.to_string(),
                from_harness: "grok".to_string(),
                owner_harness: "claude".to_string(),
                message: "spawn the reviewer from the owning session".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(stored, ControlPost::Stored(_)));
        let native = post_control(
            &file,
            &content,
            ControlInput {
                item: ITEM.to_string(),
                owner: OWNER.to_string(),
                generation: 1,
                worker_id: WORKER.to_string(),
                from_harness: "grok".to_string(),
                owner_harness: "grok-build".to_string(),
                message: "same harness stays native".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            native,
            ControlPost::Native {
                tool: "send_subagent_message"
            }
        );
        let inbox = list_inbox(&file).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].from_harness, "grok");
        assert_eq!(inbox[0].owner_harness, "claude");
        assert!(inbox[0].message.contains("spawn the reviewer"));
    }

    fn queue_doc(dir: &Path, items: &[&str]) -> PathBuf {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let path = dir.join("bugs.md");
        let rows = items
            .iter()
            .map(|item| format!("- {item}\n"))
            .collect::<String>();
        fs::write(
            &path,
            format!(
                "---\nagent_doc_session: {SESSION}\nagent_doc_format: template\n---\n\n\
## Queue\n\n<!-- agent:queue subagents go -->\n{rows}<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        path
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=T"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit_repo(dir: &Path) -> String {
        git(dir, &["init", "-b", "patchrun"]);
        fs::write(dir.join("README"), "x\n").unwrap();
        git(dir, &["add", "README"]);
        git(dir, &["commit", "-m", "init"]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn claim_start(file: &Path, item: &str, owner: &str, worker: &str) -> QueueClaim {
        let (_, claimed) = crate::queue_claim::claim(file, item, owner, 3600).unwrap();
        crate::queue_claim::start_attempt(
            file,
            &claimed.item_text,
            owner,
            worker,
            3600,
            claimed.generation,
        )
        .unwrap()
    }

    #[test]
    fn running_claim_keeps_the_lineage_location_and_prints_resume() {
        let dir = tempfile::tempdir().unwrap();
        let head = commit_repo(dir.path());
        let file = queue_doc(dir.path(), &["do [#autohandoff]"]);
        let started = claim_start(&file, "do [#autohandoff]", "subagent:patchrun", "worker-1");
        crate::queue_lineage::record(
            &file,
            &started.item_text,
            started.generation,
            crate::queue_lineage::RecordRequest {
                kind: crate::queue_lineage::QueueLineageEventKind::Attached,
                worker_id: Some("worker-1".to_string()),
                worktree_path: Some(dir.path().to_path_buf()),
                branch: Some("patchrun".to_string()),
                source_sha: Some(head.clone()),
                ..crate::queue_lineage::RecordRequest::default()
            },
        )
        .unwrap();
        let content = fs::read_to_string(&file).unwrap();
        let mut listed = false;
        let report = handoff_all_with(&file, &content, Some("Grok Build"), |_| {
            listed = true;
            Ok(Vec::new())
        })
        .unwrap();
        assert!(!listed, "a complete location does not list worktrees");
        assert_eq!(report.to.as_deref(), Some("grok"));
        assert_eq!(report.claims.len(), 1);
        assert!(report.claims[0].missing.is_empty());
        assert!(report.packet.contains(&report.claims[0].resume_command));
        assert!(report.packet.contains("(#autohandoff)"));
        let expected = dir.path().canonicalize().unwrap().display().to_string();
        assert_eq!(
            report.claims[0].record.worktree.as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(report.claims[0].record.head.as_deref(), Some(head.as_str()));
        match resume_by_key(
            &file,
            &content,
            &started.item_text,
            "subagent:patchrun",
            started.generation,
            "worker-1",
        )
        .unwrap()
        {
            StoredResume::Recovered {
                branch, head: got, ..
            } => {
                assert_eq!(branch, "patchrun");
                assert_eq!(got, head);
            }
            other => panic!("resume did not recover the lineage location: {other:?}"),
        }
    }

    #[test]
    fn worktree_list_fills_a_claim_the_start_receipt_left_blank() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/agent-doc")).unwrap();
        fs::create_dir_all(dir.path().join("src/tmux-router")).unwrap();
        fs::write(
            dir.path().join(".gitmodules"),
            "[submodule \"agent-doc\"]\n\tpath = src/agent-doc\n\turl = ../agent-doc\n\
             [submodule \"tmux-router\"]\n\tpath = src/tmux-router\n\turl = ../tmux-router\n",
        )
        .unwrap();
        let file = queue_doc(dir.path(), &["do [#patchrun]"]);
        let started = claim_start(&file, "do [#patchrun]", "subagent:patchrun", "worker-2");
        let content = fs::read_to_string(&file).unwrap();
        let report = handoff_all_with(&file, &content, None, |path| {
            if path.ends_with("src/agent-doc") {
                Ok(vec![ListedWorktree {
                    path: "/home/brian/worktrees/patchrun/agent-doc".to_string(),
                    head: Some("abc123".to_string()),
                    branch: Some("patchrun".to_string()),
                }])
            } else if path.ends_with("src/tmux-router") {
                Ok(vec![ListedWorktree {
                    path: "/home/brian/worktrees/patchrun/tmux-router".to_string(),
                    head: Some("def456".to_string()),
                    branch: None,
                }])
            } else {
                Ok(Vec::new())
            }
        })
        .unwrap();
        assert_eq!(report.claims.len(), 1);
        assert!(
            report.claims[0].missing.is_empty(),
            "{:?}",
            report.claims[0].missing
        );
        assert_eq!(
            report.claims[0].record.worktree.as_deref(),
            Some("/home/brian/worktrees/patchrun/agent-doc")
        );
        assert_eq!(report.claims[0].record.branch.as_deref(), Some("patchrun"));
        assert_eq!(report.claims[0].record.head.as_deref(), Some("abc123"));
        assert!(report.packet.contains(&report.claims[0].resume_command));
        assert_eq!(report.header, "cross-harness handoff — agent-doc");
        let _ = started;
    }

    #[test]
    fn start_receipt_location_wins_over_a_later_lineage_head() {
        let dir = tempfile::tempdir().unwrap();
        let file = queue_doc(dir.path(), &["do [#kept]"]);
        let started = claim_start(&file, "do [#kept]", "subagent:kept", "worker-3");
        let content = fs::read_to_string(&file).unwrap();
        record(
            &file,
            &content,
            HandoffInput {
                item: started.item_text.clone(),
                owner: started.owner.clone(),
                generation: started.generation,
                worker_id: "worker-3".to_string(),
                owner_harness: None,
                worker_harness: None,
                worker_conversation_id: None,
                worktree: Some("/kept/worktree".to_string()),
                branch: Some("kept-branch".to_string()),
                head: Some("abc111".to_string()),
            },
        )
        .unwrap();
        crate::queue_lineage::record(
            &file,
            &started.item_text,
            started.generation,
            crate::queue_lineage::RecordRequest {
                kind: crate::queue_lineage::QueueLineageEventKind::Attached,
                worker_id: Some("worker-3".to_string()),
                branch: Some("other".to_string()),
                source_sha: Some("def222".to_string()),
                ..crate::queue_lineage::RecordRequest::default()
            },
        )
        .unwrap();
        let mut listed = false;
        let report = handoff_all_with(&file, &content, None, |_| {
            listed = true;
            Ok(Vec::new())
        })
        .unwrap();
        assert!(!listed);
        assert_eq!(
            report.claims[0].record.worktree.as_deref(),
            Some("/kept/worktree")
        );
        assert_eq!(
            report.claims[0].record.branch.as_deref(),
            Some("kept-branch")
        );
        assert_eq!(report.claims[0].record.head.as_deref(), Some("abc111"));
    }

    #[test]
    fn claimed_and_reported_attempts_are_not_running_claims() {
        let dir = tempfile::tempdir().unwrap();
        let file = queue_doc(
            dir.path(),
            &["do [#running]", "do [#claimed]", "do [#reported]"],
        );
        let running = claim_start(&file, "do [#running]", "subagent:running", "worker-r");
        let _claimed =
            crate::queue_claim::claim(&file, "do [#claimed]", "subagent:claimed", 3600).unwrap();
        let reported = claim_start(&file, "do [#reported]", "subagent:reported", "worker-p");
        crate::queue_claim::report_attempt(&file, &reported.item_text, Some(reported.generation))
            .unwrap();
        let content = fs::read_to_string(&file).unwrap();
        let report = handoff_all_with(&file, &content, None, |_| Ok(Vec::new())).unwrap();
        assert_eq!(report.claims.len(), 1);
        assert_eq!(report.claims[0].record.key.owner, "subagent:running");
        assert_eq!(report.claims[0].record.key.worker_id, "worker-r");
        assert_eq!(report.skipped_without_worker, 0);
        assert!(report.packet.contains(&running.item_text));
        assert!(!report.packet.contains("subagent:claimed"));
        assert!(!report.packet.contains("subagent:reported"));
    }
}
