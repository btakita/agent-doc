//! Capture questions, operator answers and the id links between them from a
//! session document (and its done archives).
//!
//! The capture is purely structural. No agent action is involved: every
//! operator row the document holds is an observation the next build sees.
//!
//! Questions come from `agent:todo` rows, Waiting-on-you item heads (live,
//! struck, or quoted in a done archive) and blocking operator-request packets
//! in `agent:exchange`. Answers come from:
//!
//! - operator rows inside a waiting item's packet (anything the operator typed
//!   below the head that is not a generated `▫️` / metadata / bold-label /
//!   attempt / binary follow-up row; a struck row is a consumed answer and
//!   still counts, a binary `auto-struck` row does not);
//! - queue rows that address an id: `[#id]: answer`, `re [#id]: answer`, or
//!   operator text ending in `[#subagent-operator-request-…]`;
//! - operator rows in a waiting packet that sits in a queue outside any
//!   waiting item (its `**Response:**` row survived, its waiting-item marker
//!   did not). The packet belongs to the id its parent queue row addresses
//!   (`do [#id]`, `[#id]: …`); follow-up rows directly under a queue row are
//!   the operator talking, not answering, and are never captured
//!   (`#answeroutsidewaiting`).
//!
//! Links record why two ids may be the same question: a `**Request**: [#req]`
//! graft or trailing request id on an item head, a request id that embeds an
//! item id (`subagent-operator-request-<item>-<suffix>`), a packet's
//! `queue_item`, a `(source: … [#id])` citation, or an operator `same as
//! [#id]` reply. The ledger decides which links are identity.
//!
//! A todo or waiting copy may also carry the decision id the binary stamped
//! on it (`⟦decision:<id>⟧`, `#decisionidstamp`); copies that carry the same
//! stamp are one decision.

use crate::text::strip_decorations;
use agent_doc_element::decision_stamp::{decision_stamp, strip_decision_stamps};

pub const WAITING_ITEM_MARKER: &str = "<!-- agent-doc:waiting-item -->";
const ARCHIVE_MARKER: &str = "<!-- agent-doc:waiting-archive -->";
pub const REQUEST_ID_PREFIX: &str = "subagent-operator-request-";

/// Where a question row lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionSource {
    Todo,
    Waiting,
    Request,
}

/// One row that asks the operator something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionObs {
    pub id: Option<String>,
    pub text: String,
    pub source: QuestionSource,
    /// Still asking: an open todo, a live (unstruck, unarchived) waiting head,
    /// or a request packet.
    pub open: bool,
    /// 1-based line in its source text.
    pub line: usize,
    /// Ids cited by the row that are not its identity (`(source: … [#x])`,
    /// `queue item ‘do [#x]’`, `#x`).
    pub references: Vec<String>,
    /// Decision id the binary stamped on this copy (`⟦decision:<id>⟧`,
    /// `#decisionidstamp`).
    pub stamp: Option<String>,
}

/// One operator answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerObs {
    /// Ids the answer addresses (its waiting head's ids, or the id a queue row
    /// names), in priority order.
    pub targets: Vec<String>,
    /// Question text the answer sits under when the head has no id.
    pub head_text: Option<String>,
    /// Verbatim operator text (decorations removed).
    pub text: String,
    /// Human-readable location (`under [#y2fn]`, `queue row`, `done archive`).
    pub location: String,
    pub line: usize,
    /// Consumed (struck) or archived; still an answer.
    pub consumed: bool,
    /// The answer lives in its own target's waiting packet (the dispatch path).
    pub in_own_packet: bool,
    /// The operator's explicit `change:` / `amend:` lead-in
    /// (`#decisiongraph`); stripped from `text`.
    pub explicit: Option<crate::RevisionRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// `**Request**: [#req]` row or trailing request id on an item head.
    RequestGraft,
    /// `subagent-operator-request-<item>-<suffix>` names `<item>`.
    RequestInfix,
    /// The request packet's `queue_item` names the item.
    QueueItem,
    /// The operator said `same as [#id]`.
    OperatorSameAs,
    /// The operator said `depends on [#id]` / `assumes [#id]` (`#decisiongraph`).
    OperatorDependsOn,
    /// The operator said `conflicts with [#id]` / `contradicts [#id]`.
    OperatorConflictsWith,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkObs {
    pub from: String,
    pub to: String,
    pub kind: LinkKind,
}

/// The tracked-work component an item lives in (`#decisionstaleaudit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackedComponent {
    Todo,
    Backlog,
    Review,
    Icebox,
}

impl TrackedComponent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Backlog => "backlog",
            Self::Review => "review",
            Self::Icebox => "icebox",
        }
    }
}

/// One open or closed tracked item: an `agent:todo` row or a
/// backlog/review/icebox item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedItemObs {
    pub component: TrackedComponent,
    /// Own id, lowercase. Items without an id are not audited.
    pub id: String,
    /// Item text without checkbox and own id.
    pub text: String,
    pub open: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observations {
    pub questions: Vec<QuestionObs>,
    pub answers: Vec<AnswerObs>,
    pub links: Vec<LinkObs>,
    /// Ids a live (unstruck) queue row outside any waiting tree still names:
    /// that row is pending work that carries its answer. Closing the question
    /// would race it.
    pub live_queue_ids: std::collections::BTreeSet<String>,
    /// `#decisionstaleaudit`: every todo, backlog, review and icebox item.
    pub tracked: Vec<TrackedItemObs>,
    /// Own ids of live waiting heads whose counter still shows work for the
    /// operator or the agent (`⟦… you: N · agent: M …⟧` with N or M above
    /// zero): that tree is still being worked, so its todo never auto-closes.
    pub busy_waiting_ids: std::collections::BTreeSet<String>,
}

/// Whether operator text asks something back instead of answering
/// (`Is this done?`, `Could you point it out to me?`). A question is never
/// recorded as an answer.
pub fn is_question_text(text: &str) -> bool {
    text.trim_end().ends_with('?')
}

/// The open-work counter of a waiting head (`⟦☑️ you: 1 · agent: 0 · done: 2⟧`):
/// `true` when the operator or the agent still has rows to work.
fn waiting_head_busy(body: &str) -> bool {
    let mut rest = body;
    while let Some(start) = rest.find('⟦') {
        let after = &rest[start + '⟦'.len_utf8()..];
        let Some(end) = after.find('⟧') else { break };
        let token = &after[..end];
        if token.contains("you:") {
            let count = |label: &str| -> u64 {
                token
                    .split(label)
                    .nth(1)
                    .and_then(|tail| {
                        tail.trim_start()
                            .split(|c: char| !c.is_ascii_digit())
                            .next()
                    })
                    .and_then(|digits| digits.parse().ok())
                    .unwrap_or(0)
            };
            return count("you:") > 0 || count("agent:") > 0;
        }
        rest = &after[end..];
    }
    false
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn list_body(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .map(str::trim_end)
}

/// Every `[#id]` token in `text` (ids with `:` are attempt handles, skipped).
pub fn bracket_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else { break };
        let id = &after[..end];
        if !id.is_empty()
            && !id.contains(':')
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            ids.push(id.to_ascii_lowercase());
        }
        rest = &after[end..];
    }
    ids
}

/// The decision id stamped on a row (`#decisionidstamp`), lowercase.
fn stamp_of(text: &str) -> Option<String> {
    decision_stamp(text).map(str::to_ascii_lowercase)
}

fn is_request_id(id: &str) -> bool {
    id.starts_with(REQUEST_ID_PREFIX)
}

fn is_handle(id: &str) -> bool {
    id.starts_with("q-") || id.starts_with("r-")
}

/// Strip leading glyphs, heading hashes, attempt handles and binary tokens off
/// a queue row's body, reporting whether the row is struck.
fn work_text(body: &str) -> (String, bool) {
    let mut text = body.trim();
    loop {
        let before = text;
        text = text.trim_start_matches('#').trim_start();
        text = text.trim_start_matches(|c: char| c != '⟦' && (!c.is_ascii() || c.is_whitespace()));
        if let Some(after) = text.strip_prefix("[#")
            && let Some(end) = after.find(']')
            && after[..end].contains(':')
        {
            text = after[end + 1..].trim_start();
        }
        if let Some(after) = text.strip_prefix('⟦')
            && let Some(end) = after.find('⟧')
        {
            text = after[end + '⟧'.len_utf8()..].trim_start();
        }
        if before == text {
            break;
        }
    }
    let struck = text.starts_with("~~");
    let inner = text.trim_start_matches("~~");
    let inner = inner.strip_suffix("~~").unwrap_or(inner);
    (inner.trim().to_string(), struck)
}

/// Trim trailing binary tokens (`[#q-…]` handles, `⟦…⟧`) off answer text.
fn answer_display(text: &str) -> String {
    let mut out = text.trim().trim_end_matches("~~").trim().to_string();
    loop {
        let before = out.clone();
        if out.ends_with('⟧')
            && let Some(start) = out.rfind('⟦')
        {
            out.truncate(start);
        }
        if out.ends_with(']')
            && let Some(start) = out.rfind("[#")
            && is_handle(&out[start + 2..out.len() - 1])
        {
            out.truncate(start);
        }
        out = out.trim().to_string();
        if out == before {
            break;
        }
    }
    out
}

/// An operator row in a waiting packet, or `None` for a generated row.
fn operator_row(body: &str) -> Option<(String, bool)> {
    if body.ends_with('▫') || body.ends_with("▫️") || body.contains("<!-- agent-doc:") {
        return None;
    }
    let lead = body.trim_start_matches(|c: char| !c.is_ascii() || c.is_whitespace());
    if lead.starts_with("[#")
        && lead[2..]
            .split(']')
            .next()
            .is_some_and(|id| id.contains(':'))
    {
        return None;
    }
    let (text, struck) = work_text(body);
    if text.is_empty()
        || text.starts_with("**")
        || text.starts_with("%%")
        || text.starts_with('<')
        || text.starts_with('>')
        || text.starts_with("&gt;")
        || text.contains("auto-struck")
        || agent_doc_queue::waiting_packet::is_binary_follow_up(&text)
    {
        return None;
    }
    let display = answer_display(&text);
    let lower = text.to_ascii_lowercase();
    let bare_directive = ["do ", "close ", "re "].iter().any(|verb| {
        lower.strip_prefix(verb).is_some_and(|rest| {
            let rest = rest.trim();
            rest.starts_with("[#") && rest.ends_with(']') && !rest[2..].contains(' ')
        })
    });
    // `#decisionstaleaudit`: an operator question back (`Is this done?`) is
    // not an answer.
    if display.is_empty() || bare_directive || is_question_text(&display) {
        return None;
    }
    Some((display, struck))
}

/// `same as [#id]` / `duplicate of [#id]`: an operator merge.
fn same_as_target(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("same as ")
        .or_else(|| lower.strip_prefix("duplicate of "))
        .or_else(|| lower.strip_prefix("same question as "))?;
    bracket_ids(rest).into_iter().next()
}

/// Lead-ins that classify a revision explicitly (`#decisiongraph`).
const CHANGE_LEADS: [&str; 6] = [
    "change:",
    "changed:",
    "changes:",
    "reverse:",
    "reversal:",
    "supersede:",
];
const AMEND_LEADS: [&str; 3] = ["amend:", "amended:", "amendment:"];

/// Split an explicit `change:` / `amend:` lead-in off an operator answer.
/// The lead-in decides the revision kind; the rest is the answer (for an
/// amend, the clause it changes or adds).
pub fn explicit_relation(text: &str) -> (Option<crate::RevisionRelation>, String) {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    for (leads, relation) in [
        (&CHANGE_LEADS[..], crate::RevisionRelation::Supersedes),
        (&AMEND_LEADS[..], crate::RevisionRelation::Amends),
    ] {
        for lead in leads {
            if lower.starts_with(lead) {
                let rest = trimmed[lead.len()..].trim();
                if !rest.is_empty() {
                    return (Some(relation), rest.to_string());
                }
            }
        }
    }
    (None, trimmed.to_string())
}

const DEPENDS_PHRASES: [&str; 5] = [
    "depends on ",
    "assumes ",
    "builds on ",
    "relies on ",
    "requires decision ",
];
const CONFLICTS_PHRASES: [&str; 3] = ["conflicts with ", "contradicts ", "incompatible with "];

/// Explicit `depends on [#id]` / `conflicts with [#id]` phrases in operator
/// text, and whether the text is nothing but such phrases (a link-only row,
/// not an answer).
pub fn relation_links(text: &str) -> (Vec<(LinkKind, String)>, bool) {
    let lower = text.to_ascii_lowercase();
    let mut links = Vec::new();
    let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
    for (phrases, kind) in [
        (&DEPENDS_PHRASES[..], LinkKind::OperatorDependsOn),
        (&CONFLICTS_PHRASES[..], LinkKind::OperatorConflictsWith),
    ] {
        for phrase in phrases {
            let mut from = 0;
            while let Some(found) = lower[from..].find(phrase) {
                let start = from + found;
                let after = start + phrase.len();
                from = after;
                let boundary = !lower[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric());
                if !boundary || !lower[after..].starts_with("[#") {
                    continue;
                }
                let Some(end) = lower[after..].find(']') else {
                    continue;
                };
                let end = after + end + 1;
                if let Some(id) = bracket_ids(&lower[after..end]).into_iter().next() {
                    spans.push(start..end);
                    links.push((kind, id));
                }
            }
        }
    }
    if links.is_empty() {
        return (links, false);
    }
    // Link-only when nothing but filler words remain once the phrases are
    // removed ("This depends on [#a] and conflicts with [#b].").
    let mut residue = String::with_capacity(lower.len());
    for (index, c) in lower.char_indices() {
        if !spans.iter().any(|span| span.contains(&index)) {
            residue.push(c);
        }
    }
    let link_only = residue
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .all(|word| {
            matches!(
                word,
                "and" | "also" | "this" | "it" | "that" | "decision" | "but"
            )
        });
    (links, link_only)
}

/// `[#id]: answer` / `re [#id]: answer` / `answer [#request-id]`.
fn addressed_answer(text: &str) -> Option<(String, String)> {
    let lower = text.to_ascii_lowercase();
    let start = if lower.starts_with("[#") {
        0
    } else if lower.starts_with("re [#") {
        3
    } else {
        // Operator text ending in a request id addresses that request.
        let display = answer_display(text);
        let trimmed = display.trim_end();
        if trimmed.ends_with(']')
            && let Some(open) = trimmed.rfind("[#")
        {
            let id = trimmed[open + 2..trimmed.len() - 1].to_ascii_lowercase();
            let before = trimmed[..open].trim();
            let first_word = before
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if is_request_id(&id)
                && !before.is_empty()
                && !before.starts_with("**")
                && !is_question_text(before)
                && !matches!(first_word.as_str(), "do" | "close" | "re")
            {
                return Some((id, before.to_string()));
            }
        }
        return None;
    };
    let after = &text[start + 2..];
    let end = after.find(']')?;
    let id = after[..end].to_ascii_lowercase();
    if id.contains(':') || id.contains(' ') {
        return None;
    }
    let rest = after[end + 1..].trim_start();
    let answer = rest.strip_prefix(':')?.trim();
    let answer = answer_display(answer);
    (!answer.is_empty() && !is_question_text(&answer)).then_some((id, answer))
}

/// The ids on a waiting head: own id first (leading, else the first non-request
/// id after the marker), then grafted request ids.
fn head_ids(body: &str) -> (Option<String>, Vec<String>) {
    let (work, _) = work_text(body);
    let mut own = None;
    if let Some(after) = work.strip_prefix("[#")
        && let Some(end) = after.find(']')
    {
        let id = after[..end].to_ascii_lowercase();
        if !id.contains(':') {
            own = Some(id);
        }
    }
    let mut grafts = Vec::new();
    let tail = body.split(WAITING_ITEM_MARKER).nth(1).unwrap_or_default();
    for id in bracket_ids(tail) {
        if own.is_none() && !is_request_id(&id) && !is_handle(&id) {
            own = Some(id);
        } else if is_request_id(&id) && own.as_deref() != Some(id.as_str()) {
            grafts.push(id);
        }
    }
    if own.is_none()
        && let Some(first) = grafts.first().cloned()
    {
        own = Some(first);
        grafts.remove(0);
    }
    (own, grafts)
}

fn references(text: &str, own: Option<&str>) -> Vec<String> {
    let mut refs: Vec<String> = bracket_ids(text)
        .into_iter()
        .filter(|id| Some(id.as_str()) != own && !is_handle(id))
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

/// Scan Waiting-on-you trees in `text`. `archived` marks quoted archive text,
/// whose heads no longer ask anything.
fn scan_waiting(text: &str, archived: bool, location_prefix: &str, out: &mut Observations) {
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if !line.contains(WAITING_ITEM_MARKER) {
            index += 1;
            continue;
        }
        let Some(body) = list_body(line) else {
            index += 1;
            continue;
        };
        scan_waiting_item(&lines, index, body, archived, location_prefix, out);
        index += 1;
    }
}

fn scan_waiting_item(
    lines: &[&str],
    head: usize,
    body: &str,
    archived: bool,
    location_prefix: &str,
    out: &mut Observations,
) {
    let head_indent = indent_of(lines[head]);
    let (own, grafts) = head_ids(body);
    let (_, struck) = work_text(body);
    let question_text = strip_decorations(body);
    if !struck
        && !archived
        && let Some(own) = &own
        && waiting_head_busy(body)
    {
        out.busy_waiting_ids.insert(own.clone());
    }
    out.questions.push(QuestionObs {
        id: own.clone(),
        text: question_text.clone(),
        source: QuestionSource::Waiting,
        open: !struck && !archived,
        line: head + 1,
        references: references(
            body.split(WAITING_ITEM_MARKER).next().unwrap_or(body),
            own.as_deref(),
        ),
        stamp: stamp_of(body),
    });
    if let Some(own) = &own {
        for graft in &grafts {
            out.links.push(LinkObs {
                from: graft.clone(),
                to: own.clone(),
                kind: LinkKind::RequestGraft,
            });
        }
    }
    // Answers address the item's own id; grafted request ids join the same
    // decision only when the ledger treats the graft as identity.
    let mut targets: Vec<String> = own.iter().cloned().collect();
    if targets.is_empty() {
        targets.extend(grafts.iter().cloned());
    }
    let label = own.as_deref().map_or_else(
        || format!("“{}”", short(&question_text)),
        |id| format!("[#{id}]"),
    );
    let mut row = head + 1;
    while row < lines.len() {
        let line = lines[row];
        if line.trim().is_empty() {
            row += 1;
            continue;
        }
        let indent = indent_of(line);
        if indent <= head_indent {
            break;
        }
        if line.contains(WAITING_ITEM_MARKER) {
            // A nested item owns its own subtree.
            let nested = indent;
            row += 1;
            while row < lines.len()
                && (lines[row].trim().is_empty() || indent_of(lines[row]) > nested)
            {
                row += 1;
            }
            continue;
        }
        if let Some(row_body) = list_body(line) {
            let (work, _) = work_text(row_body);
            if let Some(rest) = work.strip_prefix("**Request**:") {
                if let Some(own) = &own {
                    for id in bracket_ids(rest).into_iter().filter(|id| is_request_id(id)) {
                        out.links.push(LinkObs {
                            from: id.clone(),
                            to: own.clone(),
                            kind: LinkKind::RequestGraft,
                        });
                        if targets.is_empty() {
                            targets.push(id);
                        }
                    }
                }
            } else {
                record_operator_row(
                    row_body,
                    row + 1,
                    &PacketScope {
                        own: own.as_deref(),
                        targets: &targets,
                        head_text: &question_text,
                        location: &format!("{location_prefix}under {label}"),
                        consumed: archived,
                        dispatch: !archived,
                    },
                    out,
                );
            }
        }
        row += 1;
    }
}

/// Where an operator row in a packet sits: the packet's own id, the ids its
/// answers address, and how the row is recorded.
struct PacketScope<'a> {
    own: Option<&'a str>,
    targets: &'a [String],
    /// Question text, keying the answer when the packet has no id.
    head_text: &'a str,
    location: &'a str,
    /// Every answer in the packet is consumed (archived, or the queue row
    /// that carried the packet is struck).
    consumed: bool,
    /// The packet is its target's dispatch path (not an archive quote).
    dispatch: bool,
}

/// Record one packet row as an operator answer or a `same as [#id]` merge;
/// generated rows are ignored.
fn record_operator_row(row_body: &str, line: usize, scope: &PacketScope, out: &mut Observations) {
    let Some((text, consumed)) = operator_row(row_body) else {
        return;
    };
    if let Some(same) = same_as_target(&text) {
        if let Some(own) = scope.own {
            out.links.push(LinkObs {
                from: own.to_string(),
                to: same,
                kind: LinkKind::OperatorSameAs,
            });
        }
        return;
    }
    let (targets, answer, in_own) = match addressed_answer(&text) {
        Some((id, answer)) if Some(id.as_str()) != scope.own => (vec![id], answer, false),
        Some((_, answer)) => (scope.targets.to_vec(), answer, true),
        None => (scope.targets.to_vec(), text, true),
    };
    let (explicit, answer) = explicit_relation(&answer);
    let (links, link_only) = relation_links(&answer);
    if let Some(from) = targets.first() {
        for (kind, to) in links {
            if &to != from {
                out.links.push(LinkObs {
                    from: from.clone(),
                    to,
                    kind,
                });
            }
        }
    }
    if link_only {
        return;
    }
    out.answers.push(AnswerObs {
        targets,
        head_text: scope.own.is_none().then(|| scope.head_text.to_string()),
        text: answer,
        location: scope.location.to_string(),
        line,
        consumed: consumed || scope.consumed,
        in_own_packet: in_own && scope.dispatch,
        explicit,
    });
}

/// A waiting packet's `**Response:** <question>` row (generated: it carries
/// the waiting-metadata marker or the `▫️` reply marker).
fn packet_question(body: &str) -> Option<String> {
    let (work, _) = work_text(body);
    let rest = work.strip_prefix("**Response:**")?;
    let generated = body.contains("<!-- agent-doc:waiting-metadata -->")
        || body.ends_with('▫')
        || body.ends_with("▫️");
    generated.then(|| strip_decorations(rest))
}

/// The id a queue row addresses: `do [#id]`, `re [#id]`, `[#id]: …` or a
/// leading `[#id] …`. Attempt handles and queue handles are not ids. Also
/// reports whether the row is struck.
fn queue_row_owner(body: &str) -> Option<(String, bool)> {
    let (work, struck) = work_text(body);
    let lower = work.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("do ")
        .or_else(|| lower.strip_prefix("re "))
        .map_or(lower.as_str(), str::trim_start);
    if !rest.starts_with("[#") {
        return None;
    }
    let id = bracket_ids(rest).into_iter().next()?;
    (!is_handle(&id) && rest[2..].starts_with(id.as_str())).then_some((id, struck))
}

/// Line ranges of waiting packets that sit in a queue outside any waiting
/// item: the packet lost its waiting-item marker (a `do [#id]` row now
/// carries it), yet the operator's answer under its `**Action:**` row is
/// still an answer (`#answeroutsidewaiting`).
fn orphan_packet_ranges(
    lines: &[&str],
    waiting: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let is_root = !waiting.iter().any(|range| range.contains(&index))
            && list_body(lines[index]).is_some_and(|body| packet_question(body).is_some());
        if !is_root {
            index += 1;
            continue;
        }
        let indent = indent_of(lines[index]);
        let mut end = index + 1;
        while end < lines.len() && (lines[end].trim().is_empty() || indent_of(lines[end]) > indent)
        {
            end += 1;
        }
        ranges.push(index..end);
        index = end;
    }
    ranges
}

/// Capture the question and operator answers of one orphan packet. The
/// packet belongs to the id its parent queue row addresses; with no such
/// parent the answer is keyed by the question text alone. Only the parent is
/// consulted, never a further ancestor, so an answer is never credited to an
/// unrelated item higher up the queue.
fn scan_orphan_packet(
    lines: &[&str],
    range: std::ops::Range<usize>,
    line_offset: usize,
    out: &mut Observations,
) {
    let root = range.start;
    let indent = indent_of(lines[root]);
    let Some(question_text) = list_body(lines[root]).and_then(packet_question) else {
        return;
    };
    let parent = (0..root)
        .rev()
        .find(|&index| !lines[index].trim().is_empty() && indent_of(lines[index]) < indent);
    let (own, owner_struck) = parent
        .and_then(|index| list_body(lines[index]))
        .and_then(queue_row_owner)
        .map_or((None, false), |(id, struck)| (Some(id), struck));
    if own.is_some() || !question_text.is_empty() {
        out.questions.push(QuestionObs {
            references: references(&question_text, own.as_deref()),
            id: own.clone(),
            text: question_text.clone(),
            source: QuestionSource::Waiting,
            // The live copies are the todo and any waiting item; this packet
            // is what was asked.
            open: false,
            line: line_offset + root + 1,
            stamp: list_body(lines[root]).and_then(stamp_of),
        });
    }
    let targets: Vec<String> = own.iter().cloned().collect();
    let label = own.as_deref().map_or_else(
        || format!("“{}”", short(&question_text)),
        |id| format!("[#{id}]"),
    );
    let location = format!("queue row, under {label} outside a waiting item");
    let scope = PacketScope {
        own: own.as_deref(),
        targets: &targets,
        head_text: &question_text,
        location: &location,
        consumed: owner_struck,
        dispatch: true,
    };
    let mut row = root + 1;
    while row < range.end {
        let line = lines[row];
        if line.contains(WAITING_ITEM_MARKER) {
            let nested = indent_of(line);
            row += 1;
            while row < range.end
                && (lines[row].trim().is_empty() || indent_of(lines[row]) > nested)
            {
                row += 1;
            }
            continue;
        }
        if let Some(row_body) = list_body(line) {
            record_operator_row(row_body, line_offset + row + 1, &scope, out);
        }
        row += 1;
    }
}

fn short(text: &str) -> String {
    let mut out: String = text.chars().take(60).collect();
    if text.chars().count() > 60 {
        out.push('…');
    }
    out
}

/// Byte ranges (as 0-based line ranges) of waiting trees, so queue-row
/// scanning skips packet rows.
fn waiting_line_ranges(lines: &[&str]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].contains(WAITING_ITEM_MARKER) && list_body(lines[index]).is_some() {
            let indent = indent_of(lines[index]);
            let mut end = index + 1;
            while end < lines.len()
                && (lines[end].trim().is_empty() || indent_of(lines[end]) > indent)
            {
                end += 1;
            }
            ranges.push(index..end);
            index = end;
        } else {
            index += 1;
        }
    }
    ranges
}

fn scan_queue_rows(content: &str, out: &mut Observations) {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return;
    };
    for component in components
        .iter()
        .filter(|component| matches!(component.name.as_str(), "queue" | "run"))
    {
        let body = component.content(content);
        let line_offset = content[..component.open_end].lines().count();
        let lines: Vec<&str> = body.lines().collect();
        let ranges = waiting_line_ranges(&lines);
        let orphans = orphan_packet_ranges(&lines, &ranges);
        for range in &orphans {
            scan_orphan_packet(&lines, range.clone(), line_offset, out);
        }
        for (index, line) in lines.iter().enumerate() {
            if ranges.iter().any(|range| range.contains(&index)) {
                continue;
            }
            let Some(row_body) = list_body(line) else {
                continue;
            };
            let (work, struck) = work_text(row_body);
            if !struck && !row_body.contains("~~") {
                for id in bracket_ids(row_body) {
                    out.live_queue_ids.insert(id);
                }
            }
            // Orphan packet rows were captured above.
            if orphans.iter().any(|range| range.contains(&index)) {
                continue;
            }
            if work.contains("auto-struck")
                || row_body.ends_with('▫')
                || row_body.ends_with("▫️")
                || row_body.contains("<!-- agent-doc:")
            {
                continue;
            }
            if let Some((id, answer)) = addressed_answer(&work) {
                if let Some(same) = same_as_target(&answer) {
                    out.links.push(LinkObs {
                        from: id,
                        to: same,
                        kind: LinkKind::OperatorSameAs,
                    });
                    continue;
                }
                let (explicit, answer) = explicit_relation(&answer);
                let (links, link_only) = relation_links(&answer);
                for (kind, to) in links {
                    if to != id {
                        out.links.push(LinkObs {
                            from: id.clone(),
                            to,
                            kind,
                        });
                    }
                }
                if link_only {
                    continue;
                }
                out.answers.push(AnswerObs {
                    targets: vec![id],
                    head_text: None,
                    text: answer,
                    location: if struck {
                        "consumed queue row".into()
                    } else {
                        "queue row".into()
                    },
                    line: line_offset + index + 1,
                    consumed: struck,
                    in_own_packet: false,
                    explicit,
                });
            }
        }
    }
}

fn scan_todos(content: &str, out: &mut Observations) {
    let Ok(Some(todo)) = agent_doc_element::todo::find_todo_component(content) else {
        return;
    };
    let body = todo.content(content);
    let line_offset = content[..todo.open_end].lines().count();
    for (index, line) in body.lines().enumerate() {
        let Some(parsed) = agent_doc_element::todo::parse_todo_line(line) else {
            continue;
        };
        let id = parsed.own_id.map(str::to_ascii_lowercase);
        let text = parsed.action.to_string();
        out.questions.push(QuestionObs {
            references: references(&text, id.as_deref()),
            stamp: stamp_of(&text),
            id,
            text,
            source: QuestionSource::Todo,
            open: parsed.state.is_open(),
            line: line_offset + index + 1,
        });
    }
}

/// `#decisionstaleaudit`: every todo row and backlog/review/icebox item with
/// an own id.
fn scan_tracked(content: &str, out: &mut Observations) {
    if let Ok(Some(todo)) = agent_doc_element::todo::find_todo_component(content) {
        for line in todo.content(content).lines() {
            let Some(parsed) = agent_doc_element::todo::parse_todo_line(line) else {
                continue;
            };
            let Some(id) = parsed.own_id else { continue };
            out.tracked.push(TrackedItemObs {
                component: TrackedComponent::Todo,
                id: id.to_ascii_lowercase(),
                text: parsed.action.to_string(),
                open: parsed.state.is_open(),
            });
        }
    }
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return;
    };
    for component in &components {
        let kind = if agent_doc_element::element::is_review_component(&component.name) {
            TrackedComponent::Review
        } else if agent_doc_element::element::is_backlog_component(&component.name) {
            TrackedComponent::Backlog
        } else if agent_doc_element::element::is_icebox_component(&component.name) {
            TrackedComponent::Icebox
        } else {
            continue;
        };
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(component.content(content));
        for item in items {
            let id = agent_doc_element_backlog::backlog::normalize_pending_id(&item.id);
            if id.is_empty() {
                continue;
            }
            out.tracked.push(TrackedItemObs {
                component: kind,
                id,
                open: !item.is_done(),
                text: item.text,
            });
        }
    }
}

fn scan_request_packets(content: &str, out: &mut Observations) {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return;
    };
    for component in components
        .iter()
        .filter(|component| component.name == "exchange")
    {
        let body = component.content(content);
        for packet in
            agent_doc_queue::operator_request_packet::blocking_operator_request_packets(body)
        {
            let id = packet.id.to_ascii_lowercase();
            let mut references = Vec::new();
            if let Some(queue_item) = &packet.queue_item {
                for origin in bracket_ids(queue_item) {
                    out.links.push(LinkObs {
                        from: id.clone(),
                        to: origin.clone(),
                        kind: LinkKind::QueueItem,
                    });
                    references.push(origin);
                }
            }
            let mut text = packet.request.clone();
            if !packet.choice_labels.is_empty() {
                text.push_str(&format!(" (choices: {})", packet.choice_labels.join(", ")));
            }
            out.questions.push(QuestionObs {
                id: Some(id),
                text,
                source: QuestionSource::Request,
                // The packet is the history of what was asked; its todo (and
                // waiting item) are the live copies.
                open: false,
                line: 0,
                references,
                stamp: None,
            });
        }
    }
}

/// Request ids that embed a known item id (`subagent-operator-request-<item>-…`).
fn infix_links(out: &mut Observations) {
    let known: std::collections::BTreeSet<String> = out
        .questions
        .iter()
        .filter_map(|question| question.id.clone())
        .filter(|id| !is_request_id(id))
        .collect();
    let request_ids: std::collections::BTreeSet<String> = out
        .questions
        .iter()
        .filter_map(|question| question.id.clone())
        .chain(out.answers.iter().flat_map(|answer| answer.targets.clone()))
        .filter(|id| is_request_id(id))
        .collect();
    for request in request_ids {
        let rest = &request[REQUEST_ID_PREFIX.len()..];
        if let Some(item) = known
            .iter()
            .filter(|item| rest == item.as_str() || rest.starts_with(&format!("{item}-")))
            .max_by_key(|item| item.len())
        {
            out.links.push(LinkObs {
                from: request.clone(),
                to: item.clone(),
                kind: LinkKind::RequestInfix,
            });
        }
    }
}

/// Split archive text into quoted waiting-packet blocks and unquote them.
fn archive_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.contains(ARCHIVE_MARKER) {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(String::new());
            continue;
        }
        match (&mut current, line.strip_prefix("  > ")) {
            (Some(block), Some(unquoted)) => {
                block.push_str(unquoted);
                block.push('\n');
            }
            (Some(_), None) if line.trim() == ">" || line.trim().is_empty() => {}
            (Some(_), None) => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
            }
            (None, _) => {}
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

/// Observe one session document plus the text of its done archives.
pub fn observe(content: &str, archives: &[&str]) -> Observations {
    let mut out = Observations::default();
    scan_todos(content, &mut out);
    scan_tracked(content, &mut out);
    scan_waiting(content, false, "", &mut out);
    scan_queue_rows(content, &mut out);
    scan_request_packets(content, &mut out);
    for block in archive_blocks(content) {
        scan_waiting(&block, true, "done archive, ", &mut out);
    }
    for archive in archives {
        for block in archive_blocks(archive) {
            scan_waiting(&block, true, "done archive, ", &mut out);
        }
    }
    // Quoted archive heads were also seen by the inline scan when the archive
    // is inline; the inline scan skips them because quoted lines are not list
    // rows, so nothing is double counted.
    infix_links(&mut out);
    out
}

/// Observe the questions a closeout is about to add (packets in the response
/// and `--todo-add` texts), without touching the document.
pub fn observe_new_questions(response: &str, todo_adds: &[String]) -> Vec<crate::NewQuestion> {
    let mut questions = Vec::new();
    for packet in
        agent_doc_queue::operator_request_packet::blocking_operator_request_packets(response)
    {
        let id = packet.id.to_ascii_lowercase();
        let references = packet
            .queue_item
            .as_deref()
            .map(bracket_ids)
            .unwrap_or_default();
        let mut full_text = packet.request.clone();
        // A copied stamp names the decision but is not the agent citing it.
        if let Some(context) = &packet.context {
            full_text.push(' ');
            full_text.push_str(context);
        }
        for row in &packet.choice_rows {
            full_text.push(' ');
            full_text.push_str(row);
        }
        questions.push(crate::NewQuestion {
            question: QuestionObs {
                id: Some(id),
                stamp: stamp_of(&packet.request),
                text: packet.request.clone(),
                source: QuestionSource::Request,
                open: true,
                line: 0,
                references,
            },
            full_text: strip_decision_stamps(&full_text),
        });
    }
    for add in todo_adds {
        let line = format!("- [ ] {}", add.trim());
        let (id, text) = match agent_doc_element::todo::parse_todo_line(&line) {
            Some(parsed) => (
                parsed.own_id.map(str::to_ascii_lowercase),
                parsed.action.to_string(),
            ),
            None => (None, add.clone()),
        };
        questions.push(crate::NewQuestion {
            question: QuestionObs {
                references: references(&text, id.as_deref()),
                stamp: stamp_of(&text),
                id,
                text,
                source: QuestionSource::Todo,
                open: true,
                line: 0,
            },
            full_text: strip_decision_stamps(add),
        });
    }
    questions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_text_strips_glyphs_handles_and_strikes() {
        assert_eq!(
            work_text("### 🙋📥 [#subagent-operator-request-x-d1d4]: Pat"),
            (
                "[#subagent-operator-request-x-d1d4]: Pat".to_string(),
                false
            )
        );
        assert_eq!(
            work_text("### ~~[#tdx]: update the version.~~"),
            ("[#tdx]: update the version.".to_string(), true)
        );
        assert_eq!(
            work_text("☑️ [#subagent:c005#1] do [#feat]").0,
            "do [#feat]".to_string()
        );
    }

    #[test]
    fn addressed_answers() {
        assert_eq!(
            addressed_answer("[#abc]: Keep at 0.1.0. [#q-1234]"),
            Some(("abc".into(), "Keep at 0.1.0.".into()))
        );
        assert_eq!(
            addressed_answer(
                "correct_sink: target the main branch [#subagent-operator-request-sink]"
            ),
            Some((
                "subagent-operator-request-sink".into(),
                "correct_sink: target the main branch".into()
            ))
        );
        assert_eq!(
            addressed_answer("do [#subagent-operator-request-sink]"),
            None
        );
        assert_eq!(addressed_answer("plain text"), None);
    }

    #[test]
    fn generated_rows_are_not_operator_answers() {
        assert!(operator_row("**Next:** you — reply ▫️").is_none());
        assert!(operator_row("**Choices:**").is_none());
        assert!(operator_row("☑️ [#subagent:alpha#1] Finish your bundle. [#q-0a3b]").is_none());
        assert!(
            operator_row("~~authorized — auto-struck: completed by #x (#qftbklgstrike)~~")
                .is_none()
        );
        assert!(operator_row("do [#q-c1a250d0313a]").is_none());
        assert_eq!(
            operator_row("~~Pat is the Alpha release. [#q-9079]~~"),
            Some(("Pat is the Alpha release.".into(), true))
        );
        assert_eq!(
            operator_row("JWS makes the cap moot."),
            Some(("JWS makes the cap moot.".into(), false))
        );
    }

    #[test]
    fn explicit_change_and_amend_lead_ins() {
        use crate::RevisionRelation::*;
        assert_eq!(
            explicit_relation("change: Keep the approver as is"),
            (Some(Supersedes), "Keep the approver as is".to_string())
        );
        assert_eq!(
            explicit_relation("Amend: but keep the release App private"),
            (Some(Amends), "but keep the release App private".to_string())
        );
        // A lead-in alone, or the word inside a sentence, is not a form.
        assert_eq!(explicit_relation("change:"), (None, "change:".to_string()));
        assert_eq!(
            explicit_relation("No change: keep it"),
            (None, "No change: keep it".to_string())
        );
    }

    #[test]
    fn depends_on_and_conflicts_with_links() {
        assert_eq!(
            relation_links("depends on [#tdpublic]"),
            (
                vec![(LinkKind::OperatorDependsOn, "tdpublic".to_string())],
                true
            )
        );
        assert_eq!(
            relation_links("This assumes [#a] and conflicts with [#b]."),
            (
                vec![
                    (LinkKind::OperatorDependsOn, "a".to_string()),
                    (LinkKind::OperatorConflictsWith, "b".to_string()),
                ],
                true
            )
        );
        let (links, link_only) = relation_links("No token in install.sh; depends on [#tdpublic]");
        assert_eq!(links.len(), 1);
        assert!(!link_only);
        // Needs an explicit id right after the phrase.
        assert_eq!(
            relation_links("it depends on the port"),
            (Vec::new(), false)
        );
        assert_eq!(relation_links("misassumes [#x]"), (Vec::new(), false));
    }

    #[test]
    fn head_ids_prefer_leading_then_trailing() {
        let (own, grafts) = head_ids(
            "🙋☑️ [#subagent:c421#1] ⟦☑️ you: 0⟧ Log in to Unity Hub <!-- agent-doc:waiting-item --> [#tdunity] ⟦effect:2⟧ [#subagent-operator-request-unity-license]",
        );
        assert_eq!(own.as_deref(), Some("tdunity"));
        assert_eq!(
            grafts,
            vec!["subagent-operator-request-unity-license".to_string()]
        );
        let (own, grafts) = head_ids(
            "~~[#tdprov] Implement ADR 0041 <!-- agent-doc:waiting-item --> [#subagent-operator-request-prov-cap]~~",
        );
        assert_eq!(own.as_deref(), Some("tdprov"));
        assert_eq!(
            grafts,
            vec!["subagent-operator-request-prov-cap".to_string()]
        );
    }
}
