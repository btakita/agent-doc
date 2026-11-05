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
//!   operator text ending in `[#subagent-operator-request-…]`.
//!
//! Links record why two ids may be the same question: a `**Request**: [#req]`
//! graft or trailing request id on an item head, a request id that embeds an
//! item id (`subagent-operator-request-<item>-<suffix>`), a packet's
//! `queue_item`, a `(source: … [#id])` citation, or an operator `same as
//! [#id]` reply. The ledger decides which links are identity.

use crate::text::strip_decorations;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observations {
    pub questions: Vec<QuestionObs>,
    pub answers: Vec<AnswerObs>,
    pub links: Vec<LinkObs>,
    /// Ids a live (unstruck) queue row outside any waiting tree still names:
    /// that row is pending work that carries its answer. Closing the question
    /// would race it.
    pub live_queue_ids: std::collections::BTreeSet<String>,
    /// Ids of open tracked items (`#decisionprune`): unchecked rows in
    /// `agent:todo`, `agent:backlog`, `agent:icebox` and `agent:review`, live
    /// waiting heads, and live queue rows. A decision that `applies_to` one of
    /// them stays in the hot set.
    pub open_item_ids: std::collections::BTreeSet<String>,
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
    if display.is_empty() || bare_directive {
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
    (!answer.is_empty()).then_some((id, answer))
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
            } else if let Some((text, consumed)) = operator_row(row_body) {
                if let Some(same) = same_as_target(&text) {
                    if let Some(own) = &own {
                        out.links.push(LinkObs {
                            from: own.clone(),
                            to: same,
                            kind: LinkKind::OperatorSameAs,
                        });
                    }
                } else {
                    let (row_targets, answer, in_own) = match addressed_answer(&text) {
                        Some((id, answer)) if Some(&id) != own.as_ref() => {
                            (vec![id], answer, false)
                        }
                        Some((_, answer)) => (targets.clone(), answer, true),
                        None => (targets.clone(), text, true),
                    };
                    let (explicit, answer) = explicit_relation(&answer);
                    let (links, link_only) = relation_links(&answer);
                    if let Some(from) = row_targets.first() {
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
                    if !link_only {
                        out.answers.push(AnswerObs {
                            targets: row_targets,
                            head_text: own.is_none().then(|| question_text.clone()),
                            text: answer,
                            location: format!("{location_prefix}under {label}"),
                            line: row + 1,
                            consumed: consumed || archived,
                            in_own_packet: in_own && !archived,
                            explicit,
                        });
                    }
                }
            }
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
            id,
            text,
            source: QuestionSource::Todo,
            open: parsed.state.is_open(),
            line: line_offset + index + 1,
        });
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

/// Components whose unchecked rows are open tracked items.
const TRACKED_COMPONENTS: &[&str] = &["todo", "backlog", "icebox", "review"];

/// The leading own id of a checklist row's text, skipping status glyphs
/// (`☑️ [#id] …`).
fn leading_row_id(action: &str) -> Option<String> {
    let rest = action.trim_start_matches(|c: char| c.is_whitespace() || !c.is_ascii());
    if !rest.starts_with("[#") {
        return None;
    }
    bracket_ids(rest).into_iter().next()
}

fn scan_open_items(content: &str, out: &mut Observations) {
    if let Ok(components) = agent_doc_element::element::parse(content) {
        for component in components
            .iter()
            .filter(|component| TRACKED_COMPONENTS.contains(&component.name.as_str()))
        {
            for line in component.content(content).lines() {
                let Some(parsed) = agent_doc_element::todo::parse_todo_line(line) else {
                    continue;
                };
                if !parsed.state.is_open() {
                    continue;
                }
                let id = parsed
                    .own_id
                    .map(str::to_ascii_lowercase)
                    .or_else(|| leading_row_id(parsed.action));
                if let Some(id) = id {
                    out.open_item_ids.insert(id);
                }
            }
        }
    }
    let open_heads: Vec<String> = out
        .questions
        .iter()
        .filter(|question| question.open && question.source != QuestionSource::Request)
        .filter_map(|question| question.id.clone())
        .collect();
    out.open_item_ids.extend(open_heads);
    out.open_item_ids.extend(out.live_queue_ids.iter().cloned());
}

impl Observations {
    /// Every key this document can name (`#decisionprune`): the id key of
    /// every question, reference, answer target and link endpoint, and the
    /// text key of every question and id-less answer head. The index loads
    /// archived decisions for exactly these keys, so a dormant decision
    /// revives as soon as the document names it again.
    pub fn lookup_keys(&self) -> std::collections::BTreeSet<String> {
        let mut keys = std::collections::BTreeSet::new();
        for question in &self.questions {
            keys.extend(crate::lookup_keys_for(question));
        }
        for answer in &self.answers {
            keys.extend(answer.targets.iter().map(|id| crate::id_key(id)));
            if let Some(key) = answer.head_text.as_deref().and_then(crate::text_key) {
                keys.insert(key);
            }
        }
        for link in &self.links {
            keys.insert(crate::id_key(&link.from));
            keys.insert(crate::id_key(&link.to));
        }
        keys
    }
}

/// Observe one session document plus the text of its done archives.
pub fn observe(content: &str, archives: &[&str]) -> Observations {
    let mut out = Observations::default();
    scan_todos(content, &mut out);
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
    scan_open_items(content, &mut out);
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
                text: packet.request.clone(),
                source: QuestionSource::Request,
                open: true,
                line: 0,
                references,
            },
            full_text,
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
                id,
                text,
                source: QuestionSource::Todo,
                open: true,
                line: 0,
            },
            full_text: add.clone(),
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
