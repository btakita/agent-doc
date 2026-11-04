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
//!
//! The same pass renders the head's first child, a binary-owned reply row
//! `**Next:** <owner> — <action> (evidence: …)` naming the single remaining
//! action derived from that state (`#waitingitemnext`). It is replaced every
//! pass and never read back as input.

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
    /// The item's Todo id.
    pub item_id: Option<String>,
    /// Owner annotation of the attempt that decides the state, and how many
    /// other live attempts there are.
    pub attempt_owner: Option<String>,
    pub other_attempts: usize,
    /// The packet's specific question, or its first action, when present.
    pub question: Option<String>,
    pub action: Option<String>,
    /// The item's own work text.
    pub item_text: String,
    /// The operator's most recent live reply in the packet.
    pub latest_reply: Option<String>,
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

    let head_work = without_status_token(agent_doc_element_queue::strip_attempt_projection_prefix(
        &head_prompt.text,
    ));
    let item_id = head_work
        .strip_prefix("[#")
        .and_then(|rest| rest.split_once(']'))
        .map(|(id, _)| id.to_ascii_lowercase())
        .or_else(|| {
            reference_ids(&head_work)
                .into_iter()
                .find(|id| open_todo_ids.contains(id))
        });
    let item_text = head_work
        .replace(crate::queue_consume::WAITING_ON_YOU_ITEM_MARKER, "")
        .trim()
        .to_string();

    let mut attempts = Vec::<(QueueLifecycleMarker, Option<String>)>::new();
    if let Some(marker) = attempt_marker(&head_prompt.text) {
        attempts.push((marker, owner_label(&head_prompt.text)));
    }
    let mut done = 0usize;
    let mut answered = false;
    let mut latest_reply = None;
    let mut question = None;
    let mut action = None;
    let mut open_requests = BTreeSet::new();
    for index in packet_member_indexes(entries, head) {
        match &entries[index] {
            QueueEntry::Completed(_) => done += 1,
            QueueEntry::Prompt(prompt) => {
                match classify_member(prompt) {
                    WaitingMember::Attempt(_) => {
                        let marker =
                            attempt_marker(&prompt.text).unwrap_or(QueueLifecycleMarker::Claimed);
                        attempts.push((marker, owner_label(&prompt.text)));
                    }
                    WaitingMember::Operator => {
                        let reply = agent_doc_element_queue::struck_work_text(&prompt.text).trim();
                        if !crate::queue_consume::is_agent_doc_reply_row(&prompt.text)
                            && !reply.is_empty()
                        {
                            answered = true;
                            latest_reply = Some(without_generated_id(reply));
                        }
                    }
                    WaitingMember::BinaryFollowUp | WaitingMember::OfferedChoice => {}
                }
                collect_open_requests(&prompt.text, &own_ids, open_todo_ids, &mut open_requests);
            }
            QueueEntry::Freeform(line) => {
                // This module's own Next row is output, never input.
                if is_next_row(line) {
                    continue;
                }
                if let Some(reply) = crate::waiting_packet::operator_freeform_text(line) {
                    answered = true;
                    latest_reply = Some(without_generated_id(reply));
                }
                if let Some(value) = metadata_value(line, "**Question:**")
                    && value != GENERIC_QUESTION
                {
                    question.get_or_insert(value);
                }
                if let Some(value) = metadata_value(line, "**Action:**") {
                    action.get_or_insert(value);
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
        .find(|marker| attempts.iter().any(|(candidate, _)| candidate == *marker))
        .copied();
    let attempt_owner = attempt.and_then(|marker| {
        attempts
            .iter()
            .find(|(candidate, _)| *candidate == marker)
            .and_then(|(_, owner)| owner.clone())
    });
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
        item_id,
        attempt_owner,
        other_attempts: attempts.len().saturating_sub(1),
        question,
        action,
        item_text,
        latest_reply,
    })
}

/// The packet placeholder question carries no decision of its own.
const GENERIC_QUESTION: &str = "What decision or completion result should be recorded?";

/// The longest action or evidence text the Next row quotes.
const NEXT_QUOTE_CHARS: usize = 140;

/// Label of the binary-owned Next row.
pub const NEXT_ROW_LABEL: &str = "**Next:**";

fn owner_label(text: &str) -> Option<String> {
    agent_doc_document::queue_projection::owner_projection(text)
        .map(|owner| format!("{}#{}", owner.owner, owner.generation))
}

/// Remove a trailing binary `[#q-…]` id from quoted operator text.
fn without_generated_id(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.rsplit_once(" [#q-") {
        Some((work, id))
            if id.strip_suffix(']').is_some_and(|hash| {
                !hash.is_empty() && hash.chars().all(|ch| ch.is_ascii_hexdigit())
            }) =>
        {
            work.trim_end().to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// The value of a generated `- **Label:** value` metadata row.
fn metadata_value(line: &str, label: &str) -> Option<String> {
    let body = line.lines().next()?.trim_start().strip_prefix("- ")?.trim();
    let value = body.strip_prefix(label)?;
    let value = value
        .replace(crate::queue_consume::AGENT_DOC_REPLY_MARKER, "")
        .replace(crate::queue_consume::LEGACY_AGENT_DOC_REPLY_MARKER, "")
        .replace(crate::queue_consume::AGENT_DOC_REPLY_PRESENTATION, "");
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Whether `line` is this module's binary-owned Next row.
pub fn is_next_row(line: &str) -> bool {
    line.lines().next().is_some_and(|first| {
        first
            .trim_start()
            .strip_prefix("- ")
            .is_some_and(|body| body.starts_with(NEXT_ROW_LABEL))
            && crate::queue_consume::is_agent_doc_reply_row(first)
    })
}

/// `text` without binary `[#id]` / `⟦...⟧` annotations, whitespace-normalized,
/// for comparing a packet row with the item's own text.
fn core_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let next = [rest.find("[#"), rest.find('⟦')]
            .into_iter()
            .flatten()
            .min();
        let Some(at) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        let close = if rest[at..].starts_with("[#") {
            "]"
        } else {
            "⟧"
        };
        match rest[at..].find(close) {
            Some(end) => rest = &rest[at + end + close.len()..],
            None => {
                out.push_str(&rest[at..]);
                break;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn quote(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= NEXT_QUOTE_CHARS {
        return flat;
    }
    let mut cut = flat.chars().take(NEXT_QUOTE_CHARS).collect::<String>();
    if let Some(space) = cut.rfind(' ') {
        cut.truncate(space);
    }
    format!("{}…", cut.trim_end())
}

impl WaitingStatus {
    /// The single remaining action, its owner, and the evidence it was derived
    /// from, as the body of the item's Next row.
    pub fn next_action(&self) -> String {
        let item = self
            .item_id
            .as_deref()
            .map(|id| format!("`#{id}`"))
            .unwrap_or_else(|| "this item".to_string());
        let others = match self.other_attempts {
            0 => String::new(),
            1 => " and 1 more attempt".to_string(),
            count => format!(" and {count} more attempts"),
        };
        let attempt = self
            .attempt_owner
            .as_deref()
            .map(|owner| format!("`{owner}`"))
            .unwrap_or_else(|| "the attempt".to_string());
        match &self.state {
            WaitingState::Attempt(marker) => {
                let verb = match marker {
                    QueueLifecycleMarker::Running | QueueLifecycleMarker::Claimed => {
                        "let the running attempt finish and report"
                    }
                    QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported
                        if self.other_attempts > 0 =>
                    {
                        "integrate the reported results, then close the item or ask what remains"
                    }
                    QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported => {
                        "integrate the reported result, then close the item or ask what remains"
                    }
                    QueueLifecycleMarker::Reconciling => {
                        "reconcile the recorded completion, then close the item"
                    }
                    QueueLifecycleMarker::Unknown => "diagnose the attempt's conflicting state",
                    _ => "recover the lost attempt's result, or retry it",
                };
                format!(
                    "agent — {verb} (evidence: {} {attempt}{others})",
                    marker.prefix()
                )
            }
            WaitingState::Operator if self.own_ask_open => {
                // Quote the packet's specific ask; never repeat the head's own
                // text, which the operator is already reading.
                let item = core_text(&self.item_text);
                let same_as_item = |text: &str| core_text(text) == item;
                if let Some(question) = self.question.as_deref().filter(|q| !same_as_item(q)) {
                    format!("you — answer: {} (evidence: Question)", quote(question))
                } else if let Some(action) = self.action.as_deref().filter(|a| !same_as_item(a)) {
                    format!("you — {} (evidence: Action)", quote(action))
                } else {
                    "you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet)".to_string()
                }
            }
            WaitingState::Operator => {
                let requests = self
                    .open_requests
                    .iter()
                    .map(|id| format!("`#{id}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "you — answer the open request {requests} (evidence: open request in this item's packet)"
                )
            }
            WaitingState::Answered => {
                let reply = self.latest_reply.as_deref().unwrap_or_default();
                format!(
                    "agent — pick up your reply “{}” and attempt {item} (evidence: your reply)",
                    quote(reply)
                )
            }
            WaitingState::Done => format!(
                "you — check off {item} in Todo, or reply with what remains (evidence: every row under the item is closed)"
            ),
        }
    }

    /// The item's Next row, rendered at `indent`.
    pub fn next_row(&self, indent: usize) -> String {
        format!(
            "{}- {NEXT_ROW_LABEL} {} {} {}",
            " ".repeat(indent),
            self.next_action(),
            crate::queue_consume::AGENT_DOC_REPLY_PRESENTATION,
            crate::queue_consume::AGENT_DOC_REPLY_MARKER
        )
    }
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

/// Project the state token and the Next row onto every live waiting head in
/// one queue body. The Next row is the head's first child; any earlier Next
/// row the item owns is replaced, and a struck head keeps none.
pub fn project_waiting_status(
    entries: &[QueueEntry],
    open_todo_ids: &HashSet<String>,
) -> Option<Vec<QueueEntry>> {
    let mut out = Vec::with_capacity(entries.len() + 8);
    let mut index = 0usize;
    while index < entries.len() {
        let entry = &entries[index];
        if !crate::waiting_packet::is_waiting_item_head(entry) {
            out.push(entry.clone());
            index += 1;
            continue;
        }
        let members = packet_member_indexes(entries, index)
            .into_iter()
            .collect::<HashSet<_>>();
        let end = crate::waiting_packet::subtree_end(entries, index);
        match (entry, waiting_status(entries, index, open_todo_ids)) {
            (QueueEntry::Prompt(prompt), Some(status)) => {
                let mut head = prompt.clone();
                head.text = with_status_token(&prompt.text, &status.token());
                out.push(QueueEntry::Prompt(head));
                out.push(QueueEntry::Freeform(status.next_row(prompt.indent + 2)));
            }
            _ => out.push(entry.clone()),
        }
        // Copy the subtree, dropping this item's previous Next rows. A
        // nested item's rows are copied by its own turn of this loop.
        let mut cursor = index + 1;
        while cursor < end {
            if crate::waiting_packet::is_waiting_item_head(&entries[cursor]) {
                break;
            }
            let stale_next = members.contains(&cursor)
                && matches!(&entries[cursor], QueueEntry::Freeform(line) if is_next_row(line));
            if !stale_next {
                out.push(entries[cursor].clone());
            }
            cursor += 1;
        }
        index = cursor;
    }
    (out != entries).then_some(out)
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

    fn next_line(body: &str, id: &str) -> String {
        let lines = body.lines().collect::<Vec<_>>();
        let head = lines
            .iter()
            .position(|line| line.contains(&format!("[#{id}]")) && line.contains("waiting-item"))
            .unwrap();
        lines[head + 1].to_string()
    }

    #[test]
    fn each_item_gets_one_next_row_naming_owner_action_and_evidence() {
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Choose an owner. ▫️ <!-- agent-doc:reply -->\n",
            "    - **Question:** Who owns the alpha release? ▫️ <!-- agent-doc:reply -->\n",
            "- [#generic] Renew the license. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Renew the license. ▫️ <!-- agent-doc:reply -->\n",
            "    - **Question:** What decision or completion result should be recorded? ▫️ <!-- agent-doc:reply -->\n",
            "    - **Next actions:** ▫️ <!-- agent-doc:reply -->\n",
            "      - **Action:** Renew the license. ▫️ <!-- agent-doc:reply -->\n",
            "- [#answered] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "  - use 8080 [#q-0123456789ab]\n",
            "- 🚧 [#subagent:run#1] [#running] Merge the PR. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Merge the PR. ▫️ <!-- agent-doc:reply -->\n",
            "- [#closed] Ship the docs. <!-- agent-doc:waiting-item -->\n",
            "  - ~~shipped~~\n",
        );
        let projected = project(body, &["ask", "generic", "answered", "running", "closed"]);
        let reply = " ▫️ <!-- agent-doc:reply -->";
        assert_eq!(
            next_line(&projected, "ask"),
            format!(
                "  - **Next:** you — answer: Who owns the alpha release? (evidence: Question){reply}"
            )
        );
        assert_eq!(
            next_line(&projected, "generic"),
            format!(
                "  - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet){reply}"
            ),
            "the head's own text is never repeated as its next action"
        );
        assert_eq!(
            next_line(&projected, "answered"),
            format!(
                "  - **Next:** agent — pick up your reply “use 8080” and attempt `#answered` (evidence: your reply){reply}"
            )
        );
        assert_eq!(
            next_line(&projected, "running"),
            format!(
                "  - **Next:** agent — let the running attempt finish and report (evidence: 🚧 `subagent:run#1`){reply}"
            )
        );
        assert_eq!(
            next_line(&projected, "closed"),
            format!(
                "  - **Next:** you — check off `#closed` in Todo, or reply with what remains (evidence: every row under the item is closed){reply}"
            )
        );
        assert_eq!(projected.matches("**Next:**").count(), 5);
        assert_eq!(
            project(
                &projected,
                &["ask", "generic", "answered", "running", "closed"]
            ),
            projected
        );
        // The Next row is inert reply metadata, never queue work.
        let entries = document_queue::parse(&projected).unwrap();
        assert!(
            document_queue::prompts(&entries)
                .iter()
                .all(|prompt| !prompt.text.contains("**Next:**"))
        );
    }

    #[test]
    fn a_stale_next_row_is_replaced_and_never_read_back_as_evidence() {
        // The previous Next named a request that has since closed. Reading the
        // binary's own row back would keep that request "open" forever.
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Next:** you — answer the open request [#subagent-operator-request-gone] (evidence: open request in this item's packet) ▫️ <!-- agent-doc:reply -->\n",
            "  - **Response:** Choose an owner. ▫️ <!-- agent-doc:reply -->\n",
            "    - **Question:** Who owns the alpha release? ▫️ <!-- agent-doc:reply -->\n",
        );
        let projected = project(body, &["ask", "subagent-operator-request-gone"]);
        assert_eq!(projected.matches("**Next:**").count(), 1, "{projected}");
        assert!(!projected.contains("request-gone"), "{projected}");
        assert!(
            projected.contains("⟦🙋 you: 1 · agent: 0 · done: 0⟧ [#ask]"),
            "{projected}"
        );
    }
}
