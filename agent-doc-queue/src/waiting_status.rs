//! Waiting-item state and roll-up (`#waitingitemstatus`).
//!
//! Every closeout derives one state per waiting item from the rows its packet
//! owns and the attempt projected on it, and renders it on the item's head as
//! one binary-owned token, `⟦<state> you: N · agent: N · done: N⟧`, right after
//! the head's attempt projection prefix. The token is projection: every parser
//! strips it before identity or work text is read, and the next closeout
//! recomputes it from the same inputs, so a second pass changes nothing.
//!
//! States, in precedence order:
//!
//! - an attempt's own lifecycle glyph (`🚧` running, `🟡` claimed, `⚠️` lost,
//!   `🔄` needs reconcile, `☑️` reported but not yet integrated, ...): a worker
//!   attempt on the item or in its packet is live, so the agent owns the next
//!   step;
//! - `🙋`: the operator owes an answer (the item's own ask, or an open request
//!   inside its packet);
//! - `📥`: the operator answered and no attempt has picked the answer up yet;
//! - `✅`: every row under the item is closed; only the Todo is still open.
//!
//! The counts: `you` open operator asks, `agent` live attempts plus an answer
//! waiting to be picked up, `done` struck rows in the packet.

use std::collections::{BTreeSet, HashSet};

use agent_doc_document::queue_projection::{QueueLifecycleMarker, is_reserved_operator_action_id};

use crate::document_queue::QueueEntry;
use crate::waiting_packet::{WaitingMember, classify_member, packet_member_indexes};

/// Attempt states in the order they win the item's state.
const ATTEMPT_PRECEDENCE: [QueueLifecycleMarker; 9] = [
    QueueLifecycleMarker::Running,
    QueueLifecycleMarker::Claimed,
    QueueLifecycleMarker::LostRunning,
    QueueLifecycleMarker::LostClaimed,
    QueueLifecycleMarker::Lost,
    QueueLifecycleMarker::Unknown,
    QueueLifecycleMarker::Reconciling,
    QueueLifecycleMarker::LostReported,
    QueueLifecycleMarker::Reported,
];

/// `✅`: every row under the item is closed; only its Todo is still open.
pub const WAITING_DONE_GLYPH: &str = "✅";

/// Who owns the item's next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitingState {
    /// A worker attempt is live; the agent owns the next step.
    Attempt(QueueLifecycleMarker),
    /// The operator owes an answer.
    Operator,
    /// The operator answered; no attempt has picked it up yet.
    Answered,
    /// Every row under the item is closed.
    Done,
}

impl WaitingState {
    pub fn glyph(&self) -> String {
        match self {
            Self::Attempt(marker) => marker.prefix().to_string(),
            Self::Operator => {
                agent_doc_document::queue_projection::operator_action_marker().to_string()
            }
            Self::Answered => QueueLifecycleMarker::Pending.prefix().to_string(),
            Self::Done => WAITING_DONE_GLYPH.to_string(),
        }
    }
}

/// The derived state and roll-up of one waiting item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingStatus {
    pub state: WaitingState,
    pub you: usize,
    pub agent: usize,
    pub done: usize,
    /// Open operator requests inside the packet, other than the item itself.
    pub open_requests: Vec<String>,
    /// Whether the item's own ask is still unanswered.
    pub own_ask_open: bool,
}

impl WaitingStatus {
    pub fn token(&self) -> String {
        agent_doc_element_queue::render_waiting_status_token(
            &self.state.glyph(),
            self.you,
            self.agent,
            self.done,
        )
    }
}

fn attempt_marker(text: &str) -> Option<QueueLifecycleMarker> {
    let marker = agent_doc_document::queue_projection::lifecycle_marker(text)?;
    ATTEMPT_PRECEDENCE.contains(&marker).then_some(marker)
}

fn reference_ids(text: &str) -> Vec<String> {
    crate::document_queue::queue_prompt_reference_ids(text)
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .collect()
}

/// Derive the state and roll-up of the live waiting item at `head`.
pub fn waiting_status(
    entries: &[QueueEntry],
    head: usize,
    open_todo_ids: &HashSet<String>,
) -> Option<WaitingStatus> {
    let QueueEntry::Prompt(head_prompt) = entries.get(head)? else {
        return None;
    };
    let own_ids = reference_ids(agent_doc_element_queue::strip_attempt_projection_prefix(
        &head_prompt.text,
    ))
    .into_iter()
    .collect::<HashSet<_>>();

    let mut attempts = Vec::new();
    if let Some(marker) = attempt_marker(&head_prompt.text) {
        attempts.push(marker);
    }
    let mut done = 0usize;
    let mut answered = false;
    let mut open_requests = BTreeSet::new();
    for index in packet_member_indexes(entries, head) {
        match &entries[index] {
            QueueEntry::Completed(_) => done += 1,
            QueueEntry::Prompt(prompt) => {
                match classify_member(prompt) {
                    WaitingMember::Attempt(_) => {
                        if let Some(marker) = attempt_marker(&prompt.text) {
                            attempts.push(marker);
                        } else {
                            attempts.push(QueueLifecycleMarker::Claimed);
                        }
                    }
                    WaitingMember::Operator => {
                        if !crate::queue_consume::is_agent_doc_reply_row(&prompt.text)
                            && !agent_doc_element_queue::struck_work_text(&prompt.text)
                                .trim()
                                .is_empty()
                        {
                            answered = true;
                        }
                    }
                    WaitingMember::BinaryFollowUp | WaitingMember::OfferedChoice => {}
                }
                collect_open_requests(&prompt.text, &own_ids, open_todo_ids, &mut open_requests);
            }
            QueueEntry::Freeform(line) => {
                if crate::waiting_packet::operator_freeform_text(line).is_some() {
                    answered = true;
                }
                if !line.trim_start().trim_start_matches("- ").starts_with("~~") {
                    collect_open_requests(line, &own_ids, open_todo_ids, &mut open_requests);
                }
            }
            _ => {}
        }
    }

    let own_ask_open = !answered && attempts.is_empty() && done == 0;
    let you = usize::from(own_ask_open) + open_requests.len();
    let attempt = ATTEMPT_PRECEDENCE
        .iter()
        .find(|marker| attempts.contains(marker))
        .copied();
    let state = match attempt {
        Some(marker) => WaitingState::Attempt(marker),
        None if you > 0 => WaitingState::Operator,
        None if answered => WaitingState::Answered,
        None => WaitingState::Done,
    };
    let agent = attempts.len() + usize::from(state == WaitingState::Answered);
    Some(WaitingStatus {
        state,
        you,
        agent,
        done,
        open_requests: open_requests.into_iter().collect(),
        own_ask_open,
    })
}

fn collect_open_requests(
    text: &str,
    own_ids: &HashSet<String>,
    open_todo_ids: &HashSet<String>,
    out: &mut BTreeSet<String>,
) {
    for id in reference_ids(text) {
        if is_reserved_operator_action_id(&id)
            && open_todo_ids.contains(&id)
            && !own_ids.contains(&id)
        {
            out.insert(id);
        }
    }
}

/// Render `token` on a head row's first line, right after its attempt
/// projection prefix, replacing any earlier status token.
pub fn with_status_token(text: &str, token: &str) -> String {
    let (line, tail) = text
        .split_once('\n')
        .map_or((text, None), |(line, tail)| (line, Some(tail)));
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(line);
    let prefix = remove_status_tokens(&line[..line.len() - work.len()]);
    let prefix = prefix.trim_end();
    let rendered = if prefix.is_empty() {
        format!("{token} {work}")
    } else {
        format!("{prefix} {token} {work}")
    };
    match tail {
        Some(tail) => format!("{rendered}\n{tail}"),
        None => rendered,
    }
}

/// Remove every waiting status token from a head row's first line.
pub fn without_status_token(text: &str) -> String {
    let (line, tail) = text
        .split_once('\n')
        .map_or((text, None), |(line, tail)| (line, Some(tail)));
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(line);
    let prefix = remove_status_tokens(&line[..line.len() - work.len()]);
    let prefix = prefix.trim_end();
    let rendered = if prefix.is_empty() {
        work.to_string()
    } else {
        format!("{prefix} {work}")
    };
    match tail {
        Some(tail) => format!("{rendered}\n{tail}"),
        None => rendered,
    }
}

fn remove_status_tokens(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len());
    let mut rest = prefix;
    while let Some(at) = rest.find('⟦') {
        out.push_str(&rest[..at]);
        match agent_doc_element_queue::strip_waiting_status_token(&rest[at..]) {
            Some(after) => rest = after.trim_start(),
            None => {
                out.push('⟦');
                rest = &rest[at + '⟦'.len_utf8()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Project the state token onto every live waiting head in one queue body.
pub fn project_waiting_status(
    entries: &[QueueEntry],
    open_todo_ids: &HashSet<String>,
) -> Option<Vec<QueueEntry>> {
    let mut out = entries.to_vec();
    let mut changed = false;
    for head in 0..entries.len() {
        if !matches!(&entries[head], QueueEntry::Prompt(_))
            || !crate::waiting_packet::is_waiting_item_head(&entries[head])
        {
            continue;
        }
        let Some(status) = waiting_status(entries, head, open_todo_ids) else {
            continue;
        };
        let QueueEntry::Prompt(prompt) = &mut out[head] else {
            continue;
        };
        let rendered = with_status_token(&prompt.text, &status.token());
        if rendered != prompt.text {
            prompt.text = rendered;
            changed = true;
        }
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;

    fn open(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn head_line(body: &str, id: &str) -> String {
        body.lines()
            .find(|line| line.contains(&format!("[#{id}]")) && line.contains("waiting-item"))
            .unwrap()
            .to_string()
    }

    fn project(body: &str, ids: &[&str]) -> String {
        let entries = document_queue::parse(body).unwrap();
        project_waiting_status(&entries, &open(ids)).map_or_else(
            || body.to_string(),
            |entries| document_queue::render(&entries),
        )
    }

    #[test]
    fn each_state_is_derived_from_the_packet_and_attempts() {
        let body = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Choose an owner. ▫️ <!-- agent-doc:reply -->\n",
            "  - [#answered] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Pick a port. ▫️ <!-- agent-doc:reply -->\n",
            "      - use 8080\n",
            "  - 🚧 [#subagent:run#1] [#running] Merge the PR. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Merge the PR. ▫️ <!-- agent-doc:reply -->\n",
            "  - [#reported] Renew the license. <!-- agent-doc:waiting-item -->\n",
            "    - ☑️ [#subagent:lic#1] Resolve the open operator action to complete the parent item. [#reported]\n",
            "  - [#closed] Ship the docs. <!-- agent-doc:waiting-item -->\n",
            "    - ~~shipped~~\n",
            "  - [#nested] Choose a release. <!-- agent-doc:waiting-item -->\n",
            "    - go\n",
            "    - **Request**: [#subagent-operator-request-nested-pick]\n",
        );
        let projected = project(
            body,
            &[
                "ask",
                "answered",
                "running",
                "reported",
                "closed",
                "nested",
                "subagent-operator-request-nested-pick",
            ],
        );
        assert!(
            head_line(&projected, "ask")
                .contains("⟦🙋 you: 1 · agent: 0 · done: 0⟧ [#ask] Choose an owner."),
            "{projected}"
        );
        assert!(
            head_line(&projected, "answered")
                .contains("⟦📥 you: 0 · agent: 1 · done: 0⟧ [#answered] Pick a port."),
            "{projected}"
        );
        assert!(
            head_line(&projected, "running")
                .contains("🚧 [#subagent:run#1] ⟦🚧 you: 0 · agent: 1 · done: 0⟧ [#running]"),
            "{projected}"
        );
        assert!(
            head_line(&projected, "reported")
                .contains("⟦☑️ you: 0 · agent: 1 · done: 0⟧ [#reported]"),
            "{projected}"
        );
        assert!(
            head_line(&projected, "closed").contains("⟦✅ you: 0 · agent: 0 · done: 1⟧ [#closed]"),
            "{projected}"
        );
        assert!(
            head_line(&projected, "nested").contains("⟦🙋 you: 1 · agent: 0 · done: 0⟧ [#nested]"),
            "an open request inside an answered packet is still owed: {projected}"
        );
    }

    #[test]
    fn a_second_pass_changes_nothing() {
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Choose an owner. ▫️ <!-- agent-doc:reply -->\n",
            "- 🙋☑️ [#subagent:x#1] [#gated] Merge it. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Merge it. ▫️ <!-- agent-doc:reply -->\n",
        );
        let once = project(body, &["ask", "gated"]);
        assert_ne!(once, body);
        assert_eq!(project(&once, &["ask", "gated"]), once);
        assert!(
            once.contains("🙋☑️ [#subagent:x#1] ⟦☑️ you: 0 · agent: 1 · done: 0⟧ [#gated]"),
            "{once}"
        );
    }

    #[test]
    fn the_token_is_never_identity_or_work_text() {
        let bare = "[#ask] Choose an owner. <!-- agent-doc:waiting-item -->";
        let marked = with_status_token(bare, "⟦🙋 you: 1 · agent: 0 · done: 0⟧");
        assert_eq!(
            crate::queue_claim::claim_identity(&marked),
            crate::queue_claim::claim_identity(bare)
        );
        assert_eq!(
            agent_doc_element_queue::strip_priority_markers(&marked),
            agent_doc_element_queue::strip_priority_markers(bare)
        );
        assert_eq!(agent_doc_element_queue::struck_work_text(&marked), bare);
        assert_eq!(without_status_token(&marked), bare);
        // A changed roll-up replaces the token; it never stacks.
        let replaced = with_status_token(&marked, "⟦✅ you: 0 · agent: 0 · done: 2⟧");
        assert_eq!(replaced, format!("⟦✅ you: 0 · agent: 0 · done: 2⟧ {bare}"));
        // Operator text that merely looks bracketed is never taken as the token.
        let operator = "⟦note⟧ keep this";
        assert_eq!(
            agent_doc_element_queue::strip_attempt_projection_prefix(operator),
            operator
        );
    }
}
