//! `#exchangeresponseid`: every response has a corresponding `agent:exchange`
//! entry, even when it is written as a response tree under a queue row.
//!
//! Live 2026-11-24 (`tasks/agent-doc/agent-doc-bugs.md`): `queue
//! report-complete` for `subagent:selfsteer3` wrote the row's tree
//! `**Response [#r-35be26482ddb]**`, whose `**Details:**` row sent the reader
//! to "the `subagent:selfsteer3` hand-back in agent:exchange", and the
//! completion cell holding that hand-back. The cell never named
//! `[#r-35be26482ddb]`. Once the operator deleted the finished tree, the id
//! was in no exchange entry at all, and nothing linked the tree to its
//! hand-back while both were present.
//!
//! [`link_response_trees_to_exchange`] keeps the invariant: each queue
//! response tree's `[#r-…]` id is carried by an exchange entry, on its
//! heading, where the response index and `response-toc` resolve it.
//!
//! - **Hand-back trees.** The row's completion cell, or the hand-back record
//!   it was retired to, is tagged with the id. The link is proven, never
//!   guessed: the id is recomputed from that cell's attested hand-back and
//!   the row's scope, exactly as the strike minted it. The tree's Details
//!   row names the same id.
//! - **Other trees** (an `--ack-text` summary, a failure, a stop
//!   explanation, an already-done strike) have no exchange response of their
//!   own. When one is newly minted (absent from `before`), a binary-owned
//!   queue-response record carrying the id is appended
//!   ([`agent_doc_document::queue_response_record`]).
//!
//! A write that mints trees passes the document it started from as `before`
//! and links only the trees absent from it. With no `before` (repair: `queue
//! sync`), every tree is considered, but only proven links are added: no
//! record is invented for a tree an older binary wrote.

use std::collections::BTreeSet;

use agent_doc_document::subagent_completion::split_heading_response_id;
use agent_doc_document::worker_handback::WorkerHandback;

/// What [`link_response_trees_to_exchange`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExchangeResponseLink {
    pub document: String,
    /// Ids whose existing completion cell or hand-back record now carries
    /// them on its heading.
    pub annotated: Vec<String>,
    /// Ids that got a new queue-response record.
    pub recorded: Vec<String>,
    /// Details rows that now name their tree's id.
    pub details: usize,
}

impl ExchangeResponseLink {
    pub fn changed(&self) -> bool {
        !self.annotated.is_empty() || !self.recorded.is_empty() || self.details > 0
    }
}

const DETAILS_ANCHOR: &str = " hand-back in agent:exchange";

/// One response tree under a queue row.
struct Tree {
    id: String,
    row_text: String,
    row_start: usize,
    /// The tree's body lines, relative indent kept, Guidance left out.
    body: Vec<String>,
    /// Byte offset just after the Details anchor, when the Details row
    /// points at a hand-back without naming the id yet.
    details_insert_at: Option<usize>,
}

fn queue_trees(document: &str) -> Vec<Tree> {
    let Some(items) = crate::derived_memo::queue_items(document) else {
        return Vec::new();
    };
    let mut trees = Vec::new();
    for item in items.iter() {
        let Some(response) =
            crate::response_tree_sync::own_response(document, item.start_byte, item.end_byte)
        else {
            continue;
        };
        let Some(id) = response.id else {
            continue;
        };
        let mut body = Vec::new();
        let mut skip_below = None;
        let mut details_insert_at = None;
        for line in &response.block {
            if line.text.is_empty() {
                continue;
            }
            if skip_below.is_some_and(|indent| line.indent > indent) {
                continue;
            }
            skip_below = None;
            if line.indent == response.detail_indent && line.text.starts_with("- **Guidance:**") {
                skip_below = Some(line.indent);
                continue;
            }
            if line.indent == response.detail_indent
                && line.text.starts_with("- **Details:**")
                && let Some(at) = line.text.find(DETAILS_ANCHOR)
                && !line.text.contains(id)
            {
                details_insert_at = Some(line.start + line.indent + at + DETAILS_ANCHOR.len());
            }
            body.push(format!(
                "{}{}",
                " ".repeat(line.indent.saturating_sub(response.detail_indent)),
                line.text
            ));
        }
        trees.push(Tree {
            id: id.to_string(),
            row_text: item.text.clone(),
            row_start: item.start_byte,
            body,
            details_insert_at,
        });
    }
    trees
}

/// The exchange component's body range in `document`.
fn exchange_range(document: &str) -> Option<std::ops::Range<usize>> {
    let components = agent_doc_element::element::parse(document).ok()?;
    let mut exchanges = components
        .iter()
        .filter(|component| component.name == "exchange");
    let exchange = exchanges.next()?;
    exchanges
        .next()
        .is_none()
        .then_some(exchange.open_end..exchange.close_start)
}

/// Response ids an exchange entry carries: a heading's trailing id, or a
/// binary lookup marker line (`**Response [#r-…]**`) outside a quote.
pub fn exchange_response_ids(exchange_body: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in exchange_body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.strip_suffix(" (HEAD)").unwrap_or(trimmed);
            let heading = heading.strip_suffix("~~").unwrap_or(heading);
            if let Some(id) = split_heading_response_id(heading).1 {
                ids.insert(id.to_string());
            }
        } else if let Some(id) =
            agent_doc_markdown_ast::exchange_tree::response_id_from_lookup_marker(trimmed)
        {
            ids.insert(id.to_string());
        }
    }
    ids
}

/// Link every queue response tree of `after` to an `agent:exchange` entry
/// carrying its id (see the module docs). `before`, when given, is the
/// document before the write that minted trees: a tree absent from it that
/// has no provable hand-back gets a queue-response record. `None` when there
/// is nothing to change or the document has no single exchange component.
pub fn link_response_trees_to_exchange(
    before: Option<&str>,
    after: &str,
) -> Option<ExchangeResponseLink> {
    let exchange = exchange_range(after)?;
    let trees = queue_trees(after);
    if trees.is_empty() {
        return None;
    }
    let exchange_body = &after[exchange.clone()];
    let mut resolved = exchange_response_ids(exchange_body);
    let before_ids = before.map(|before| {
        queue_trees(before)
            .into_iter()
            .map(|tree| tree.id)
            .collect::<BTreeSet<_>>()
    });
    let exchange_first_line = after[..exchange.start].matches('\n').count();
    let records = agent_doc_document::subagent_completion::handback_records(after);
    let handbacks = crate::queue_consume::DocumentHandbacks::new(after);

    enum Link {
        Cell(String),
        Record(usize),
        New(String),
    }
    let mut links = Vec::<(String, Link)>::new();
    // A write that mints trees links only the trees it minted; older trees
    // are the repair pass's (`before` = `None`).
    let in_scope = |id: &str| {
        before_ids
            .as_ref()
            .is_none_or(|before_ids| !before_ids.contains(id))
    };
    for tree in &trees {
        if !in_scope(&tree.id)
            || resolved.contains(&tree.id)
            || links.iter().any(|(id, _)| *id == tree.id)
        {
            continue;
        }
        let scope = handbacks.scope_for_row(&tree.row_text, tree.row_start);
        if let Some(found) = handbacks.for_row(&tree.row_text, tree.row_start)
            && crate::queue_consume::handback_response_id(&scope, &found.handback) == tree.id
        {
            links.push((tree.id.clone(), Link::Cell(found.key)));
            continue;
        }
        let record = records.iter().find(|record| {
            record.line >= exchange_first_line
                && record.response_id.is_none()
                && crate::queue_claim::completion_evidence_matches_row(&record.item, &tree.row_text)
                && WorkerHandback::parse(&record.response).is_some_and(|handback| {
                    crate::queue_consume::handback_response_id(&scope, &handback) == tree.id
                })
        });
        if let Some(record) = record {
            links.push((tree.id.clone(), Link::Record(record.line)));
            continue;
        }
        if before_ids.is_some() {
            let item = crate::struck_prompt_response::row_prose(&tree.row_text);
            let item = if item.trim().is_empty() {
                tree.row_text.trim().to_string()
            } else {
                item
            };
            if let Some(record) =
                agent_doc_document::queue_response_record::record(&item, &tree.id, &tree.body)
            {
                links.push((tree.id.clone(), Link::New(record)));
            }
        }
    }

    let mut out = after.to_string();
    let mut report = ExchangeResponseLink::default();
    for (id, link) in &links {
        let updated = match link {
            Link::Cell(key) => {
                agent_doc_document::subagent_completion::annotate_completion_cell_response_id(
                    &out, key, id,
                )
            }
            Link::Record(line) => {
                agent_doc_document::subagent_completion::annotate_handback_record_response_id(
                    &out, *line, id,
                )
            }
            Link::New(_) => continue,
        };
        if let Some(updated) = updated {
            out = updated;
            report.annotated.push(id.clone());
            resolved.insert(id.clone());
        }
    }
    // A Details row points at its hand-back's exact entry once that entry
    // carries the id. An annotation above can shift the queue (an exchange
    // placed before it), so the offsets are re-derived after any edit.
    let trees = if out == after {
        trees
    } else {
        queue_trees(&out)
    };
    let mut details = trees
        .iter()
        .filter(|tree| in_scope(&tree.id) && resolved.contains(&tree.id))
        .filter_map(|tree| tree.details_insert_at.map(|at| (at, tree.id.clone())))
        .collect::<Vec<_>>();
    details.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, id) in details {
        out.insert_str(at, &format!(" under {id}"));
        report.details += 1;
    }
    let new_records = links
        .iter()
        .filter_map(|(id, link)| match link {
            Link::New(record) => Some((id.clone(), record.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !new_records.is_empty() {
        let joined = new_records
            .iter()
            .map(|(_, record)| *record)
            .collect::<Vec<_>>()
            .join("\n\n");
        if let Some(appended) = append_to_exchange(&out, &joined) {
            out = appended;
            report
                .recorded
                .extend(new_records.into_iter().map(|(id, _)| id));
        }
    }
    report.document = out;
    report.changed().then_some(report)
}

/// `document` with `block` appended to its exchange body, before the last
/// boundary marker when one exists, separated from the text around it by a
/// blank line.
fn append_to_exchange(document: &str, block: &str) -> Option<String> {
    let exchange = exchange_range(document)?;
    let body = &document[exchange.clone()];
    let mut offset = 0usize;
    let mut boundary = None;
    for line in body.split_inclusive('\n') {
        if line.trim().starts_with("<!-- agent:boundary:") {
            boundary = Some(offset);
        }
        offset += line.len();
    }
    let insert_at = exchange.start + boundary.unwrap_or(body.len());
    let mut out = String::with_capacity(document.len() + block.len() + 4);
    out.push_str(&document[..insert_at]);
    if insert_at > exchange.start {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") && !out[exchange.start..].trim().is_empty() {
            out.push('\n');
        }
    }
    out.push_str(block);
    out.push('\n');
    let rest = &document[insert_at..];
    let next = rest.lines().next().unwrap_or("").trim();
    if !next.is_empty() && !next.starts_with("<!-- agent:") && !next.starts_with("<!-- /agent:") {
        out.push('\n');
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
#[path = "exchange_response_link_tests.rs"]
mod tests;
