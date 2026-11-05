//! Per-subtree triage of one waiting item (`#waitingtriage`).
//!
//! A Waiting-on-you item's packet often grows several decisions: the item's
//! own ask, options a response offered, worker attempts each option started,
//! and the operator's replies. One head-level roll-up cannot say which part of
//! that tree waits on whom, so this module derives, for any row of a packet,
//! the state of the subtree rooted there:
//!
//! - the open operator asks inside it (reserved request ids still open in
//!   Todo), each with the options offered for it, so the ask is exact;
//! - the agent-owned debt inside it: live worker attempts (reported results
//!   not yet integrated, running, lost) and operator replies no attempt or
//!   response has picked up;
//! - closure evidence (struck rows and `**Closed by:**` links), which makes a
//!   subtree with no asks and no debt deletable;
//! - the newest recorded activity (`**Timestamp:**`, `⟦addressed-at:…⟧`), so
//!   agent-owned debt idle for over a day is flagged.
//!
//! [`subtree_status`] is the one computation; [`decision_subtree_roots`] picks
//! the subtrees worth naming, and [`waiting_triage`] combines both for one
//! item. Rendering lives in [`crate::waiting_status`]; the queue tree view and
//! the answer/guidance projections read the same values.
//!
//! This module is pure: it reads parsed queue entries and never writes them.

use std::collections::{BTreeMap, HashSet};

use agent_doc_document::queue_projection::{
    QueueLifecycleMarker, is_reserved_operator_action_id, lifecycle_marker,
};

use crate::document_queue::QueueEntry;
use crate::waiting_packet::{
    self, WaitingMember, classify_member, entry_list_indent, subtree_end, waiting_owner,
};

/// Agent-owned debt older than this many minutes is flagged stale.
pub const STALE_AGENT_DEBT_MINUTES: i64 = 24 * 60;

/// The longest subtree label quoted in a rendered row.
const LABEL_CHARS: usize = 60;

/// One open operator ask inside a subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAsk {
    /// The reserved request id (`subagent-operator-request-…`).
    pub id: String,
    /// Labels of the options offered beneath that request's `**Request**` row.
    pub choices: Vec<String>,
}

/// One live worker attempt inside a subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtreeAttempt {
    pub marker: QueueLifecycleMarker,
    /// `owner#generation`, when the row carries an owner annotation.
    pub owner: Option<String>,
}

/// What a subtree needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtreeNext {
    /// The operator owes the exact ask.
    You(String),
    /// The agent owes the action.
    Agent(String),
    /// Every row is struck or integrated; nothing is owed.
    Deletable,
    /// No ask, debt, or closure is recorded.
    Undetermined,
}

/// The derived state of the subtree rooted at one packet row (or at the head).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtreeStatus {
    /// Entry index of the subtree root.
    pub root: usize,
    /// Short display label of the root row (no `[#id]` references).
    pub label: String,
    pub open_asks: Vec<OpenAsk>,
    pub attempts: Vec<SubtreeAttempt>,
    /// Operator replies nothing has picked up yet, newest last.
    pub unpicked_replies: Vec<String>,
    /// Struck rows.
    pub struck: usize,
    /// `**Closed by:**` back-references.
    pub closed_by: usize,
    /// Newest recorded activity, in minutes since the Unix epoch, and its date.
    pub latest_activity: Option<(i64, String)>,
    /// Whether agent-owned debt has sat for over a day as of the caller's time.
    pub stale: bool,
}

impl SubtreeStatus {
    /// Open asks to the operator.
    pub fn you(&self) -> usize {
        self.open_asks.len()
    }

    /// Worker results not yet integrated (any live attempt) plus replies not
    /// yet picked up.
    pub fn agent(&self) -> usize {
        self.attempts.len() + self.unpicked_replies.len()
    }

    /// Whether the subtree can be deleted: nothing is owed and every row that
    /// carried work is struck or linked to the response that closed it.
    pub fn deletable(&self) -> bool {
        self.you() == 0 && self.agent() == 0 && self.struck + self.closed_by > 0
    }

    fn count_attempts(&self, pick: impl Fn(QueueLifecycleMarker) -> bool) -> usize {
        self.attempts
            .iter()
            .filter(|attempt| pick(attempt.marker))
            .count()
    }

    /// The single owner and action this subtree needs, operator asks first.
    pub fn next(&self) -> SubtreeNext {
        if !self.open_asks.is_empty() {
            return SubtreeNext::You(ask_text(&self.open_asks));
        }
        if self.agent() > 0 {
            return SubtreeNext::Agent(self.agent_action());
        }
        if self.deletable() {
            return SubtreeNext::Deletable;
        }
        SubtreeNext::Undetermined
    }

    /// The agent-owned part, whether or not an ask comes first.
    pub fn agent_action(&self) -> String {
        let mut parts = Vec::new();
        let reported = self.count_attempts(|marker| {
            matches!(
                marker,
                QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported
            )
        });
        let running = self.count_attempts(|marker| {
            matches!(
                marker,
                QueueLifecycleMarker::Running | QueueLifecycleMarker::Claimed
            )
        });
        let reconciling = self.count_attempts(|marker| marker == QueueLifecycleMarker::Reconciling);
        let other = self.attempts.len() - reported - running - reconciling;
        if reported > 0 {
            parts.push(format!(
                "integrate {reported} reported {}{}",
                plural(reported, "result", "results"),
                owners_suffix(&self.attempts, |marker| matches!(
                    marker,
                    QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported
                ))
            ));
        }
        if running > 0 {
            parts.push(format!(
                "let {running} running {} finish",
                plural(running, "attempt", "attempts")
            ));
        }
        if reconciling > 0 {
            parts.push(format!(
                "reconcile {reconciling} recorded {}",
                plural(reconciling, "completion", "completions")
            ));
        }
        if other > 0 {
            parts.push(format!(
                "recover or retry {other} lost {}",
                plural(other, "attempt", "attempts")
            ));
        }
        if let Some(reply) = self.unpicked_replies.last() {
            let count = self.unpicked_replies.len();
            parts.push(if count == 1 {
                format!("pick up your reply “{}”", quote(reply, 80))
            } else {
                format!("pick up {count} replies (latest “{}”)", quote(reply, 80))
            });
        }
        parts.join("; ")
    }

    /// ` ⏰ stale: …` when agent-owned debt has sat for over a day.
    pub fn stale_note(&self) -> String {
        match (&self.latest_activity, self.stale) {
            (Some((_, date)), true) => {
                format!(" ⏰ stale: agent-owned, no recorded activity since {date} (over a day)")
            }
            _ => String::new(),
        }
    }

    /// One rendered line body: `“label” — Next: you — …`.
    pub fn summary(&self) -> String {
        let state = match self.next() {
            SubtreeNext::You(ask) => {
                let mut text = format!("Next: you — {ask}");
                if self.agent() > 0 {
                    text.push_str(&format!("; then agent — {}", self.agent_action()));
                }
                text
            }
            SubtreeNext::Agent(action) => format!("Next: agent — {action}"),
            SubtreeNext::Deletable => {
                "🗑️ deletable — every row here is struck or integrated".to_string()
            }
            SubtreeNext::Undetermined => {
                "Next: you — reply here with the decision or result this part asks for (no ask, attempt, or closure recorded yet)".to_string()
            }
        };
        format!("“{}” — {state}{}", self.label, self.stale_note())
    }
}

/// The triage of one waiting item: the item as a whole and its named
/// decision subtrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingTriage {
    pub item: SubtreeStatus,
    /// `(depth, status)` per decision subtree, in document order; `depth` is
    /// the number of enclosing decision subtrees.
    pub subtrees: Vec<(usize, SubtreeStatus)>,
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

fn owners_suffix(
    attempts: &[SubtreeAttempt],
    pick: impl Fn(QueueLifecycleMarker) -> bool,
) -> String {
    let owners = attempts
        .iter()
        .filter(|attempt| pick(attempt.marker))
        .filter_map(|attempt| attempt.owner.as_deref())
        .collect::<Vec<_>>();
    match owners.as_slice() {
        [] => String::new(),
        [one] => format!(" (`{one}`)"),
        [first, rest @ ..] => format!(" (`{first}` and {} more)", rest.len()),
    }
}

/// The exact operator ask for `asks`: each open request id with the options
/// offered for it.
pub fn ask_text(asks: &[OpenAsk]) -> String {
    asks.iter()
        .map(|ask| {
            let choices = ask
                .choices
                .iter()
                .map(|label| format!("**{label}**"))
                .collect::<Vec<_>>();
            match choices.as_slice() {
                [] => format!("answer the open request `#{}`", ask.id),
                [one] => format!(
                    "answer the open request `#{}`: confirm {one} or reply with another",
                    ask.id
                ),
                [rest @ .., last] => format!(
                    "answer the open request `#{}`: choose {} or {last}, or reply with another",
                    ask.id,
                    rest.join(", ")
                ),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn quote(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let mut cut = flat.chars().take(limit).collect::<String>();
    if let Some(space) = cut.rfind(' ') {
        cut.truncate(space);
    }
    format!("{}…", cut.trim_end())
}

/// The first line of an entry, without its bullet.
fn entry_body(entry: &QueueEntry) -> Option<&str> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
            Some(prompt.text.lines().next().unwrap_or_default())
        }
        QueueEntry::Freeform(line) => {
            let first = line.lines().next().unwrap_or_default().trim_start();
            first
                .strip_prefix("- ")
                .or_else(|| first.strip_prefix("* "))
                .or_else(|| first.strip_prefix("+ "))
        }
        _ => None,
    }
}

fn is_struck_body(body: &str) -> bool {
    body.trim_start().starts_with("~~")
}

/// The label of an offered choice row (`**label**: effect`).
fn choice_label(body: &str) -> Option<String> {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(body).trim();
    let work = work
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .unwrap_or(work);
    if !waiting_packet::is_offered_choice(work) {
        return None;
    }
    let rest = work.strip_prefix("**")?;
    let (label, _) = rest.split_once("**: ")?;
    Some(label.trim().to_string())
}

fn request_row_id(body: &str) -> Option<String> {
    let rest = body.trim().strip_prefix("**Request**: [#")?;
    rest.split_once(']').map(|(id, _)| id.to_ascii_lowercase())
}

fn is_closed_by(body: &str) -> bool {
    body.trim().starts_with("**Closed by:** [#r-")
}

/// Generated rows the triage itself renders; never read back as input.
fn is_triage_output(entries: &[QueueEntry], index: usize) -> bool {
    crate::waiting_status::is_status_row(entries, index)
}

/// Rows of the subtree rooted at `root` that belong to the same waiting item:
/// the root itself (unless it is the head), then every descendant the item
/// owns, minus the triage's own output rows and everything beneath them.
fn owned_rows(entries: &[QueueEntry], head: usize, root: usize) -> Vec<usize> {
    let end = subtree_end(entries, root);
    let mut rows = Vec::new();
    if root != head {
        rows.push(root);
    }
    let mut index = root + 1;
    while index < end {
        if is_triage_output(entries, index) {
            index = subtree_end(entries, index);
            continue;
        }
        if waiting_owner(entries, index) == Some(head) {
            rows.push(index);
        }
        index += 1;
    }
    rows
}

/// Parse `YYYY-MM-DDTHH:MM[:SS[.fff]][Z|±HH:MM]` into minutes since the Unix
/// epoch (UTC) and the local calendar date as written.
pub fn parse_timestamp_minutes(text: &str) -> Option<(i64, String)> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 16 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute) = (number(11..13)?, number(14..16)?);
    if bytes[13] != b':' || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut rest = &text[16..];
    if let Some(seconds) = rest.strip_prefix(':') {
        let digits = seconds.bytes().take_while(u8::is_ascii_digit).count();
        rest = &seconds[digits..];
        if let Some(fraction) = rest.strip_prefix('.') {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            rest = &fraction[digits..];
        }
    }
    let offset = match rest.trim_end_matches(|ch: char| ch == '⟧' || ch.is_whitespace()) {
        "" | "Z" => 0,
        zone => {
            let sign = match zone.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits = zone[1..].replace(':', "");
            if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            sign * (digits[..2].parse::<i64>().ok()? * 60 + digits[2..].parse::<i64>().ok()?)
        }
    };
    // Days from the civil date (proleptic Gregorian), Howard Hinnant's method.
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = shifted.div_euclid(400);
    let year_of_era = shifted - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some((
        days * 1440 + hour * 60 + minute - offset,
        text[..10].to_string(),
    ))
}

/// Recorded activity times on one row: a `**Timestamp:**` value or a
/// `⟦addressed-at:…⟧` annotation.
fn row_activity(body: &str) -> Option<(i64, String)> {
    let text = agent_doc_markdown_ast::reply_row::reply_row_text(body);
    if let Some(value) = text.strip_prefix("**Timestamp:**") {
        return parse_timestamp_minutes(value.trim());
    }
    let at = body.find("⟦addressed-at:")?;
    let value = &body[at + "⟦addressed-at:".len()..];
    let end = value.find('⟧')?;
    parse_timestamp_minutes(&value[..end])
}

/// Labels of the offered options grouped under each `**Request**: [#id]` row
/// of the item: the options that follow it among its siblings, up to the next
/// `**Request**` sibling.
fn request_choices(entries: &[QueueEntry], head: usize) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, Vec<String>>::new();
    for index in owned_rows(entries, head, head) {
        let Some(id) = entry_body(&entries[index]).and_then(request_row_id) else {
            continue;
        };
        let Some(indent) = entry_list_indent(&entries[index]) else {
            continue;
        };
        let parent_end = (0..index)
            .rev()
            .find(|candidate| {
                entry_list_indent(&entries[*candidate]).is_some_and(|other| other < indent)
            })
            .map_or(entries.len(), |parent| subtree_end(entries, parent));
        let labels = out.entry(id).or_default();
        let mut sibling = subtree_end(entries, index);
        while sibling < parent_end {
            if entry_list_indent(&entries[sibling]) != Some(indent) {
                sibling += 1;
                continue;
            }
            let body = entry_body(&entries[sibling]).unwrap_or_default();
            if request_row_id(body).is_some() {
                break;
            }
            if let Some(label) = choice_label(body)
                && !labels.contains(&label)
            {
                labels.push(label);
            }
            sibling = subtree_end(entries, sibling);
        }
    }
    out
}

/// Short display label of a root row: an offered choice's label, else its
/// work text without projection, comments, or `%%` fences, with each `[#id]`
/// reference written as a `` `#id` `` code span.
fn root_label(entries: &[QueueEntry], root: usize) -> String {
    let body = entry_body(&entries[root]).unwrap_or_default();
    if let Some(label) = choice_label(body) {
        return quote(&label, LABEL_CHARS);
    }
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(body).trim();
    let work = crate::waiting_status::without_status_token(work);
    let mut text = String::with_capacity(work.len());
    let mut rest = work.as_str();
    loop {
        let next = [rest.find("[#"), rest.find('⟦'), rest.find("<!--")]
            .into_iter()
            .flatten()
            .min();
        let Some(at) = next else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..at]);
        let reference = rest[at..].starts_with("[#");
        let close = if reference {
            "]"
        } else if rest[at..].starts_with('⟦') {
            "⟧"
        } else {
            "-->"
        };
        match rest[at..].find(close) {
            Some(end) => {
                // A reference stays readable as a code span, never `[#id]`.
                if reference {
                    text.push_str(&format!("`{}`", &rest[at + 1..at + end]));
                }
                rest = &rest[at + end + close.len()..];
            }
            None => break,
        }
    }
    let text = text.replace("%%", "").replace("~~", "");
    let text = text.trim().trim_end_matches(':').trim();
    if text.is_empty() {
        "this part".to_string()
    } else {
        quote(text, LABEL_CHARS)
    }
}

/// Whether an operator reply at `index` was picked up: something generated
/// (a `**label**` row), attempted, or struck sits beneath it.
fn reply_picked_up(entries: &[QueueEntry], index: usize) -> bool {
    (index + 1..subtree_end(entries, index)).any(|child| match &entries[child] {
        QueueEntry::Completed(_) => true,
        QueueEntry::Prompt(prompt) => {
            matches!(classify_member(prompt), WaitingMember::Attempt(_))
                || agent_doc_element_queue::struck_work_text(&prompt.text).starts_with("**")
        }
        QueueEntry::Freeform(_) => waiting_packet::operator_freeform_text(entries, child).is_none(),
        _ => false,
    })
}

fn owner_label(text: &str) -> Option<String> {
    agent_doc_document::queue_projection::owner_projection(text)
        .map(|owner| format!("{}#{}", owner.owner, owner.generation))
}

fn attempt_on(text: &str) -> Option<SubtreeAttempt> {
    let marker = lifecycle_marker(text)?;
    matches!(
        marker,
        QueueLifecycleMarker::Claimed
            | QueueLifecycleMarker::Running
            | QueueLifecycleMarker::Reported
            | QueueLifecycleMarker::Lost
            | QueueLifecycleMarker::LostClaimed
            | QueueLifecycleMarker::LostRunning
            | QueueLifecycleMarker::LostReported
            | QueueLifecycleMarker::Reconciling
            | QueueLifecycleMarker::Unknown
    )
    .then(|| SubtreeAttempt {
        marker,
        owner: owner_label(text),
    })
}

fn collect_asks(
    text: &str,
    own_ids: &HashSet<String>,
    open_todo_ids: &HashSet<String>,
    choices: &BTreeMap<String, Vec<String>>,
    out: &mut Vec<OpenAsk>,
) {
    for id in crate::document_queue::queue_prompt_reference_ids(text) {
        let id = id.to_ascii_lowercase();
        if is_reserved_operator_action_id(&id)
            && open_todo_ids.contains(&id)
            && !own_ids.contains(&id)
            && !out.iter().any(|ask| ask.id == id)
        {
            out.push(OpenAsk {
                choices: choices.get(&id).cloned().unwrap_or_default(),
                id,
            });
        }
    }
}

/// The state of the subtree rooted at `root` inside the waiting item at
/// `head` (`root == head` for the whole item). `as_of` is the caller's notion
/// of now in minutes since the Unix epoch (the document's newest response
/// time keeps the projection deterministic); without it nothing is stale.
pub fn subtree_status(
    entries: &[QueueEntry],
    head: usize,
    root: usize,
    open_todo_ids: &HashSet<String>,
    as_of: Option<i64>,
) -> SubtreeStatus {
    let head_text = match entries.get(head) {
        Some(QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt)) => prompt.text.as_str(),
        _ => "",
    };
    let own_ids = crate::document_queue::queue_prompt_reference_ids(
        agent_doc_element_queue::strip_attempt_projection_prefix(head_text),
    )
    .into_iter()
    .collect::<HashSet<_>>();
    let choices = request_choices(entries, head);
    let mut status = SubtreeStatus {
        root,
        label: root_label(entries, root),
        open_asks: Vec::new(),
        attempts: Vec::new(),
        unpicked_replies: Vec::new(),
        struck: 0,
        closed_by: 0,
        latest_activity: None,
        stale: false,
    };
    if root == head
        && let Some(attempt) = attempt_on(head_text)
    {
        status.attempts.push(attempt);
    }
    let note_activity = |activity: Option<(i64, String)>, status: &mut SubtreeStatus| {
        if let Some(activity) = activity
            && status
                .latest_activity
                .as_ref()
                .is_none_or(|(latest, _)| activity.0 > *latest)
        {
            status.latest_activity = Some(activity);
        }
    };
    for index in owned_rows(entries, head, root) {
        let entry = &entries[index];
        let body = entry_body(entry).unwrap_or_default();
        note_activity(row_activity(body), &mut status);
        match entry {
            QueueEntry::Completed(_) => status.struck += 1,
            QueueEntry::Prompt(prompt) => {
                match classify_member(prompt) {
                    WaitingMember::Attempt(fallback) => {
                        status
                            .attempts
                            .push(attempt_on(&prompt.text).unwrap_or(SubtreeAttempt {
                                marker: fallback,
                                owner: owner_label(&prompt.text),
                            }));
                    }
                    WaitingMember::Operator => {
                        let reply = agent_doc_element_queue::struck_work_text(&prompt.text).trim();
                        if !waiting_packet::is_agent_doc_reply_entry(entries, index)
                            && !reply.is_empty()
                            && !reply.starts_with("**")
                            && !reply_picked_up(entries, index)
                        {
                            status.unpicked_replies.push(reply.to_string());
                        }
                    }
                    WaitingMember::BinaryFollowUp | WaitingMember::OfferedChoice => {}
                }
                collect_asks(
                    &prompt.text,
                    &own_ids,
                    open_todo_ids,
                    &choices,
                    &mut status.open_asks,
                );
            }
            QueueEntry::Freeform(_) => {
                if is_struck_body(body) {
                    status.struck += 1;
                    continue;
                }
                if is_closed_by(body) {
                    status.closed_by += 1;
                }
                if let Some(reply) = waiting_packet::operator_freeform_text(entries, index)
                    && !reply_picked_up(entries, index)
                {
                    status.unpicked_replies.push(reply.to_string());
                }
                collect_asks(
                    body,
                    &own_ids,
                    open_todo_ids,
                    &choices,
                    &mut status.open_asks,
                );
            }
            _ => {}
        }
    }
    if status.latest_activity.is_none() && root != head {
        // A subtree without its own timestamp inherits the item's newest one.
        for index in owned_rows(entries, head, head) {
            let activity = entry_body(&entries[index]).and_then(row_activity);
            note_activity(activity, &mut status);
        }
    }
    status.stale = status.agent() > 0
        && matches!((as_of, &status.latest_activity), (Some(now), Some((at, _)))
            if now - at > STALE_AGENT_DEBT_MINUTES);
    status
}

/// The packet rows that root a decision of their own, in document order:
///
/// - each direct child of the head that has rows beneath it, or carries a
///   worker attempt (each is one unit of agent-owned debt);
/// - each offered option (`**label**: effect`) that something acted on: a
///   worker attempt or a `**Closed by:**` link sits beneath it. Options
///   copied inside a `**Closed by:**` back-reference are references, never
///   decisions of their own.
///
/// Generated rows (the head's Response metadata, a `**Response [#r-…]**`
/// link, the triage's own rows) are never roots: the head's Next row already
/// speaks for them.
pub fn decision_subtree_roots(entries: &[QueueEntry], head: usize) -> Vec<usize> {
    let Some(head_indent) = entries.get(head).and_then(entry_list_indent) else {
        return Vec::new();
    };
    let rows = owned_rows(entries, head, head);
    let child_indent = rows
        .iter()
        .filter_map(|index| entry_list_indent(&entries[*index]))
        .filter(|indent| *indent > head_indent)
        .min();
    let mut closed_by_ranges = Vec::new();
    let mut roots = Vec::new();
    for index in rows {
        let entry = &entries[index];
        let body = entry_body(entry).unwrap_or_default();
        if is_closed_by(body) {
            closed_by_ranges.push(index..subtree_end(entries, index));
            continue;
        }
        // Generated metadata (reply rows, `**Response [#r-…]**`, `**Choices:**`)
        // is never a decision of its own; an offered option is.
        let generated = waiting_packet::is_agent_doc_reply_entry(entries, index)
            || agent_doc_element_queue::strip_attempt_projection_prefix(body)
                .trim_start()
                .starts_with("**");
        if generated && choice_label(body).is_none() {
            continue;
        }
        let has_children = subtree_end(entries, index) > index + 1;
        let direct = entry_list_indent(entry) == child_indent;
        let is_attempt = matches!(entry, QueueEntry::Prompt(prompt)
            if matches!(classify_member(prompt), WaitingMember::Attempt(_)));
        if direct && (has_children || is_attempt) {
            roots.push(index);
            continue;
        }
        let inside_reference = closed_by_ranges.iter().any(|range| range.contains(&index));
        if inside_reference || choice_label(body).is_none() {
            continue;
        }
        let acted_on = (index + 1..subtree_end(entries, index)).any(|child| {
            let child_body = entry_body(&entries[child]).unwrap_or_default();
            is_closed_by(child_body)
                || matches!(&entries[child], QueueEntry::Prompt(prompt)
                    if matches!(classify_member(prompt), WaitingMember::Attempt(_)))
        });
        if acted_on {
            roots.push(index);
        }
    }
    roots
}

/// The item's own status and the status of each decision subtree.
pub fn waiting_triage(
    entries: &[QueueEntry],
    head: usize,
    open_todo_ids: &HashSet<String>,
    as_of: Option<i64>,
) -> WaitingTriage {
    let roots = decision_subtree_roots(entries, head);
    let subtrees = roots
        .iter()
        .map(|root| {
            let end = subtree_end(entries, *root);
            let depth = roots
                .iter()
                .filter(|other| *other < root && subtree_end(entries, **other) >= end)
                .count();
            (
                depth,
                subtree_status(entries, head, *root, open_todo_ids, as_of),
            )
        })
        .collect();
    WaitingTriage {
        item: subtree_status(entries, head, head, open_todo_ids, as_of),
        subtrees,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;

    fn open(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn timestamps_parse_with_offsets_and_fractions() {
        let utc = parse_timestamp_minutes("2026-11-02T00:20:07.000Z").unwrap();
        let central = parse_timestamp_minutes("2026-11-01T19:20-05:00").unwrap();
        assert_eq!(utc.0, central.0);
        assert_eq!(utc.1, "2026-11-02");
        assert_eq!(central.1, "2026-11-01");
        assert_eq!(parse_timestamp_minutes("1970-01-01T00:00Z").unwrap().0, 0);
        assert_eq!(parse_timestamp_minutes("1970-01-02T00:00").unwrap().0, 1440);
        assert!(parse_timestamp_minutes("yesterday").is_none());
        assert!(parse_timestamp_minutes("2026-13-01T00:00Z").is_none());
    }

    const ITEM: &str = concat!(
        "- [#rel] Release App access <!-- agent-doc:waiting-item -->\n",
        "  - %% steps: add the repo, then re-run. %%\n",
        "    - tmstorm [#q-569d77146187]\n",
        "      - **Response [#r-30853087f858]**\n",
        "        - **Timestamp:** 2026-11-02T00:20:07.000Z\n",
        "        - **Choices:**\n",
        "          - **Request**: [#subagent-operator-request-pick]\n",
        "          - **Done as-is**: leave it. [#q-865c445b9b8a]\n",
        "            - **Closed by:** [#r-3b817c02c2e8]\n",
        "            - ☑️ [#subagent:865c445b#1] Resolve the open operator action to complete the parent item. ⟦effect:4⟧\n",
        "          - **Add by PR**: open a PR. [#q-229564d17c90]\n",
        "          - **Old option**: struck one. [#q-0f24a54c9ec9]\n",
        "            - **Closed by:** [#r-3b817c02c2e8]\n",
        "            - ~~pick~~\n",
        "  - ☑️ [#subagent:c252releaseapp#1] do [#q-c4dbe51b8e36]\n",
    );

    fn triage(body: &str, ids: &[&str], as_of: Option<&str>) -> WaitingTriage {
        let entries = document_queue::parse(body).unwrap();
        let as_of = as_of.map(|text| parse_timestamp_minutes(text).unwrap().0);
        waiting_triage(&entries, 0, &open(ids), as_of)
    }

    #[test]
    fn each_decision_subtree_names_its_owner_ask_and_debt() {
        let triage = triage(
            ITEM,
            &["rel", "subagent-operator-request-pick"],
            Some("2026-11-04T19:08-06:00"),
        );
        let labels = triage
            .subtrees
            .iter()
            .map(|(depth, status)| (*depth, status.label.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                (0, "steps: add the repo, then re-run."),
                (1, "Done as-is"),
                (1, "Old option"),
                (0, "do `#q-c4dbe51b8e36`"),
            ]
        );
        let steps = &triage.subtrees[0].1;
        assert_eq!(steps.you(), 1);
        assert_eq!(steps.agent(), 1);
        assert_eq!(
            steps.next(),
            SubtreeNext::You(
                "answer the open request `#subagent-operator-request-pick`: choose **Done as-is**, **Add by PR** or **Old option**, or reply with another"
                    .to_string()
            )
        );
        let done_as_is = &triage.subtrees[1].1;
        assert_eq!(
            done_as_is.next(),
            SubtreeNext::Agent("integrate 1 reported result (`subagent:865c445b#1`)".to_string())
        );
        assert!(done_as_is.stale, "{done_as_is:?}");
        assert!(
            done_as_is.summary().ends_with(
                "⏰ stale: agent-owned, no recorded activity since 2026-11-02 (over a day)"
            ),
            "{}",
            done_as_is.summary()
        );
        let old = &triage.subtrees[2].1;
        assert!(old.deletable(), "{old:?}");
        assert_eq!(old.next(), SubtreeNext::Deletable);
        assert!(old.summary().contains("🗑️ deletable"), "{}", old.summary());
        let attempt = &triage.subtrees[3].1;
        assert_eq!(attempt.agent(), 1);
        assert!(attempt.stale, "inherits the item's newest activity");
        assert_eq!(triage.item.you(), 1);
        assert_eq!(triage.item.agent(), 2);
    }

    #[test]
    fn fresh_debt_is_not_stale_and_no_clock_means_never_stale() {
        let fresh = triage(
            ITEM,
            &["rel", "subagent-operator-request-pick"],
            Some("2026-11-02T12:00Z"),
        );
        assert!(fresh.subtrees.iter().all(|(_, status)| !status.stale));
        let unclocked = triage(ITEM, &["rel", "subagent-operator-request-pick"], None);
        assert!(unclocked.subtrees.iter().all(|(_, status)| !status.stale));
    }

    #[test]
    fn a_closed_request_leaves_only_agent_debt_and_then_nothing() {
        let triage_closed = triage(ITEM, &["rel"], None);
        let steps = &triage_closed.subtrees[0].1;
        assert_eq!(steps.you(), 0);
        assert!(matches!(steps.next(), SubtreeNext::Agent(_)));
        let integrated = ITEM
            .replace(
                "            - ☑️ [#subagent:865c445b#1] Resolve the open operator action to complete the parent item. ⟦effect:4⟧\n",
                "",
            )
            .replace(
                "  - ☑️ [#subagent:c252releaseapp#1] do [#q-c4dbe51b8e36]\n",
                "  - ~~do [#q-c4dbe51b8e36]~~\n",
            );
        let done = triage(&integrated, &["rel"], None);
        let steps = &done.subtrees[0].1;
        assert!(steps.deletable(), "{steps:?}");
        assert_eq!(done.item.agent(), 0);
    }

    #[test]
    fn an_unpicked_reply_is_agent_debt_and_a_picked_up_one_is_not() {
        let body = concat!(
            "- [#port] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "  - use 8080\n",
            "  - use 9090\n",
            "    - **Response [#r-0123456789ab]**\n",
        );
        let entries = document_queue::parse(body).unwrap();
        let item = subtree_status(&entries, 0, 0, &open(&["port"]), None);
        assert_eq!(item.unpicked_replies, vec!["use 8080".to_string()]);
        assert_eq!(
            item.next(),
            SubtreeNext::Agent("pick up your reply “use 8080”".to_string())
        );
    }
}
