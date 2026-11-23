//! `#nestedreplyfollowup`: an operator reply typed under a struck row's
//! response tree becomes its own top-level follow-up row.
//!
//! Without this, a reply under `**Response [#r-…]**` is admitted where it
//! stands, as a nested leaf whose work identity is its nest vector, and each
//! round of replies nests one level deeper. This pass moves one reply line in
//! one edit:
//!
//! - a new top-level row `- [#<origin>] follow-up: <verbatim reply> [#q-…]`
//!   lands right after the origin's top-level tree, after any follow-ups
//!   already chained to the same origin;
//! - the reply line becomes an inert link row
//!   `- ↪ [#q-…] <!-- agent-doc:followup-link -->` naming that row, never a
//!   copy of the text.
//!
//! A reply under a follow-up row's own response chains to the same origin id,
//! so the chain stays flat and in order. The pass is pure and fails closed:
//! every condition it cannot prove leaves the reply where it is, and the IO
//! caller supplies the two gates this module cannot see (whether any claim,
//! attempt, lineage record, or receipt holds the reply's nest identity, and
//! whether the line is stable rather than being typed). A second pass over its
//! own output changes nothing.
//!
//! Only a reply whose owning row is struck moves. A reply under a live row may
//! be steering for a running worker (`#nestedclaimsteer`) or an answer, and a
//! Waiting-on-you packet keeps its own answer semantics (`#waitingitemstatus`).

use std::ops::Range;

use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_markdown_ast::overlay::FOLLOWUP_LINK_MARKER;

use crate::document_queue::{self, QueueEntry, QueuePrompt};

/// The glyph a link row leads with.
pub const FOLLOWUP_LINK_GLYPH: &str = "\u{21aa}";

/// Upper bound on moves in one pass. Each move re-reads the document, and a
/// gate that keeps failing never moves a row, so this only bounds a pathological
/// document.
const MAX_MOVES_PER_PASS: usize = 32;

/// The work text of an inert link row naming `id` (after its `- ` bullet).
pub fn followup_link_row_text(id: &str) -> String {
    format!("{FOLLOWUP_LINK_GLYPH} [#{id}] {FOLLOWUP_LINK_MARKER}")
}

/// The id a link row names, for a list item's content (`↪ [#q-…] <marker>`).
pub fn followup_link_target(item_content: &str) -> Option<&str> {
    let rest = item_content
        .trim()
        .strip_suffix(FOLLOWUP_LINK_MARKER)?
        .trim_end()
        .strip_prefix(FOLLOWUP_LINK_GLYPH)?
        .trim_start()
        .strip_prefix("[#")?
        .strip_suffix(']')?;
    (!rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
    .then_some(rest)
}

/// One nested reply the gates are asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedReplyRow {
    /// The reply's claim identity (its nest vector), as a claim would key it.
    pub identity: QueueItemIdentity,
    /// The reply's nest vector (`root: … : leaf`).
    pub nest_vector: String,
    /// The operator's text: no cosmetic `📥`/`👀` prefix and no trailing
    /// binary-admitted `[#q-…]` id.
    pub reply: String,
    /// The reply's whole source line, without its line terminator.
    pub line: String,
    /// The claim identity of the row that owns the response.
    pub owner_identity: QueueItemIdentity,
}

/// What this pure pass cannot decide alone.
pub struct NestedReplyGates<'a> {
    /// `true` when any claim, attempt, lineage record, or release receipt
    /// holds the reply (by identity or text), or a live claim or pending
    /// receipt holds the row that owns its response. A held reply never
    /// moves.
    pub held: &'a dyn Fn(&NestedReplyRow) -> bool,
    /// `true` when the line is stable: present unchanged in the stable
    /// baseline and not being typed. An unstable row does not move yet.
    pub stable: &'a dyn Fn(&NestedReplyRow) -> bool,
}

/// One reply this pass moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedNestedReply {
    /// The origin id the new row names (`[#<origin>] follow-up:`).
    pub origin_id: String,
    /// The work text of the row that owns the response.
    pub owner_row: String,
    /// The response the reply was typed under (`r-…`).
    pub response_id: String,
    /// The reply as the operator wrote it.
    pub reply: String,
    /// The reply row's text before the move (after its bullet).
    pub reply_row: String,
    /// The reply's nest vector before the move.
    pub nest_vector: String,
    /// The id of the top-level row that now carries the reply.
    pub followup_id: String,
    /// That row's text after its `- ` bullet.
    pub followup_row: String,
    /// `true` when an equal chained follow-up already existed and the reply
    /// was only linked to it.
    pub linked_existing: bool,
    /// `true` when the owner had been reopened only by this reply and was
    /// struck again once the reply left its tree.
    pub owner_restruck: bool,
}

/// Move every eligible nested reply of `document`. Returns the new document
/// and the moves, or `None` when nothing moved.
pub fn move_nested_replies(
    document: &str,
    document_id: &str,
    gates: &NestedReplyGates<'_>,
) -> Option<(String, Vec<MovedNestedReply>)> {
    let mut content = document.to_string();
    let mut moved = Vec::new();
    while moved.len() < MAX_MOVES_PER_PASS {
        let Some((next, record)) = move_first_nested_reply(&content, document_id, gates) else {
            break;
        };
        content = next;
        moved.push(record);
    }
    (!moved.is_empty()).then_some((content, moved))
}

fn move_first_nested_reply(
    content: &str,
    document_id: &str,
    gates: &NestedReplyGates<'_>,
) -> Option<(String, MovedNestedReply)> {
    let components = agent_doc_element::element::parse(content).ok()?;
    for component in components.iter().filter(|c| c.name == "queue") {
        // Follow-ups never join a loop (`#loopnoinsert`).
        if crate::queue_graph::is_loop_queue(component) {
            continue;
        }
        let body_start = component.open_end;
        let body = component.content(content);
        let Ok(spans) = document_queue::parse_spans(body) else {
            continue;
        };
        let entries: Vec<QueueEntry> = spans.iter().map(|(entry, _)| entry.clone()).collect();
        for index in 0..entries.len() {
            let Some(candidate) = candidate_at(body, &spans, &entries, index) else {
                continue;
            };
            if (gates.held)(&candidate.row) || !(gates.stable)(&candidate.row) {
                continue;
            }
            if let Some(result) =
                apply_move(content, document_id, body_start, body, &spans, &candidate)
            {
                return Some(result);
            }
        }
    }
    None
}

/// A reply that passed every structural check.
struct Candidate {
    row: NestedReplyRow,
    /// The reply row's text after its bullet, exactly as written.
    reply_row: String,
    /// The reply's own binary-admitted id, when preflight already gave it one.
    reply_id: Option<String>,
    /// The reply line's span in the queue body, including its terminator.
    line_span: Range<usize>,
    /// The line's leading whitespace and bullet (`    - `).
    bullet_prefix: String,
    origin_id: String,
    owner_row: String,
    response_id: String,
    /// Index of the top-level entry whose tree holds the reply.
    root_index: usize,
    /// The owner's line span and strike text, when the owner is a struck row
    /// the reply reopened and no other live row is left under it.
    owner_restrike: Option<(Range<usize>, String)>,
}

fn candidate_at(
    body: &str,
    spans: &[(QueueEntry, Range<usize>)],
    entries: &[QueueEntry],
    index: usize,
) -> Option<Candidate> {
    let QueueEntry::Prompt(prompt) = &entries[index] else {
        return None;
    };
    if prompt.indent == 0 || prompt.multiline || prompt.ordered_marker.is_some() {
        return None;
    }
    // A reply with its own subtree is not one line; leave it.
    if entries
        .get(index + 1)
        .and_then(entry_indent)
        .is_some_and(|next| next > prompt.indent)
    {
        return None;
    }
    let (reply, reply_id) = operator_reply_text(&prompt.text)?;
    if !reply_is_movable(&reply) {
        return None;
    }

    // Walk the ancestors: generated response rows up to the response row,
    // then the row that owns it, then on to the top-level root.
    let mut depth = prompt.indent;
    let mut response: Option<(usize, String)> = None;
    let mut owner: Option<usize> = None;
    let mut root_index = None;
    for earlier in (0..index).rev() {
        let entry = &entries[earlier];
        let Some(indent) = entry_indent(entry) else {
            // A fence or directive line between the reply and its tree.
            return None;
        };
        if indent >= depth {
            continue;
        }
        depth = indent;
        let raw = entry_raw_text(entry);
        if raw.contains("agent-doc:waiting-")
            || crate::waiting_packet::is_waiting_on_you_root_text(&raw)
        {
            return None;
        }
        match entry {
            QueueEntry::Freeform(line) if owner.is_none() => {
                let item = list_item_content(line)?;
                if let Some(id) = canonical_response_row_id(item) {
                    if response.is_none() {
                        response = Some((earlier, id.to_string()));
                    }
                } else if !agent_doc_markdown_ast::overlay::is_generated_response_structure_row(
                    item,
                ) {
                    return None;
                }
            }
            QueueEntry::Freeform(_) => {}
            // The row that owns the response must be the reply's nearest
            // queue-row ancestor: a reply to another reply stays.
            QueueEntry::Prompt(_) | QueueEntry::Completed(_) if owner.is_none() => {
                response.as_ref()?;
                owner = Some(earlier);
            }
            QueueEntry::Prompt(_) | QueueEntry::Completed(_) => {}
            _ => return None,
        }
        if depth == 0 {
            root_index = Some(earlier);
            break;
        }
    }
    let owner_index = owner?;
    let (response_index, response_id) = response?;
    let root_index = root_index?;
    if !matches!(
        entries[root_index],
        QueueEntry::Prompt(_) | QueueEntry::Completed(_)
    ) {
        return None;
    }
    let (owner_prompt, owner_struck) = match &entries[owner_index] {
        QueueEntry::Completed(row) => (row, true),
        QueueEntry::Prompt(row) => (row, false),
        _ => return None,
    };
    // A live owner is moved out of only when the reply reopened it: the
    // structural parent of a struck row is derived open while it has a live
    // leaf. Its response says the work finished, and no attempt marks it.
    if !owner_struck
        && (!owner_response_finished(entries, response_index)
            || agent_doc_document::queue_projection::lifecycle_marker(&owner_prompt.text).is_some()
            || agent_doc_document::queue_projection::owner_projection(&owner_prompt.text).is_some()
            || agent_doc_element_queue::strip_attempt_projection_prefix(&owner_prompt.text)
                != owner_prompt.text)
    {
        return None;
    }
    let origin_id = chained_origin_id(&owner_prompt.text)?;

    let line_span = spans[index].1.clone();
    let line = body.get(line_span.clone())?;
    let line = line.trim_end_matches(['\r', '\n']);
    let bullet_len = bullet_prefix_len(line)?;
    // Byte-for-byte: the parser's text is exactly the line after its bullet.
    if line[bullet_len..] != prompt.text {
        return None;
    }
    let owner_restrike = if owner_struck {
        None
    } else {
        // Re-strike only when the reply is the owner's last live leaf.
        let subtree = owner_index + 1..tree_end_from(entries, owner_index);
        let other_live = subtree
            .filter(|at| *at != index)
            .any(|at| matches!(entries[at], QueueEntry::Prompt(_)));
        if other_live {
            None
        } else {
            Some(struck_owner_line(
                body,
                &spans[owner_index].1,
                owner_prompt,
            )?)
        }
    };
    let identity = crate::queue_claim::claim_identity_at(entries, index)?;
    let owner_identity = crate::queue_claim::claim_identity_at(entries, owner_index)?;
    let nest_vector = document_queue::nest_vector_at(entries, index, &prompt.text)?;
    Some(Candidate {
        row: NestedReplyRow {
            identity,
            nest_vector,
            reply,
            line: line.to_string(),
            owner_identity,
        },
        reply_row: prompt.text.clone(),
        reply_id,
        line_span,
        bullet_prefix: line[..bullet_len].to_string(),
        origin_id,
        owner_row: owner_prompt.text.clone(),
        response_id,
        root_index,
        owner_restrike,
    })
}

/// The owner's response (at `response_index`) has a `**State:**` row saying
/// its work finished: `Complete.`, or `Follow-up work remains.` (its action
/// items are follow-up rows of their own).
fn owner_response_finished(entries: &[QueueEntry], response_index: usize) -> bool {
    let Some(indent) = entry_indent(&entries[response_index]) else {
        return false;
    };
    entries[response_index + 1..]
        .iter()
        .take_while(|entry| entry_indent(entry).is_some_and(|child| child > indent))
        .any(|entry| match entry {
            QueueEntry::Freeform(line) if entry_indent(entry) == Some(indent + 2) => {
                list_item_content(line)
                    .and_then(|item| item.trim().strip_prefix("**State:** "))
                    .is_some_and(|state| {
                        let state = state.trim();
                        state == crate::queue_consume::RESPONSE_STATE_COMPLETE
                            || state == crate::queue_consume::RESPONSE_STATE_FOLLOW_UP
                    })
            }
            _ => false,
        })
}

/// The owner's line with its work text struck (`- ### ~~work~~`), and its
/// span. `None` when the line is not the plain shape this can strike.
fn struck_owner_line(
    body: &str,
    span: &Range<usize>,
    owner: &QueuePrompt,
) -> Option<(Range<usize>, String)> {
    let line = body.get(span.clone())?.trim_end_matches(['\r', '\n']);
    let bullet_len = bullet_prefix_len(line)?;
    let rest = &line[bullet_len..];
    let (heading, work) = match rest.strip_prefix("### ") {
        Some(work) => ("### ", work),
        None => ("", rest),
    };
    if work.is_empty() || work.contains("~~") || work != work.trim() {
        return None;
    }
    if work != owner.text && rest != owner.text {
        return None;
    }
    Some((
        span.start..span.start + line.len(),
        format!("{}{heading}~~{work}~~", &line[..bullet_len]),
    ))
}

fn apply_move(
    content: &str,
    document_id: &str,
    body_start: usize,
    body: &str,
    spans: &[(QueueEntry, Range<usize>)],
    candidate: &Candidate,
) -> Option<(String, MovedNestedReply)> {
    let reply = candidate.row.reply.as_str();
    let line_start = body_start + candidate.line_span.start;
    let line_end = body_start + candidate.line_span.end;
    let terminator = &content[line_start..line_end][candidate.row.line.len()..];

    let (content_with_row, followup_id, followup_row, linked_existing) =
        match existing_chained_followup(spans, &candidate.origin_id, reply) {
            Some(existing) => {
                let (id, row) = existing?;
                if candidate.reply_id.as_ref().is_some_and(|own| *own != id) {
                    // Two ids for one reply: leave it for the operator.
                    return None;
                }
                (content.to_string(), id, row, true)
            }
            None => {
                let title = format!(
                    "[#{}] {} {reply}",
                    candidate.origin_id,
                    agent_doc_element_queue::FOLLOWUP_REFERENCE_KEYWORD
                );
                let row = match &candidate.reply_id {
                    // The reply's own id follows it to its new row.
                    Some(id) => format!("{title} [#{id}]"),
                    None => title.clone(),
                };
                let insert_at = body_start + chain_end(body, spans, candidate);
                // The insertion is after the reply's own tree, so the reply's
                // offsets stay valid.
                if insert_at < line_end {
                    return None;
                }
                let mut inserted = String::with_capacity(content.len() + row.len() + 4);
                inserted.push_str(&content[..insert_at]);
                if insert_at > 0 && !content[..insert_at].ends_with('\n') {
                    inserted.push('\n');
                }
                inserted.push_str("- ");
                inserted.push_str(&row);
                inserted.push('\n');
                inserted.push_str(&content[insert_at..]);
                match &candidate.reply_id {
                    Some(id) => (inserted, id.clone(), row, false),
                    None => {
                        let admitted = crate::prompt_identity::admit_queue_row_id(
                            &inserted,
                            document_id,
                            &title,
                        );
                        let [assignment] = admitted.assignments.as_slice() else {
                            return None;
                        };
                        let followup_row = format!("{title} [#{}]", assignment.id);
                        if !admitted.content.contains(&format!("- {followup_row}\n")) {
                            return None;
                        }
                        (admitted.content, assignment.id.clone(), followup_row, false)
                    }
                }
            }
        };

    // Every edit above lands after the reply line, so its span is unchanged.
    if content_with_row.get(line_start..line_start + candidate.row.line.len())
        != Some(candidate.row.line.as_str())
    {
        return None;
    }
    let mut moved = String::with_capacity(content_with_row.len());
    moved.push_str(&content_with_row[..line_start]);
    moved.push_str(&candidate.bullet_prefix);
    moved.push_str(&followup_link_row_text(&followup_id));
    moved.push_str(terminator);
    moved.push_str(&content_with_row[line_end..]);
    // The owner line is before every other edit, so its span is unchanged.
    if let Some((span, struck)) = &candidate.owner_restrike {
        let owner_start = body_start + span.start;
        let owner_end = body_start + span.end;
        if owner_end > line_start {
            return None;
        }
        moved.replace_range(owner_start..owner_end, struck);
    }
    if !move_verified(&moved, &followup_id, &followup_row) {
        return None;
    }
    Some((
        moved,
        MovedNestedReply {
            origin_id: candidate.origin_id.clone(),
            owner_row: candidate.owner_row.clone(),
            response_id: candidate.response_id.clone(),
            reply: reply.to_string(),
            reply_row: candidate.reply_row.clone(),
            nest_vector: candidate.row.nest_vector.clone(),
            followup_id,
            followup_row,
            linked_existing,
            owner_restruck: candidate.owner_restrike.is_some(),
        },
    ))
}

/// The moved document parses, the link is inert, and exactly one top-level
/// row carries the follow-up id.
fn move_verified(document: &str, followup_id: &str, followup_row: &str) -> bool {
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return false;
    };
    let mut rows = 0usize;
    let mut links = 0usize;
    for component in components.iter().filter(|c| c.name == "queue") {
        let Ok(entries) = document_queue::parse(component.content(document)) else {
            return false;
        };
        for entry in &entries {
            match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                    if prompt.text.contains(FOLLOWUP_LINK_MARKER) {
                        return false;
                    }
                    if prompt.indent == 0
                        && top_level_work(&prompt.text) == followup_row
                        && top_level_stable_id(&prompt.text).as_deref() == Some(followup_id)
                    {
                        rows += 1;
                    }
                }
                QueueEntry::Freeform(line)
                    if list_item_content(line)
                        .and_then(followup_link_target)
                        .is_some_and(|id| id == followup_id) =>
                {
                    links += 1;
                }
                _ => {}
            }
        }
    }
    rows == 1 && links >= 1
}

/// `(response id, summary)` of the response a moved top-level follow-up row
/// came from: the link row naming the row's own id sits under that response.
/// `None` for any other row.
pub fn linked_response_context(entries: &[QueueEntry], row_text: &str) -> Option<(String, String)> {
    let work = top_level_work(row_text);
    agent_doc_element_queue::followup_origin_reference(&work)?;
    let id = agent_doc_element_queue::queue_prompt_stable_id(&work)?;
    let link = entries.iter().position(|entry| {
        matches!(entry, QueueEntry::Freeform(line)
            if list_item_content(line).and_then(followup_link_target) == Some(id.as_str()))
    })?;
    let mut depth = entry_indent(&entries[link])?;
    for earlier in (0..link).rev() {
        let indent = entry_indent(&entries[earlier])?;
        if indent >= depth {
            continue;
        }
        depth = indent;
        let QueueEntry::Freeform(line) = &entries[earlier] else {
            return None;
        };
        let Some(response_id) = list_item_content(line).and_then(canonical_response_row_id) else {
            continue;
        };
        let summary = entries[earlier + 1..]
            .iter()
            .take_while(|entry| entry_indent(entry).is_some_and(|child| child > indent))
            .find_map(|entry| match entry {
                QueueEntry::Freeform(line) if entry_indent(entry) == Some(indent + 2) => {
                    list_item_content(line)?
                        .trim()
                        .strip_prefix("**Summary:** ")
                        .map(|value| value.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_default();
        return Some((response_id.to_string(), summary));
    }
    None
}

/// The origin id a reply under `owner`'s response chains to: a follow-up
/// row's own origin (the chain stays flat), else the row's stable id.
fn chained_origin_id(owner_text: &str) -> Option<String> {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(owner_text.trim());
    if let Some((origin, _)) = agent_doc_element_queue::followup_origin_reference(work) {
        return Some(origin.to_string());
    }
    crate::handback_followups::followup_origin_id(owner_text)
}

/// `Some(Some((id, row)))` when a top-level row already holds
/// `[#<origin>] follow-up: <reply>`; `Some(None)` when it has no id to link
/// to (the move fails closed); `None` when there is none.
fn existing_chained_followup(
    spans: &[(QueueEntry, Range<usize>)],
    origin_id: &str,
    reply: &str,
) -> Option<Option<(String, String)>> {
    let wanted = collapse(reply);
    for (entry, _) in spans {
        let (QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt)) = entry else {
            continue;
        };
        if prompt.indent != 0 {
            continue;
        }
        let work = top_level_work(&prompt.text);
        let Some((origin, item)) = agent_doc_element_queue::followup_origin_reference(&work) else {
            continue;
        };
        if !origin.eq_ignore_ascii_case(origin_id) {
            continue;
        }
        let item = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item);
        if collapse(&item) != wanted {
            continue;
        }
        return Some(top_level_stable_id(&prompt.text).map(|id| (id, work)));
    }
    None
}

/// Body offset where the new row goes: the end of the root's top-level tree,
/// then past every following top-level tree already chained to the origin.
fn chain_end(body: &str, spans: &[(QueueEntry, Range<usize>)], candidate: &Candidate) -> usize {
    let mut next = tree_end(spans, candidate.root_index);
    while let Some((entry, _)) = spans.get(next) {
        let (QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt)) = entry else {
            break;
        };
        let work = top_level_work(&prompt.text);
        let chained = agent_doc_element_queue::followup_origin_reference(&work)
            .is_some_and(|(origin, _)| origin.eq_ignore_ascii_case(&candidate.origin_id));
        if prompt.indent != 0 || !chained {
            break;
        }
        next = tree_end(spans, next);
    }
    match next.checked_sub(1).and_then(|last| spans.get(last)) {
        Some((_, span)) => span.end.min(body.len()),
        None => body.len(),
    }
}

/// Index of the first entry after the subtree of the entry at `index`.
fn tree_end_from(entries: &[QueueEntry], index: usize) -> usize {
    let Some(own) = entry_indent(&entries[index]) else {
        return index + 1;
    };
    let mut next = index + 1;
    while entries
        .get(next)
        .and_then(entry_indent)
        .is_some_and(|indent| indent > own)
    {
        next += 1;
    }
    next
}

/// Index of the first entry after `root`'s tree.
fn tree_end(spans: &[(QueueEntry, Range<usize>)], root: usize) -> usize {
    let mut next = root + 1;
    while let Some((entry, _)) = spans.get(next) {
        match entry_indent(entry) {
            Some(indent) if indent > 0 => next += 1,
            _ => break,
        }
    }
    next
}

/// A top-level row's work text: no `### ` heading, strike, or projection.
fn top_level_work(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("### ").unwrap_or(text).trim();
    let text = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    let text = text
        .strip_prefix("~~")
        .and_then(|inner| inner.rfind("~~").map(|close| &inner[..close]))
        .unwrap_or(text);
    agent_doc_element_queue::strip_attempt_projection_prefix(text)
        .trim()
        .to_string()
}

fn top_level_stable_id(text: &str) -> Option<String> {
    agent_doc_element_queue::queue_prompt_stable_id(&top_level_work(text))
}

/// The operator's text of a nested row and its own binary-admitted id: the
/// binary's cosmetic `📥`/`👀` prefix and trailing `[#q-…]` removed, nothing
/// else. `None` when any other projection (a claim marker, an owner label)
/// is on the row.
fn operator_reply_text(text: &str) -> Option<(String, Option<String>)> {
    use agent_doc_document::queue_projection::QueueLifecycleMarker;
    let mut rest = text;
    for marker in [QueueLifecycleMarker::Pending, QueueLifecycleMarker::Seen] {
        for prefix in [marker.prefix(), marker.default_prefix()] {
            if !prefix.is_empty()
                && let Some(after) = rest.strip_prefix(prefix)
                && after.starts_with(' ')
            {
                rest = after.trim_start_matches(' ');
            }
        }
    }
    if rest.trim().is_empty()
        || rest != rest.trim()
        || agent_doc_document::queue_projection::lifecycle_marker(rest).is_some()
        || agent_doc_document::queue_projection::owner_projection(rest).is_some()
        || agent_doc_element_queue::strip_attempt_projection_prefix(rest) != rest
    {
        return None;
    }
    let work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(rest);
    if work == rest {
        return Some((rest.to_string(), None));
    }
    // Exactly `<work> [#q-…]`, so the work is the operator's bytes.
    let id = rest
        .strip_prefix(work.as_str())?
        .strip_prefix(" [#")?
        .strip_suffix(']')?;
    (id.starts_with("q-") && !work.is_empty() && work == work.trim())
        .then(|| (work.clone(), Some(id.to_string())))
}

/// Whether a reply is plain operator prose this pass may move. Directives,
/// ids, presets, commands, struck text, and markers keep their nested meaning.
fn reply_is_movable(reply: &str) -> bool {
    let lower = reply.to_ascii_lowercase();
    !(reply.contains("[#")
        || reply.contains("<!--")
        || reply.contains("-->")
        || reply.contains("~~")
        || reply.starts_with(['#', '/', '>', '!', '%'])
        || reply.starts_with(FOLLOWUP_LINK_GLYPH)
        || lower.starts_with("do ")
        || lower.starts_with("re ")
        || lower.starts_with("re:")
        || lower.starts_with("operator:")
        || lower.starts_with("human (")
        || lower.starts_with("include #")
        || lower.trim_end_matches('?') == "status"
        || agent_doc_markdown_ast::overlay::is_generated_response_structure_row(reply))
}

/// `r-…` of a canonical `**Response [#r-…]**` row.
fn canonical_response_row_id(item: &str) -> Option<&str> {
    let id = item
        .trim()
        .strip_prefix("**Response ")?
        .strip_suffix("**")?;
    agent_doc_markdown_ast::exchange_tree::is_compact_response_id(id)
        .then(|| id.trim_start_matches("[#").trim_end_matches(']'))
}

fn entry_indent(entry: &QueueEntry) -> Option<usize> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt.indent),
        QueueEntry::Freeform(line) => {
            let first = line.lines().next().unwrap_or_default();
            Some(first.len() - first.trim_start_matches([' ', '\t']).len())
        }
        _ => None,
    }
}

fn entry_raw_text(entry: &QueueEntry) -> String {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => prompt.text.clone(),
        QueueEntry::Freeform(line) => line.clone(),
        _ => String::new(),
    }
}

/// A list line's content after its `- ` / `* ` / `+ ` bullet.
fn list_item_content(line: &str) -> Option<&str> {
    let first = line.lines().next().unwrap_or_default();
    let len = bullet_prefix_len(first)?;
    Some(&first[len..])
}

/// Length of a line's leading whitespace plus its `- ` / `* ` / `+ ` bullet.
fn bullet_prefix_len(line: &str) -> Option<usize> {
    let rest = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - rest.len();
    ["- ", "* ", "+ "]
        .iter()
        .find(|bullet| rest.starts_with(**bullet))
        .map(|bullet| indent + bullet.len())
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "nested_reply_followup_tests.rs"]
mod tests;
