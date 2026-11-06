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
//! - complete: every row under the item is closed and no offered decision is
//!   open, but its Todo is still open. The open Todo is the operator's, so it
//!   counts as `you: 1` and the head shows the `🙋` gate, never `✅`
//!   (`#waitingdonetodoopen`); a response closeout checks such a Todo off
//!   ([`complete_waiting_todo_ids`]) and the item leaves the tree;
//! - `✅`: every row under the item is closed and its Todo is closed too.
//!
//! The counts: `you` open operator asks (the item's own ask, open requests,
//! offered decisions nobody acted on, and a complete item's still-open Todo),
//! `agent` live attempts plus an answer waiting to be picked up, `done` struck
//! rows in the packet.
//!
//! The same pass renders the head's first child, a binary-owned reply row
//! `**Next:** <owner> — <action> (evidence: …)` naming the single remaining
//! action derived from that state (`#waitingitemnext`). It is replaced every
//! pass and never read back as input.
//!
//! The Next row names an open operator ask first, even while attempts are
//! live. An item with several decision subtrees also gets nested `**Roll-up:**`
//! and `**Subtree:**` rows derived by [`crate::waiting_subtree`]
//! (`#waitingtriage`); they are output exactly like the Next row.

use std::collections::{BTreeSet, HashSet};

use agent_doc_document::queue_projection::{QueueLifecycleMarker, is_reserved_operator_action_id};

use crate::document_queue::QueueEntry;
use crate::waiting_packet::{WaitingMember, classify_member, packet_member_indexes};
use crate::waiting_subtree::{OpenAsk, WaitingTriage};

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

/// `✅`: every row under the item is closed and its Todo is closed too.
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
    /// Every row under the item is closed, but its Todo is still open
    /// (`#waitingdonetodoopen`): the closeout checks it off.
    Complete,
    /// Every row under the item is closed and its Todo is closed.
    Done,
}

impl WaitingState {
    pub fn glyph(&self) -> String {
        match self {
            Self::Attempt(marker) => marker.prefix().to_string(),
            Self::Operator | Self::Complete => {
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
    /// Offered decisions inside the packet nobody has acted on yet
    /// (`#waitingdonetodoopen`).
    pub open_choices: Vec<OpenAsk>,
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

    /// The token as rendered on the head. The `🙋` state is the one the
    /// head's open-Todo operator-action gate already shows, so the token omits
    /// it and keeps the counts (`⟦you: 1 · agent: 0 · done: 0⟧`); one row
    /// never shows `🙋 ⟦🙋 …⟧` (`#managedlistnodrift`). The form depends only
    /// on the derived state, never on whether the gate pass already ran, so it
    /// is one fixed point in either pass order. Both forms parse as the same
    /// projection token.
    pub fn head_token(&self) -> String {
        if matches!(self.state, WaitingState::Operator | WaitingState::Complete) {
            return agent_doc_element_queue::render_waiting_status_token(
                "", self.you, self.agent, self.done,
            );
        }
        self.token()
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
                        // A bold `**label**` row (`**Consumed by:**`, a
                        // `**Response [#r-…]**` link) is generated, never a reply.
                        if !crate::waiting_packet::is_agent_doc_reply_entry(entries, index)
                            && !reply.is_empty()
                            && !reply.starts_with("**")
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
                // This module's own Next, Roll-up, and Subtree rows are
                // output, never input.
                if is_status_row(entries, index) {
                    continue;
                }
                if let Some(reply) = crate::waiting_packet::operator_freeform_text(entries, index) {
                    answered = true;
                    latest_reply = Some(without_generated_id(reply));
                }
                // `#answerack`: a consumed (struck) operator answer is closed
                // packet work, exactly like a struck prompt row.
                if crate::waiting_packet::is_struck_operator_freeform(entries, index) {
                    done += 1;
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
    // `#waitingdonetodoopen`: an offered decision nobody acted on is still an
    // ask. While the item's own ask is open its options are that ask.
    let open_choices = if own_ask_open {
        Vec::new()
    } else {
        crate::waiting_subtree::open_choice_asks(entries, head, head)
    };
    let mut you = usize::from(own_ask_open) + open_requests.len() + open_choices.len();
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
    // The item is the view of its Todo; one without a resolvable id is
    // treated as open, so it never reads `✅`.
    let todo_open = item_id.as_ref().is_none_or(|id| open_todo_ids.contains(id));
    let state = match attempt {
        Some(marker) => WaitingState::Attempt(marker),
        None if you > 0 => WaitingState::Operator,
        None if answered => WaitingState::Answered,
        None if todo_open => {
            // The open Todo is the operator's last open row.
            you += 1;
            WaitingState::Complete
        }
        None => WaitingState::Done,
    };
    let agent = attempts.len() + usize::from(state == WaitingState::Answered);
    Some(WaitingStatus {
        state,
        you,
        agent,
        done,
        open_requests: open_requests.into_iter().collect(),
        open_choices,
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

/// Label of the binary-owned roll-up row beneath a triaged item's Next row
/// (`#waitingtriage`).
pub const ROLLUP_ROW_LABEL: &str = "**Roll-up:**";

/// Label of a binary-owned per-subtree row beneath a triaged item's Next row
/// (`#waitingtriage`).
pub const SUBTREE_ROW_LABEL: &str = "**Subtree:**";

/// Waiting items with at least this many decision subtrees render the
/// Roll-up row and one Subtree row per decision; below it the head's Next
/// row already speaks for the whole item.
pub const TRIAGE_MIN_SUBTREES: usize = 2;

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
    body.strip_prefix(label)?;
    let text = agent_doc_markdown_ast::reply_row::reply_row_text(body);
    let value = text.strip_prefix(label)?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn is_next_body(body: &str) -> bool {
    body.starts_with(NEXT_ROW_LABEL)
}

/// Whether `entries[index]` is this module's binary-owned Next row: a
/// `**Next:**` reply row under the shared `#barereplymarker` predicate.
pub fn is_next_row(entries: &[QueueEntry], index: usize) -> bool {
    matches!(entries.get(index), Some(QueueEntry::Freeform(line)) if line
        .lines()
        .next()
        .and_then(|first| first.trim_start().strip_prefix("- "))
        .is_some_and(is_next_body))
        && crate::waiting_packet::is_agent_doc_reply_entry(entries, index)
}

/// Whether `entries[index]` is any row this module renders: the Next row or a
/// Roll-up / Subtree row beneath it (`#waitingtriage`). Never read as input.
pub fn is_status_row(entries: &[QueueEntry], index: usize) -> bool {
    matches!(entries.get(index), Some(QueueEntry::Freeform(line)) if line
        .lines()
        .next()
        .and_then(|first| first.trim_start().strip_prefix("- "))
        .is_some_and(|body| is_next_body(body)
            || body.starts_with(ROLLUP_ROW_LABEL)
            || body.starts_with(SUBTREE_ROW_LABEL)))
        && crate::waiting_packet::is_agent_doc_reply_entry(entries, index)
}

/// [`is_next_row`] over markdown source lines.
pub fn is_next_row_at(lines: &[&str], index: usize) -> bool {
    lines
        .get(index)
        .and_then(|line| line.trim_start().strip_prefix("- "))
        .is_some_and(is_next_body)
        && agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_at(lines, index)
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
            WaitingState::Operator if self.open_requests.is_empty() => {
                let ask = crate::waiting_subtree::ask_text(&self.open_choices);
                format!("you — {ask} (evidence: open choice in this item's packet)")
            }
            WaitingState::Operator => {
                let requests = self
                    .open_requests
                    .iter()
                    .map(|id| format!("`#{id}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let choices = if self.open_choices.is_empty() {
                    String::new()
                } else {
                    format!("; {}", crate::waiting_subtree::ask_text(&self.open_choices))
                };
                format!(
                    "you — answer the open request {requests}{choices} (evidence: open request in this item's packet)"
                )
            }
            WaitingState::Answered => {
                let reply = self.latest_reply.as_deref().unwrap_or_default();
                format!(
                    "agent — pick up your reply “{}” and attempt {item} (evidence: your reply)",
                    quote(reply)
                )
            }
            WaitingState::Complete if self.item_id.is_some() => format!(
                "you — nothing is left but the Todo: the next response closeout checks off {item}; reply under this item if anything remains (evidence: every row under the item is closed)"
            ),
            WaitingState::Complete => format!(
                "you — check off {item} in Todo, or reply with what remains (evidence: every row under the item is closed)"
            ),
            WaitingState::Done => format!(
                "agent — nothing is owed; {item} is closed in Todo (evidence: every row under the item is closed)"
            ),
        }
    }

    /// The item's Next row, rendered at `indent`.
    pub fn next_row(&self, indent: usize) -> String {
        format!(
            "{}- {NEXT_ROW_LABEL} {} {}",
            " ".repeat(indent),
            self.next_action(),
            crate::queue_consume::AGENT_DOC_REPLY_GLYPH
        )
    }

    /// The Next action informed by the item's triage (`#waitingtriage`): an
    /// open operator ask is surfaced first even while attempts are live, each
    /// ask names the options offered for it, and agent-owned debt idle for
    /// over a day is flagged.
    pub fn triaged_next_action(&self, triage: &WaitingTriage) -> String {
        let asks = &triage.item.open_asks;
        let ask = (!asks.is_empty()).then(|| crate::waiting_subtree::ask_text(asks));
        let mut action = match (&self.state, ask) {
            (WaitingState::Attempt(_), Some(ask)) if self.you > 0 => {
                let agent = self.next_action();
                let agent = agent.strip_prefix("agent — ").unwrap_or(&agent);
                format!(
                    "you — {ask} (evidence: open request in this item's packet); then agent — {agent}"
                )
            }
            (WaitingState::Operator, Some(ask)) if !self.own_ask_open => {
                format!("you — {ask} (evidence: open request in this item's packet)")
            }
            _ => self.next_action(),
        };
        if triage.item.stale {
            action.push_str(&triage.item.stale_note());
        }
        action
    }

    /// The Roll-up row body: each count of the head token spelled out.
    pub fn rollup_text(&self) -> String {
        let answered = usize::from(self.state == WaitingState::Answered);
        let attempts = self.agent - answered;
        let mut asks = Vec::new();
        if self.own_ask_open {
            asks.push("this item's own ask".to_string());
        }
        asks.extend(self.open_requests.iter().map(|id| format!("`#{id}`")));
        asks.extend(self.open_choices.iter().map(|ask| {
            let labels = ask
                .choices
                .iter()
                .map(|label| format!("**{label}**"))
                .collect::<Vec<_>>()
                .join(" / ");
            format!("open choice {labels}")
        }));
        if self.state == WaitingState::Complete {
            asks.push("this item's open Todo".to_string());
        }
        let asks = if asks.is_empty() {
            String::new()
        } else {
            format!(" ({})", asks.join(", "))
        };
        format!(
            "you {} = open asks to you{asks} · agent {} = {attempts} worker {} not yet integrated + {answered} {} not yet picked up · done {} = struck rows",
            self.you,
            self.agent,
            if attempts == 1 { "result" } else { "results" },
            if answered == 1 { "answer" } else { "answers" },
            self.done,
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

/// Todo ids of the live waiting items in one queue body whose tree is complete
/// while their Todo is still open (`#waitingdonetodoopen`). A response
/// closeout checks each off instead of leaving a Next row that asks the
/// operator to tick its box.
pub fn complete_waiting_todo_ids(
    entries: &[QueueEntry],
    open_todo_ids: &HashSet<String>,
) -> Vec<String> {
    (0..entries.len())
        .filter(|index| {
            matches!(&entries[*index], QueueEntry::Prompt(_))
                && crate::waiting_packet::is_waiting_item_head(&entries[*index])
        })
        .filter_map(|index| waiting_status(entries, index, open_todo_ids))
        .filter(|status| status.state == WaitingState::Complete)
        .filter_map(|status| status.item_id)
        .filter(|id| open_todo_ids.contains(id))
        .collect()
}

/// Project the state token and the Next row onto every live waiting head in
/// one queue body. The Next row is the head's first child; any earlier Next
/// row the item owns is replaced, and a struck head keeps none.
///
/// An item with [`TRIAGE_MIN_SUBTREES`] or more decision subtrees
/// (`#waitingtriage`) also gets, beneath its Next row, one Roll-up row that
/// spells out the token's counts and one Subtree row per decision naming what
/// that part needs (`Next: you — …`, `Next: agent — …`, or `🗑️ deletable`).
/// `as_of` (minutes since the Unix epoch; the document's newest response
/// time) decides which agent-owned debt is stale; `None` flags none.
pub fn project_waiting_status(
    entries: &[QueueEntry],
    open_todo_ids: &HashSet<String>,
    as_of: Option<i64>,
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
                head.text = with_status_token(&prompt.text, &status.head_token());
                out.push(QueueEntry::Prompt(head));
                let triage =
                    crate::waiting_subtree::waiting_triage(entries, index, open_todo_ids, as_of);
                let glyph = crate::queue_consume::AGENT_DOC_REPLY_GLYPH;
                let indent = prompt.indent + 2;
                out.push(QueueEntry::Freeform(format!(
                    "{}- {NEXT_ROW_LABEL} {} {glyph}",
                    " ".repeat(indent),
                    status.triaged_next_action(&triage)
                )));
                if triage.subtrees.len() >= TRIAGE_MIN_SUBTREES {
                    out.push(QueueEntry::Freeform(format!(
                        "{}- {ROLLUP_ROW_LABEL} {} {glyph}",
                        " ".repeat(indent + 2),
                        status.rollup_text()
                    )));
                    for (depth, subtree) in &triage.subtrees {
                        out.push(QueueEntry::Freeform(format!(
                            "{}- {SUBTREE_ROW_LABEL} {} {glyph}",
                            " ".repeat(indent + 2 + 2 * depth),
                            subtree.summary()
                        )));
                    }
                }
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
            if members.contains(&cursor) && is_status_row(entries, cursor) {
                // The previous Next row and the triage rows nested beneath it.
                let mut skip = cursor + 1;
                while skip < end && is_status_row(entries, skip) {
                    skip += 1;
                }
                cursor = skip;
                continue;
            }
            out.push(entries[cursor].clone());
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
        project_waiting_status(&entries, &open(ids), None).map_or_else(
            || body.to_string(),
            |entries| document_queue::render(&entries),
        )
    }

    /// `#managedlistnodrift` regression: a head whose open Todo already shows
    /// the `🙋` gate rendered `🙋 ⟦🙋 you: 1 · agent: 0 · done: 0⟧ …`, the
    /// state twice. The gate is the state now; the token keeps the counts, it
    /// still parses as projection (never identity), and a second pass is a
    /// no-op.
    #[test]
    fn an_operator_gate_head_shows_its_state_once() {
        let body = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - 🙋 Run the sample smoke test on a real workspace <!-- agent-doc:waiting-item --> [#samplesmoke]\n",
            "    - **Response:** Run the sample smoke test. ▫️\n",
        );
        let once = project(body, &["samplesmoke"]);
        let head = once.lines().nth(1).unwrap().trim_start();
        assert!(
            head.starts_with("- 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ Run the sample smoke test"),
            "{once}"
        );
        // The same state without the gate rendered yet (the gate pass may run
        // after this one) renders the same token: one fixed point.
        let bare = body.replacen("  - 🙋 Run", "  - Run", 1);
        assert!(
            project(&bare, &["samplesmoke"])
                .contains("  - ⟦you: 1 · agent: 0 · done: 0⟧ Run the sample smoke test"),
            "{}",
            project(&bare, &["samplesmoke"])
        );
        assert_eq!(head.matches('🙋').count(), 1, "{once}");
        assert_eq!(
            project(&once, &["samplesmoke"]),
            once,
            "a second pass is a no-op"
        );
        let work = head.trim_start_matches("- ");
        assert_eq!(
            agent_doc_element_queue::strip_attempt_projection_prefix(work),
            "Run the sample smoke test on a real workspace <!-- agent-doc:waiting-item --> [#samplesmoke]"
        );
        // An answered item without the gate keeps the full token.
        let answered = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#picked] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "    - use 8080\n",
        );
        assert!(
            project(answered, &["picked"]).contains("⟦📥 you: 0 · agent: 1 · done: 0⟧ [#picked]")
        );
    }

    #[test]
    fn each_state_is_derived_from_the_packet_and_attempts() {
        let body = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Choose an owner. ▫️\n",
            "  - [#answered] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Pick a port. ▫️\n",
            "      - use 8080\n",
            "  - 🚧 [#subagent:run#1] [#running] Merge the PR. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Merge the PR. ▫️\n",
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
                .contains("⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Choose an owner."),
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
        // `#waitingdonetodoopen`: a complete tree whose Todo is still open
        // counts that Todo as the operator's and never reads `✅`.
        assert!(
            head_line(&projected, "closed").contains("⟦you: 1 · agent: 0 · done: 1⟧ [#closed]"),
            "{projected}"
        );
        assert!(
            head_line(&projected, "nested").contains("⟦you: 1 · agent: 0 · done: 0⟧ [#nested]"),
            "an open request inside an answered packet is still owed: {projected}"
        );
    }

    #[test]
    fn a_second_pass_changes_nothing() {
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Choose an owner. ▫️\n",
            "- 🙋☑️ [#subagent:x#1] [#gated] Merge it. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Merge it. ▫️\n",
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
            "  - **Response:** Choose an owner. ▫️\n",
            "    - **Question:** Who owns the alpha release? ▫️\n",
            "- [#generic] Renew the license. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Renew the license. ▫️\n",
            "    - **Question:** What decision or completion result should be recorded? ▫️\n",
            "    - **Next actions:** ▫️\n",
            "      - **Action:** Renew the license. ▫️\n",
            "- [#answered] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "  - use 8080 [#q-0123456789ab]\n",
            "- 🚧 [#subagent:run#1] [#running] Merge the PR. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Merge the PR. ▫️\n",
            "- [#closed] Ship the docs. <!-- agent-doc:waiting-item -->\n",
            "  - ~~shipped~~\n",
        );
        let projected = project(body, &["ask", "generic", "answered", "running", "closed"]);
        let reply = " ▫️";
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
                "  - **Next:** you — nothing is left but the Todo: the next response closeout checks off `#closed`; reply under this item if anything remains (evidence: every row under the item is closed){reply}"
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
            "  - **Next:** you — answer the open request [#subagent-operator-request-gone] (evidence: open request in this item's packet) ▫️\n",
            "  - **Response:** Choose an owner. ▫️\n",
            "    - **Question:** Who owns the alpha release? ▫️\n",
        );
        let projected = project(body, &["ask", "subagent-operator-request-gone"]);
        assert_eq!(projected.matches("**Next:**").count(), 1, "{projected}");
        assert!(!projected.contains("request-gone"), "{projected}");
        assert!(
            projected.contains("⟦you: 1 · agent: 0 · done: 0⟧ [#ask]"),
            "{projected}"
        );
    }

    /// `#barereplymarker`: a legacy Next row that still carries the hidden
    /// reply comment is recognized as this module's output, dropped, and
    /// re-rendered with only the bare trailing glyph.
    #[test]
    fn legacy_commented_next_row_is_replaced_by_the_bare_form() {
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Next:** stale text ▫️ <!-- agent-doc:reply -->\n",
            "  - **Response:** Choose an owner. ▫️ <!-- agent-doc:reply -->\n",
            "    - **Question:** Who owns the alpha release? ▫️ <!-- agent-doc:reply -->\n",
        );
        let projected = project(body, &["ask"]);
        assert!(!projected.contains("stale text"), "{projected}");
        assert_eq!(projected.matches("**Next:**").count(), 1, "{projected}");
        assert_eq!(
            next_line(&projected, "ask"),
            "  - **Next:** you — answer: Who owns the alpha release? (evidence: Question) ▫️"
        );
    }

    /// `#waitingtriage`: triage rows are output. When the item shrinks to one
    /// decision, the earlier Roll-up and Subtree rows (one naming a request
    /// that has since closed) are removed with the old Next row, never read
    /// back as an open ask, and never left behind.
    #[test]
    fn stale_triage_rows_are_dropped_with_their_next_row() {
        let body = concat!(
            "- [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
            "  - **Next:** stale ▫️\n",
            "    - **Roll-up:** you 2 = stale ▫️\n",
            "    - **Subtree:** “gone” — Next: you — answer the open request `#subagent-operator-request-gone` ▫️\n",
            "  - **Response:** Choose an owner. ▫️\n",
            "    - **Question:** Who owns the alpha release? ▫️\n",
        );
        let projected = project(body, &["ask", "subagent-operator-request-gone"]);
        assert!(!projected.contains("Roll-up"), "{projected}");
        assert!(!projected.contains("Subtree"), "{projected}");
        assert!(!projected.contains("request-gone"), "{projected}");
        assert!(
            projected.contains("⟦you: 1 · agent: 0 · done: 0⟧ [#ask]"),
            "{projected}"
        );
        assert_eq!(
            next_line(&projected, "ask"),
            "  - **Next:** you — answer: Who owns the alpha release? (evidence: Question) ▫️"
        );
    }

    /// `#waitingtriage`: two decisions under one item get a Roll-up row and
    /// one Subtree row each; an open ask leads the head's Next row even while
    /// an attempt is live, and debt idle for over a day is flagged.
    #[test]
    fn a_two_decision_item_renders_its_triage_rows_once() {
        let body = concat!(
            "- [#rel] Release App access <!-- agent-doc:waiting-item -->\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Timestamp:** 2026-11-02T00:20:07.000Z ▫️\n",
            "  - pick a reviewer\n",
            "    - **Response [#r-0123456789ac]**\n",
            "      - **Choices:**\n",
            "        - **Request**: [#subagent-operator-request-reviewer]\n",
            "        - **Luke**: Luke reviews.\n",
            "        - **Dillon**: Dillon reviews.\n",
            "  - ☑️ [#subagent:grant#1] grant the repo [#q-0123456789ab]\n",
        );
        let entries = document_queue::parse(body).unwrap();
        let as_of = crate::waiting_subtree::parse_timestamp_minutes("2026-11-04T19:08-06:00")
            .map(|(minutes, _)| minutes);
        let ids = open(&["rel", "subagent-operator-request-reviewer"]);
        let once = document_queue::render(&project_waiting_status(&entries, &ids, as_of).unwrap());
        let stale = "⏰ stale: agent-owned, no recorded activity since 2026-11-02 (over a day)";
        let ask = "answer the open request `#subagent-operator-request-reviewer`: choose **Luke** or **Dillon**, or reply with another";
        let lines = once.lines().collect::<Vec<_>>();
        assert_eq!(
            lines[1],
            format!(
                "  - **Next:** you — {ask} (evidence: open request in this item's packet); then agent — integrate the reported result, then close the item or ask what remains (evidence: ☑️ `subagent:grant#1`) {stale} ▫️"
            ),
            "{once}"
        );
        assert_eq!(
            lines[2],
            "    - **Roll-up:** you 1 = open asks to you (`#subagent-operator-request-reviewer`) · agent 1 = 1 worker result not yet integrated + 0 answers not yet picked up · done 0 = struck rows ▫️"
        );
        assert_eq!(
            lines[3],
            format!("    - **Subtree:** “pick a reviewer” — Next: you — {ask} ▫️")
        );
        assert_eq!(
            lines[4],
            format!(
                "    - **Subtree:** “grant the repo `#q-0123456789ab`” — Next: agent — integrate 1 reported result (`subagent:grant#1`) {stale} ▫️"
            )
        );
        let again = document_queue::parse(&once).unwrap();
        assert!(
            project_waiting_status(&again, &ids, as_of).is_none(),
            "fixed point"
        );
        // Once the attempt is integrated (struck), its subtree is deletable.
        assert!(
            lines[0].contains("⟦☑️ you: 1 · agent: 1 · done: 0⟧ [#rel]"),
            "{once}"
        );
        let integrated = once.replace(
            "  - ☑️ [#subagent:grant#1] grant the repo [#q-0123456789ab]",
            "  - ~~grant the repo [#q-0123456789ab]~~\n    - **Closed by:** [#r-0123456789ad]",
        );
        let entries = document_queue::parse(&integrated).unwrap();
        let rendered =
            document_queue::render(&project_waiting_status(&entries, &ids, as_of).unwrap());
        assert!(
            rendered.contains(
                "    - **Subtree:** “grant the repo `#q-0123456789ab`” — 🗑️ deletable — every row here is struck or integrated ▫️"
            ),
            "{rendered}"
        );
    }

    /// An operator answer that happens to end in the glyph is still an answer.
    #[test]
    fn operator_answer_ending_in_the_glyph_is_still_an_answer() {
        let body = concat!(
            "- [#pick] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Pick a port. ▫️\n",
            "  - use 8080 ▫️\n",
        );
        let projected = project(body, &["pick"]);
        assert!(
            next_line(&projected, "pick").contains("pick up your reply “use 8080 ▫️”"),
            "{projected}"
        );
        assert!(projected.contains("  - use 8080 ▫️\n"), "{projected}");
    }
}
