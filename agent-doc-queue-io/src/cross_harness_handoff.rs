//! Persist cross-harness handoffs and owning-session inbox messages
//! (`#crossagenthandoff`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::cross_harness_handoff::{
    CROSS_HARNESS_HANDOFF_STATE_KIND, CROSS_HARNESS_INBOX_STATE_KIND, HandoffKey, HandoffLedger,
    HandoffRecord, InboxLedger, OwningSessionMessage, ResumeDecision, ResumeQuery, SpawnDelivery,
    decide_resume, push_inbox, query_from_parts, recovery_from_hand_back, spawn_delivery,
    upsert_record,
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
}
