//! Cross-harness handoff of an in-flight queue claim (`#crossagenthandoff`).
//!
//! Resume keys off the agent-doc session, the document, the queue item, the
//! owner, the generation, and the worker id. A harness conversation id is
//! recorded so the owner harness is known; it never continues a worker.

use crate::worker_quota::quota_harness_key;

/// `state_kind` of the per-document handoff ledger in `state.db`.
pub const CROSS_HARNESS_HANDOFF_STATE_KIND: &str = "queue_cross_harness_handoffs";

/// `state_kind` of spawn requests waiting for the queue-owning session.
pub const CROSS_HARNESS_INBOX_STATE_KIND: &str = "queue_cross_harness_inbox";

/// The six fields that resume one in-flight attempt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HandoffKey {
    pub session_id: String,
    pub document_path: String,
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
}

impl HandoffKey {
    /// Trim the text fields. The generation stays exact.
    pub fn normalized(self) -> Self {
        Self {
            session_id: self.session_id.trim().to_string(),
            document_path: self.document_path.trim().to_string(),
            item: self.item.trim().to_string(),
            owner: self.owner.trim().to_string(),
            generation: self.generation,
            worker_id: self.worker_id.trim().to_string(),
        }
    }

    /// True when every text field is non-empty.
    pub fn is_complete(&self) -> bool {
        !self.session_id.is_empty()
            && !self.document_path.is_empty()
            && !self.item.is_empty()
            && !self.owner.is_empty()
            && !self.worker_id.is_empty()
    }
}

/// One recorded attempt, including the worktree a later harness resumes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HandoffRecord {
    pub key: HandoffKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_harness: Option<String>,
    /// The worker's harness conversation, when one was known. Not a resume key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub recorded_at_secs: u64,
}

/// Worktree, branch, and head parsed from a worker hand-back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HandBackLocation {
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
}

/// How a helper reaches the queue-owning session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnDelivery {
    /// Same harness. The binary does not store the message.
    Native { tool: &'static str },
    /// The owning session is a different harness. Store an inbox message.
    OwningSessionInbox,
}

/// A spawn or steer request waiting for the owning session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwningSessionMessage {
    pub session_id: String,
    pub document_path: String,
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
    pub from_harness: String,
    pub owner_harness: String,
    pub message: String,
    pub recorded_at_secs: u64,
}

/// What `queue resume` decided. A conversation id never yields [`Recovered`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeDecision<'a> {
    Recovered {
        worktree: &'a str,
        branch: &'a str,
        head: &'a str,
        record: &'a HandoffRecord,
    },
    Incomplete {
        missing: Vec<&'static str>,
        record: &'a HandoffRecord,
    },
    Missing,
    ConversationRefused {
        harness: String,
        conversation_id: String,
        others: Vec<&'a HandoffRecord>,
    },
}

/// A resume lookup. The key wins when both are present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeQuery {
    Key(HandoffKey),
    Conversation {
        harness: String,
        conversation_id: String,
    },
}

/// The ledger persisted under [`CROSS_HARNESS_HANDOFF_STATE_KIND`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HandoffLedger {
    #[serde(default)]
    pub records: Vec<HandoffRecord>,
}

/// The ledger persisted under [`CROSS_HARNESS_INBOX_STATE_KIND`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InboxLedger {
    #[serde(default)]
    pub messages: Vec<OwningSessionMessage>,
}

const INBOX_LIMIT: usize = 64;

fn filled(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn prefer_filled(incoming: Option<String>, previous: Option<String>) -> Option<String> {
    if filled(&incoming).is_some() {
        incoming
    } else {
        previous
    }
}

/// Keep a previous worktree, branch, or head when the new record omits it.
pub fn merge_record(previous: &HandoffRecord, mut incoming: HandoffRecord) -> HandoffRecord {
    incoming.worktree = prefer_filled(incoming.worktree, previous.worktree.clone());
    incoming.branch = prefer_filled(incoming.branch, previous.branch.clone());
    incoming.head = prefer_filled(incoming.head, previous.head.clone());
    incoming.owner_harness = prefer_filled(incoming.owner_harness, previous.owner_harness.clone());
    incoming.worker_harness =
        prefer_filled(incoming.worker_harness, previous.worker_harness.clone());
    incoming.worker_conversation_id = prefer_filled(
        incoming.worker_conversation_id,
        previous.worker_conversation_id.clone(),
    );
    incoming
}

/// Insert or replace `incoming` by its key, preserving a prior location.
pub fn upsert_record(records: &mut Vec<HandoffRecord>, incoming: HandoffRecord) {
    if let Some(existing) = records.iter_mut().find(|record| record.key == incoming.key) {
        *existing = merge_record(existing, incoming);
    } else {
        records.push(incoming);
    }
}

/// Fields still required before resume can recover the attempt.
pub fn missing_location(record: &HandoffRecord) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if filled(&record.worktree).is_none() {
        missing.push("worktree");
    }
    if filled(&record.branch).is_none() {
        missing.push("branch");
    }
    if filled(&record.head).is_none() {
        missing.push("head");
    }
    missing
}

/// Resume by key, or refuse a harness conversation id.
pub fn decide_resume<'a>(records: &'a [HandoffRecord], query: ResumeQuery) -> ResumeDecision<'a> {
    match query {
        ResumeQuery::Conversation {
            harness,
            conversation_id,
        } => {
            let caller = quota_harness_key(&harness);
            let others = records
                .iter()
                .filter(|record| match record.worker_harness.as_deref() {
                    Some(worker_harness) => quota_harness_key(worker_harness) != caller,
                    None => true,
                })
                .collect();
            ResumeDecision::ConversationRefused {
                harness: caller,
                conversation_id,
                others,
            }
        }
        ResumeQuery::Key(key) => {
            let Some(record) = records.iter().find(|record| record.key == key) else {
                return ResumeDecision::Missing;
            };
            let missing = missing_location(record);
            if missing.is_empty() {
                ResumeDecision::Recovered {
                    worktree: filled(&record.worktree).unwrap_or(""),
                    branch: filled(&record.branch).unwrap_or(""),
                    head: filled(&record.head).unwrap_or(""),
                    record,
                }
            } else {
                ResumeDecision::Incomplete { missing, record }
            }
        }
    }
}

/// The six-field key wins when a conversation id is also supplied.
pub fn query_from_parts(
    key: Option<HandoffKey>,
    harness: Option<&str>,
    conversation_id: Option<&str>,
) -> Result<ResumeQuery, &'static str> {
    if let Some(key) = key {
        return Ok(ResumeQuery::Key(key));
    }
    match (harness, conversation_id) {
        (Some(harness), Some(conversation_id))
            if !harness.trim().is_empty() && !conversation_id.trim().is_empty() =>
        {
            Ok(ResumeQuery::Conversation {
                harness: quota_harness_key(harness),
                conversation_id: conversation_id.trim().to_string(),
            })
        }
        _ => Err("resume needs the agent-doc key or --conversation with --harness"),
    }
}

/// Native tool for `harness`. Grok uses `send_subagent_message`.
pub fn native_tool(harness: &str) -> &'static str {
    if quota_harness_key(harness) == "grok" {
        "send_subagent_message"
    } else {
        "SendMessage"
    }
}

/// Same-harness helpers stay on the native tool. A different owner harness
/// gets an inbox message.
pub fn spawn_delivery(owner_harness: &str, helper_harness: &str) -> SpawnDelivery {
    if quota_harness_key(owner_harness) == quota_harness_key(helper_harness) {
        SpawnDelivery::Native {
            tool: native_tool(helper_harness),
        }
    } else {
        SpawnDelivery::OwningSessionInbox
    }
}

/// Store `message` when the harnesses differ. An identical message is kept once.
pub fn push_inbox(messages: &mut Vec<OwningSessionMessage>, message: OwningSessionMessage) -> bool {
    let duplicate = messages.iter().any(|existing| {
        existing.session_id == message.session_id
            && existing.item == message.item
            && existing.owner == message.owner
            && existing.generation == message.generation
            && existing.worker_id == message.worker_id
            && existing.from_harness == message.from_harness
            && existing.owner_harness == message.owner_harness
            && existing.message == message.message
    });
    if duplicate {
        return false;
    }
    messages.push(message);
    if messages.len() > INBOX_LIMIT {
        let overflow = messages.len() - INBOX_LIMIT;
        messages.drain(0..overflow);
    }
    true
}

fn clean_value(raw: &str) -> String {
    raw.trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches(['.', ','])
        .trim()
        .to_string()
}

fn normalize_line(line: &str) -> &str {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        if rest == before {
            break;
        }
    }
    rest
}

fn hex_sha(value: &str) -> Option<String> {
    let sha = clean_value(value);
    if (7..=40).contains(&sha.len()) && sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(sha)
    } else {
        None
    }
}

/// Read the incident hand-back shape: a delivery worktree, a head branch, and
/// a head. `FIX-ON-HEAD ... head=<sha>` fills the head when no `Head:` line does.
pub fn recovery_from_hand_back(response: &str) -> HandBackLocation {
    let mut location = HandBackLocation::default();
    for line in response.lines() {
        let line = normalize_line(line);
        if let Some(rest) = line.strip_prefix("Delivery worktree:") {
            if location.worktree.is_none() {
                let value = clean_value(rest);
                if !value.is_empty() {
                    location.worktree = Some(value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("Head branch:") {
            if location.branch.is_none() {
                let value = clean_value(rest);
                if !value.is_empty() {
                    location.branch = Some(value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("Head:")
            && location.head.is_none()
        {
            location.head = hex_sha(rest);
        }
    }
    if location.head.is_none() {
        location.head = crate::address_fix_on_head::address_fix_receipts(response)
            .into_iter()
            .map(|(_, sha)| sha)
            .next();
    }
    location
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "78c70863-fed7-40a8-b703-8261eeb8da9c";
    const DOCUMENT: &str =
        "/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md";
    const ITEM: &str =
        "https://github.com/haiven-dev/haiven-sdk/pull/286: #address then #adversarial-pr";
    const OWNER: &str = "subagent:addr-286";
    const WORKER: &str = "a7e8d9d85c28f8da8";
    const GROK_CONVERSATION: &str = "01a1e0e1-6a34-7a12-bd15-5fa98f34c9e5";
    const WORKTREE: &str = "/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/demooverlay";
    const BRANCH: &str = "feat/demo-upstream-overlay";
    const HEAD: &str = "177f46b753f55f7b37c22706a615ad687544a041";

    fn incident_record() -> HandoffRecord {
        HandoffRecord {
            key: HandoffKey {
                session_id: SESSION.to_string(),
                document_path: DOCUMENT.to_string(),
                item: ITEM.to_string(),
                owner: OWNER.to_string(),
                generation: 1,
                worker_id: WORKER.to_string(),
            },
            owner_harness: Some("grok".to_string()),
            worker_harness: Some("claude".to_string()),
            worker_conversation_id: None,
            worktree: Some(WORKTREE.to_string()),
            branch: Some(BRANCH.to_string()),
            head: Some(HEAD.to_string()),
            recorded_at_secs: 1,
        }
    }

    #[test]
    fn resume_by_agent_doc_key_recovers_worktree_branch_and_head() {
        let records = vec![incident_record()];
        let query = query_from_parts(
            Some(records[0].key.clone()),
            Some("grok"),
            Some(GROK_CONVERSATION),
        )
        .unwrap();
        assert!(matches!(query, ResumeQuery::Key(_)));
        match decide_resume(&records, query) {
            ResumeDecision::Recovered {
                worktree,
                branch,
                head,
                record,
            } => {
                assert_eq!(worktree, WORKTREE);
                assert_eq!(branch, BRANCH);
                assert_eq!(head, HEAD);
                assert_eq!(record.key.worker_id, WORKER);
                assert_eq!(record.key.generation, 1);
            }
            other => panic!("key resume recovers the attempt: {other:?}"),
        }
    }

    #[test]
    fn grok_conversation_does_not_continue_the_claude_worker() {
        let records = vec![incident_record()];
        let query = query_from_parts(None, Some("grok"), Some(GROK_CONVERSATION)).unwrap();
        match decide_resume(&records, query) {
            ResumeDecision::ConversationRefused {
                harness,
                conversation_id,
                others,
            } => {
                assert_eq!(harness, "grok");
                assert_eq!(conversation_id, GROK_CONVERSATION);
                assert_eq!(others.len(), 1);
                assert_eq!(others[0].key.worker_id, WORKER);
                assert_eq!(others[0].worker_harness.as_deref(), Some("claude"));
                assert_eq!(others[0].key.owner, OWNER);
                assert_eq!(others[0].key.session_id, SESSION);
            }
            other => panic!("conversation resume is refused: {other:?}"),
        }
    }

    #[test]
    fn empty_location_keeps_the_recorded_worktree() {
        let mut records = vec![incident_record()];
        let mut later = incident_record();
        later.worktree = None;
        later.branch = Some(" ".to_string());
        later.head = None;
        later.recorded_at_secs = 2;
        upsert_record(&mut records, later);
        assert_eq!(records[0].worktree.as_deref(), Some(WORKTREE));
        assert_eq!(records[0].branch.as_deref(), Some(BRANCH));
        assert_eq!(records[0].head.as_deref(), Some(HEAD));
    }

    #[test]
    fn different_harness_helper_uses_the_owning_session_inbox() {
        assert_eq!(
            spawn_delivery("claude", "grok"),
            SpawnDelivery::OwningSessionInbox
        );
        assert_eq!(
            spawn_delivery("grok", "claude-code"),
            SpawnDelivery::OwningSessionInbox
        );
        assert_eq!(
            spawn_delivery("grok-build", "grok"),
            SpawnDelivery::Native {
                tool: "send_subagent_message"
            }
        );
        assert_eq!(
            spawn_delivery("claude-code", "claude"),
            SpawnDelivery::Native {
                tool: "SendMessage"
            }
        );
        assert_eq!(
            spawn_delivery("codex", "codex"),
            SpawnDelivery::Native {
                tool: "SendMessage"
            }
        );
    }

    #[test]
    fn hand_back_parses_the_generation_two_location() {
        let response = concat!(
            "**Summary**\n\n",
            "Delivery worktree: `/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/addr-286-g2`\n",
            "Head branch: `feat/demo-upstream-overlay`\n",
            "Head: `177f46b753f55f7b37c22706a615ad687544a041`\n",
            "FIX-ON-HEAD https://github.com/haiven-dev/haiven-sdk/pull/286#pullrequestreview-5912901233 head=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        );
        let location = recovery_from_hand_back(response);
        assert_eq!(
            location.worktree.as_deref(),
            Some("/home/brian/work/btakita/worktrees/haiven-dev/haiven-sdk/addr-286-g2")
        );
        assert_eq!(location.branch.as_deref(), Some(BRANCH));
        assert_eq!(location.head.as_deref(), Some(HEAD));
    }
}
