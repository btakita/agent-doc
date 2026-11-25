//! `#dupdone`: a live queue row whose work is already finished is resolved,
//! never left open.
//!
//! Two shapes left a row showing `🙋` "waiting on you" for hours after its
//! work was done (live 2026-11-23, private names replaced):
//!
//! - **A duplicate of finished work.** The coordinator queued one chat request
//!   twice, `[from:chat key=report]` and then `[from:chat key=report2]`. The
//!   second copy was dispatched, finished and struck; the first stayed live.
//! - **An operator's own completion reply.** The operator typed `done` under a
//!   `🙋` `[operator-verify]` row after doing the action. Nothing struck it,
//!   and `queue consume --ack-text 'done'` found no unique executable leaf.
//!
//! [`reconcile_finished_rows`] resolves both, in place, on every preflight and
//! `queue sync` pass. Each strike carries an abbreviated response tree
//! ([`crate::struck_prompt_response::abbreviated_strike_response_lines`]), so
//! no strike is bare (`#strucknoresponse`).
//!
//! # Duplicate of finished work
//!
//! The finished twin is a struck queue row with a response beneath it, or a
//! prompt `agent:exchange` echoes as a consumed `> **Queue prompt:**`. A live
//! row is struck as its duplicate only when all of these hold:
//!
//! - Both rows carry a `[from:<source> key=…]` tag of one source, and the two
//!   keys are one *re-queue family*: one key is the other plus a single digit
//!   (`report` / `report2`). The same key twice is not a re-queue: an inbound
//!   source that sends the same key again (a test failing again) is new work.
//! - Their prose, without tags, ids and projection, is near-identical
//!   ([`STRIKE_SIMILARITY`]).
//! - The live row is a settled top-level row with nothing beneath it: present
//!   in the committed baseline, no attempt in flight, no lifecycle marker a
//!   durable record backs, no open linked Todo, not an operator reply or a
//!   question (`#strucknoresponse`), and not an `operator:` / `human (…):`
//!   action. An operator's own prompt carries no `[from:…]` tag, so it is
//!   never struck by similarity.
//!
//! A row that matches a finished twin less certainly (an untagged row with the
//! same prose, the same key re-sent, a family key with looser prose, or a
//! strike candidate a guard keeps live) is surfaced instead:
//! [`possible_finished_twins`] feeds the Loose ends digest, which names the
//! twin as `possibly done by …` on the row's entry.
//!
//! # Operator completion reply
//!
//! A live `[operator-verify]` row (no inline verdict) whose only direct child
//! is the operator's completion reply (`done`, `done.`, `✅`, `completed`, …)
//! is resolved: the row and the reply are struck and the response quotes the
//! reply. The reply the operator already struck (`~~done~~`) is that same
//! answer, not a separate head: it strikes the parent too. A response tree
//! under the reply is the reply's record, not more work. The reply must be
//! settled (in the committed baseline); every other row beneath the parent
//! must already be struck. Without such an operator answer no agent strikes
//! an operator row.
//!
//! This module is pure: document text in, document text out.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element;
use anyhow::Result;

use crate::followup_owner::{ActionItemOwner, explicit_owner_label, followup_row_action_item};
use crate::queue_inbound::{InboundTag, parse_inbound_tag};
use crate::queue_tree::{QueueNodeState, QueueTree, QueueTreeNode, QueueTreeOptions, queue_tree};

/// The note a duplicate strike bakes inside the strikethrough.
pub const DUPLICATE_STRIKE_MARKER: &str = "(#dupdone)";

/// The note an operator-reply strike bakes inside the strikethrough.
pub const OPERATOR_DONE_MARKER: &str = "(#opdonereply)";

/// Word-set similarity at or above which a re-queued `[from:…]` twin is the
/// same request.
pub const STRIKE_SIMILARITY: f64 = 0.9;

/// Word-set similarity at or above which a family twin is surfaced as a
/// possible duplicate.
const SURFACE_SIMILARITY: f64 = 0.6;

/// Operator replies that say "I did it".
const COMPLETION_REPLIES: [&str; 12] = [
    "done",
    "done.",
    "done!",
    "✅",
    "✔",
    "✔\u{fe0f}",
    "completed",
    "completed.",
    "complete",
    "complete.",
    "finished",
    "finished.",
];

/// The completion word inside `text`, strike wrapper removed.
fn completion_reply_quote(text: &str) -> String {
    let text = agent_doc_element_queue::struck_work_text(text).trim();
    text.strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
        .unwrap_or(text)
        .trim()
        .to_string()
}

/// Whether `text` (a row's work text) is an operator completion reply.
pub fn is_completion_reply(text: &str) -> bool {
    let text = completion_reply_quote(text);
    COMPLETION_REPLIES
        .iter()
        .any(|reply| text.eq_ignore_ascii_case(reply))
}

/// A line under a completion reply that records the reply, not new work.
fn line_is_response_record(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    let content = ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| trimmed.strip_prefix(marker))
        .unwrap_or(trimmed)
        .trim();
    content.starts_with("**Response ")
        || agent_doc_markdown_ast::overlay::is_generated_response_structure_row(content)
}

/// A finished request: the evidence a live row may be a duplicate of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedTwin {
    /// The finished row's first-line work text.
    pub text: String,
    /// The finished row's own id (`q-…` or an authored slug).
    pub id: Option<String>,
    /// The commit that finished it, when the evidence names exactly one.
    pub commit: Option<String>,
    /// Where an `agent:exchange` echo recorded it (the response heading).
    pub exchange_heading: Option<String>,
    tag: Option<InboundTag>,
    words: HashSet<String>,
}

impl FinishedTwin {
    /// `[#id]`, else `[from:<source> key=<key>]`, else the quoted text.
    pub fn handle(&self) -> String {
        match (&self.id, &self.tag) {
            (Some(id), _) => format!("`[#{id}]`"),
            (
                None,
                Some(InboundTag {
                    source,
                    key: Some(key),
                }),
            ) => {
                format!("`[from:{source} key={key}]`")
            }
            _ => format!(
                "“{}”",
                crate::response_remaining::short_row_text(&self.text, 60)
            ),
        }
    }

    /// `done by <handle>/<commit>`, or where the echo is.
    fn done_by(&self) -> String {
        match (&self.commit, &self.exchange_heading) {
            (Some(commit), _) => format!("{}/`{commit}`", self.handle()),
            (None, Some(heading)) => format!(
                "{} (agent:exchange “{}”)",
                self.handle(),
                crate::response_remaining::short_row_text(heading, 60)
            ),
            (None, None) => self.handle(),
        }
    }
}

/// How sure a twin match is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Certainty {
    Strike,
    Possible,
}

/// Lowercase words of a row's prose, without its `[from:…]` tag, bracket
/// tokens (`[#q-…]`, `[operator-verify]`), projection, or a struck note.
fn prose_words(text: &str) -> HashSet<String> {
    let mut work = agent_doc_element_queue::struck_work_text(text).trim();
    if let Some(inner) = work
        .strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
    {
        work = inner.trim();
    }
    let work = match work.find(" — auto-struck:") {
        Some(at) => &work[..at],
        None => work,
    };
    let work = without_inbound_tag(work);
    let mut cleaned = String::with_capacity(work.len());
    let mut rest = work;
    while let Some(open) = rest.find('[') {
        cleaned.push_str(&rest[..open]);
        let after = &rest[open..];
        match after.find(']') {
            Some(close) if !after[..close].contains(char::is_whitespace) => {
                cleaned.push(' ');
                rest = &after[close + 1..];
            }
            _ => {
                cleaned.push('[');
                rest = &after[1..];
            }
        }
    }
    cleaned.push_str(rest);
    cleaned
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn without_inbound_tag(text: &str) -> &str {
    let trimmed = text.trim_start();
    if parse_inbound_tag(trimmed).is_some()
        && let Some(close) = trimmed.find(']')
    {
        return trimmed[close + 1..].trim_start();
    }
    trimmed
}

fn similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    shared as f64 / (a.len() + b.len() - shared) as f64
}

/// `key` without a re-queue suffix: one trailing digit right after a letter.
fn requeue_stem(key: &str) -> Option<&str> {
    let mut chars = key.chars().rev();
    let last = chars.next()?;
    let before = chars.next()?;
    (last.is_ascii_digit() && before.is_alphabetic()).then(|| &key[..key.len() - 1])
}

/// Whether two keys are one re-queue family: one is the other plus a single
/// trailing digit (`report` / `report2`), never the same key twice.
fn requeue_family(a: &str, b: &str) -> bool {
    a != b && (requeue_stem(a) == Some(b) || requeue_stem(b) == Some(a))
}

/// How the live row's tag and words match `twin`, if at all.
fn match_certainty(
    tag: Option<&InboundTag>,
    words: &HashSet<String>,
    twin: &FinishedTwin,
) -> Option<Certainty> {
    let score = similarity(words, &twin.words);
    match (tag, &twin.tag) {
        (
            Some(InboundTag {
                source,
                key: Some(key),
            }),
            Some(InboundTag {
                source: twin_source,
                key: Some(twin_key),
            }),
        ) if source == twin_source => {
            if requeue_family(key, twin_key) {
                if score >= STRIKE_SIMILARITY {
                    Some(Certainty::Strike)
                } else if score >= SURFACE_SIMILARITY {
                    Some(Certainty::Possible)
                } else {
                    None
                }
            } else {
                (score >= STRIKE_SIMILARITY).then_some(Certainty::Possible)
            }
        }
        _ => (score >= STRIKE_SIMILARITY).then_some(Certainty::Possible),
    }
}

/// One document line split into its list prefix (indent, marker, `### `) and
/// the row body after it.
fn split_row(line: &str) -> Option<(&str, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let after = &line[indent..];
    let marker = ["- ", "* ", "+ "]
        .into_iter()
        .find(|marker| after.starts_with(marker))?;
    let mut body_start = indent + marker.len();
    if let Some(rest) = line[body_start..].strip_prefix("### ") {
        body_start = line.len() - rest.len();
    }
    Some((&line[..body_start], &line[body_start..]))
}

fn line_at<'a>(lines: &[&'a str], line: usize) -> &'a str {
    lines
        .get(line.saturating_sub(1))
        .copied()
        .unwrap_or_default()
        .trim_end_matches(['\n', '\r'])
}

/// A commit SHA token in `line`: 7–40 lowercase hex characters holding a digit
/// and a letter, not part of an id (`q-…`, `r-…`) or a longer word.
fn commit_shas(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_hexdigit() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
            end += 1;
        }
        let token = &line[start..end];
        let bounded_before = start == 0 || {
            let before = bytes[start - 1];
            !(before.is_ascii_alphanumeric()
                || matches!(before, b'-' | b'_' | b'#' | b'/' | b'+' | b'.'))
        };
        let bounded_after = end == bytes.len() || {
            let after = bytes[end];
            !(after.is_ascii_alphanumeric() || matches!(after, b'-' | b'_'))
        };
        if bounded_before
            && bounded_after
            && (7..=40).contains(&token.len())
            && token.bytes().all(|b| !b.is_ascii_uppercase())
            && token.bytes().any(|b| b.is_ascii_digit())
            && token.bytes().any(|b| b.is_ascii_alphabetic())
        {
            out.push(token.to_string());
        }
        start = end;
    }
    out
}

/// The single commit the lines name, or `None` for none or several.
fn single_commit<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut found: Vec<String> = Vec::new();
    for line in lines {
        for sha in commit_shas(line) {
            if !found
                .iter()
                .any(|seen| seen.starts_with(&sha) || sha.starts_with(seen))
            {
                found.push(sha);
            }
        }
    }
    match found.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// Whether a line of a row's subtree answers it (a response tree).
fn answers_line(line: &str) -> bool {
    let item = line.trim_start();
    let item = item.strip_prefix("- ").unwrap_or(item);
    [
        "**Response [#r-",
        "**Consumed by:**",
        "**Closed by:**",
        "**State:** Complete",
    ]
    .iter()
    .any(|label| item.starts_with(label))
}

/// Prompts `agent:exchange` echoes as consumed (`> **Queue prompt:**`), with
/// the heading of the response that echoes each.
fn exchange_echoes(document: &str) -> Vec<(String, String)> {
    let Ok(components) = element::parse(document) else {
        return Vec::new();
    };
    let mut echoes = Vec::new();
    for component in components.iter().filter(|c| c.name == "exchange") {
        let mut heading = String::new();
        let mut awaiting = false;
        for line in component.content(document).lines() {
            if let Some(title) = line.strip_prefix("### ") {
                heading = title.trim().to_string();
                awaiting = false;
                continue;
            }
            let Some(quoted) = line.strip_prefix('>') else {
                awaiting = false;
                continue;
            };
            let quoted = quoted.trim();
            if let Some(rest) = quoted.strip_prefix("**Queue prompt:**") {
                let rest = rest.trim();
                if rest.is_empty() {
                    awaiting = true;
                } else {
                    echoes.push((rest.to_string(), heading.clone()));
                }
                continue;
            }
            if awaiting && !quoted.is_empty() {
                echoes.push((quoted.to_string(), heading.clone()));
                awaiting = false;
            }
        }
    }
    echoes
}

/// The single commit named by rows that follow up on `id`
/// (`[#id] follow-up: …`) and their subtrees.
fn followup_commit(lines: &[&str], nodes: &[QueueTreeNode], id: &str) -> Option<String> {
    let mut texts = Vec::new();
    for node in nodes {
        let Some((_, body)) = split_row(line_at(lines, node.line)) else {
            continue;
        };
        let work = agent_doc_element_queue::struck_work_text(body);
        let work = work.strip_prefix("~~").unwrap_or(work);
        if agent_doc_element_queue::followup_origin_reference(work)
            .is_some_and(|(origin, _)| origin.eq_ignore_ascii_case(id))
        {
            texts.extend((node.line..=node.end_line).map(|line| line_at(lines, line)));
        }
    }
    single_commit(texts)
}

/// Every finished request the document records.
fn finished_twins(document: &str, tree: &QueueTree) -> Vec<FinishedTwin> {
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    let echoes = exchange_echoes(document);
    let echoed_ids: HashMap<String, String> = echoes
        .iter()
        .filter_map(|(text, heading)| {
            agent_doc_element_queue::queue_prompt_stable_id(text).map(|id| (id, heading.clone()))
        })
        .collect();
    let mut twins: Vec<FinishedTwin> = Vec::new();
    let mut push = |text: &str, id: Option<String>, commit, heading: Option<String>| {
        let text = text.trim().to_string();
        let words = prose_words(&text);
        if words.is_empty()
            || twins
                .iter()
                .any(|twin| twin.text == text || (id.is_some() && twin.id == id))
        {
            return;
        }
        twins.push(FinishedTwin {
            tag: parse_inbound_tag(without_queue_noise(&text)),
            id,
            commit,
            exchange_heading: heading,
            words,
            text,
        });
    };
    for node in tree.nodes.iter().filter(|node| node.closed) {
        let Some((_, body)) = split_row(line_at(&lines, node.line)) else {
            continue;
        };
        let Some(inner) = body
            .trim()
            .strip_prefix("~~")
            .and_then(|rest| rest.strip_suffix("~~"))
        else {
            continue;
        };
        let inner = match inner.find(" — auto-struck:") {
            Some(at) => &inner[..at],
            None => inner,
        };
        // A row the binary itself struck as a duplicate is no evidence.
        if body.contains(DUPLICATE_STRIKE_MARKER) {
            continue;
        }
        let id = agent_doc_element_queue::queue_prompt_stable_id(inner);
        let subtree: Vec<&str> = (node.line + 1..=node.end_line)
            .map(|line| line_at(&lines, line))
            .collect();
        let heading = id.as_ref().and_then(|id| echoed_ids.get(id).cloned());
        if !subtree.iter().any(|line| answers_line(line)) && heading.is_none() {
            continue;
        }
        let commit = single_commit(subtree.iter().copied()).or_else(|| {
            id.as_deref()
                .and_then(|id| followup_commit(&lines, &tree.nodes, id))
        });
        push(inner, id, commit, heading);
    }
    for (text, heading) in &echoes {
        let id = agent_doc_element_queue::queue_prompt_stable_id(text);
        let commit = id
            .as_deref()
            .and_then(|id| followup_commit(&lines, &tree.nodes, id));
        push(text, id, commit, Some(heading.clone()));
    }
    twins
}

/// `text` without leading projection, so a `[from:…]` tag parses.
fn without_queue_noise(text: &str) -> &str {
    agent_doc_element_queue::struck_work_text(text)
}

/// Whether the node is a candidate row at all: a live top-level row of work.
fn live_top_level_row(node: &QueueTreeNode) -> bool {
    node.depth == 0
        && !node.closed
        && !node.waiting_item
        && !matches!(
            node.state,
            QueueNodeState::Group
                | QueueNodeState::Reply
                | QueueNodeState::Choice
                | QueueNodeState::FollowUp
        )
}

/// The best finished twin of the live row `body`, and how sure the match is.
fn best_twin<'a>(body: &str, twins: &'a [FinishedTwin]) -> Option<(&'a FinishedTwin, Certainty)> {
    let work = agent_doc_element_queue::struck_work_text(body);
    let tag = parse_inbound_tag(work);
    let words = prose_words(work);
    let own_id = agent_doc_element_queue::queue_prompt_stable_id(work);
    let mut best: Option<(&FinishedTwin, Certainty)> = None;
    for twin in twins {
        if own_id.is_some() && twin.id == own_id {
            continue;
        }
        let Some(certainty) = match_certainty(tag.as_ref(), &words, twin) else {
            continue;
        };
        match (best, certainty) {
            (None, _) | (Some((_, Certainty::Possible)), Certainty::Strike) => {
                best = Some((twin, certainty));
            }
            (Some((first, Certainty::Strike)), Certainty::Strike) if first.id != twin.id => {
                // Two different finished twins: not certain which one.
                best = Some((first, Certainty::Possible));
            }
            _ => {}
        }
    }
    best
}

/// Whether the row on `line` is in `baseline` at the same list position: a
/// baseline row with the same prefix and the same work text (projection
/// markers painted since then do not count as an edit).
fn settled(baseline: Option<&str>, line: &str) -> bool {
    let Some((prefix, body)) = split_row(line) else {
        return false;
    };
    let work = agent_doc_element_queue::struck_work_text(body);
    baseline.is_some_and(|baseline| {
        baseline.lines().any(|candidate| {
            split_row(candidate.trim_end()).is_some_and(|(candidate_prefix, candidate_body)| {
                candidate_prefix == prefix
                    && agent_doc_element_queue::struck_work_text(candidate_body) == work
            })
        })
    })
}

/// Whether a request id the row carries names an open `agent:todo` item. The
/// bare `🙋` glyph alone is projection, not a hold: it is recomputed from the
/// row's current classification on every pass.
fn linked_todo_open(body: &str, open_todo_ids: &HashSet<String>) -> bool {
    agent_doc_document::queue_projection::operator_action_ids(body)
        .iter()
        .any(|id| open_todo_ids.contains(id))
}

/// Guards that keep a live row from being struck as finished.
fn strike_guards_pass(
    lines: &[&str],
    node: &QueueTreeNode,
    body: &str,
    baseline: Option<&str>,
    in_flight: &dyn Fn(&str) -> bool,
    open_todo_ids: &HashSet<String>,
) -> bool {
    use agent_doc_document::queue_projection as qp;
    let work = agent_doc_element_queue::struck_work_text(body);
    let action = followup_row_action_item(work).unwrap_or(without_inbound_tag(work));
    node.end_line == node.line
        && settled(baseline, line_at(lines, node.line))
        && !in_flight(work)
        && qp::owner_projection(body).is_none()
        && !qp::lifecycle_marker(body).is_some_and(crate::document_queue::durable_backed_marker)
        && !linked_todo_open(body, open_todo_ids)
        && !crate::struck_prompt_response::is_operator_reply_shape(work)
        && !crate::struck_prompt_response::is_question(work)
        && !matches!(explicit_owner_label(action), Some(ActionItemOwner::Human))
        && !crate::queue_continuation::is_recurring_imperative_head(work)
}

/// One planned rewrite: replace document line `line` with `text`.
struct Rewrite {
    line: usize,
    text: String,
}

/// Lines to insert after document line `after`.
struct Insert {
    after: usize,
    lines: Vec<String>,
}

fn apply(document: &str, rewrites: Vec<Rewrite>, inserts: Vec<Insert>) -> Option<String> {
    if rewrites.is_empty() && inserts.is_empty() {
        return None;
    }
    let rewrites: HashMap<usize, String> = rewrites.into_iter().map(|r| (r.line, r.text)).collect();
    let mut inserts_at: HashMap<usize, Vec<String>> = HashMap::new();
    for insert in inserts {
        inserts_at
            .entry(insert.after)
            .or_default()
            .extend(insert.lines);
    }
    let mut out = String::with_capacity(document.len() + 512);
    for (index, line) in document.split_inclusive('\n').enumerate() {
        let number = index + 1;
        let ending_len = line.len() - line.trim_end_matches(['\n', '\r']).len();
        let ending = &line[line.len() - ending_len..];
        match rewrites.get(&number) {
            Some(text) => {
                out.push_str(text);
                out.push_str(if ending.is_empty() { "\n" } else { ending });
            }
            None => {
                out.push_str(line);
                if ending.is_empty() && inserts_at.contains_key(&number) {
                    out.push('\n');
                }
            }
        }
        if let Some(lines) = inserts_at.get(&number) {
            for inserted in lines {
                out.push_str(inserted);
                out.push('\n');
            }
        }
    }
    (out != document).then_some(out)
}

/// The struck rendering of the row on `line`: list prefix, then
/// `~~<work> — <note>~~`.
fn struck_line(line: &str, note: &str) -> Option<String> {
    let (prefix, body) = split_row(line)?;
    let work = agent_doc_element_queue::struck_work_text(body).trim_end();
    if work.starts_with("~~") {
        return None;
    }
    Some(match note.is_empty() {
        true => format!("{prefix}~~{work}~~"),
        false => format!("{prefix}~~{work} — {note}~~"),
    })
}

fn row_indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// `#dupdone`: remove each `🙋` glyph the operator-action projection no
/// longer gives its row: one an earlier misclassification painted (a tag
/// quoted inside inline code read as the row's own). Only removals are kept;
/// every other projection change is left to preflight, so `queue sync` stays
/// a narrow repair. `None` when no glyph is stale.
pub fn clear_stale_operator_glyphs(document: &str) -> Result<Option<String>> {
    let marker = agent_doc_document::queue_projection::operator_action_marker();
    if !document.contains(marker) {
        return Ok(None);
    }
    let open_todo_ids = agent_doc_element::todo::open_todo_ids(document);
    let Some(projected) =
        crate::document_queue::project_all_prompt_operator_actions(document, &[], &open_todo_ids)?
    else {
        return Ok(None);
    };
    let before: Vec<&str> = document.split_inclusive('\n').collect();
    let after: Vec<&str> = projected.split_inclusive('\n').collect();
    if before.len() != after.len() {
        return Ok(None);
    }
    let with_space = format!("{marker} ");
    let mut out = String::with_capacity(document.len());
    let mut changed = false;
    for (old, new) in before.iter().zip(&after) {
        if old != new && old.contains(&with_space) && old.replacen(&with_space, "", 1) == *new {
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(old);
        }
    }
    Ok(changed.then_some(out))
}

/// Resolve every live row whose work is finished (see the module docs):
/// operator completion replies, then duplicates of finished work. `baseline`
/// is the committed document; a row absent from it is still being typed and
/// is never struck. `in_flight` names work text a claim attempt holds.
/// `None` when nothing changes (a second pass is a no-op).
pub fn reconcile_finished_rows(
    document: &str,
    baseline: Option<&str>,
    in_flight: &dyn Fn(&str) -> bool,
) -> Result<Option<String>> {
    if !document.contains("agent:queue") && !document.contains("agent:run") {
        return Ok(None);
    }
    let tree = queue_tree(document, &QueueTreeOptions::default())?;
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    let open_todo_ids = agent_doc_element::todo::open_todo_ids(document);
    let mut rewrites = Vec::new();
    let mut inserts = Vec::new();
    let mut touched: HashSet<usize> = HashSet::new();

    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some((reply_line, reply)) = operator_done_reply(
            &lines,
            &tree.nodes,
            index,
            baseline,
            in_flight,
            &open_todo_ids,
        ) && let Some((parent, reply_rewrite, response)) =
            resolve_done_reply(&lines, node, reply_line, &reply, None)
        {
            touched.insert(node.line);
            rewrites.push(parent);
            rewrites.push(reply_rewrite);
            inserts.push(response);
        }
    }

    let twins = finished_twins(document, &tree);
    if !twins.is_empty() {
        for node in tree.nodes.iter().filter(|node| live_top_level_row(node)) {
            if touched.contains(&node.line) {
                continue;
            }
            let line = line_at(&lines, node.line);
            let Some((_, body)) = split_row(line) else {
                continue;
            };
            let Some((twin, Certainty::Strike)) = best_twin(body, &twins) else {
                continue;
            };
            if !strike_guards_pass(&lines, node, body, baseline, in_flight, &open_todo_ids) {
                continue;
            }
            let done_by = twin.done_by();
            let note = match &twin.id {
                Some(id) => format!("auto-struck: done by #{id} {DUPLICATE_STRIKE_MARKER}"),
                None => format!(
                    "auto-struck: done by {} {DUPLICATE_STRIKE_MARKER}",
                    twin.handle().trim_matches('`')
                ),
            };
            let Some(struck) = struck_line(line, &note) else {
                continue;
            };
            let summary = format!(
                "Duplicate of finished work: done by {done_by}, which completed this same request. Un-strike the row if it still needs doing."
            );
            let response = crate::struck_prompt_response::abbreviated_strike_response_lines(
                split_row(&struck).map_or(&struck[..], |(_, body)| body),
                row_indent(line),
                &summary,
            );
            touched.insert(node.line);
            rewrites.push(Rewrite {
                line: node.line,
                text: struck,
            });
            inserts.push(Insert {
                after: node.end_line,
                lines: response,
            });
        }
    }
    Ok(apply(document, rewrites, inserts))
}

/// The strike of the operator-verify row `node` and its completion reply on
/// `reply_line`, and the response tree that quotes the reply. `summary`
/// replaces the default response summary.
fn resolve_done_reply(
    lines: &[&str],
    node: &QueueTreeNode,
    reply_line: usize,
    reply: &str,
    summary: Option<&str>,
) -> Option<(Rewrite, Rewrite, Insert)> {
    let parent = line_at(lines, node.line);
    let reply_quote = completion_reply_quote(reply);
    let note = format!("operator answered “{reply_quote}” {OPERATOR_DONE_MARKER}");
    let struck_parent = struck_line(parent, &note)?;
    let reply_raw = line_at(lines, reply_line);
    // The operator may have struck the reply already (`~~done~~`). Keep that
    // line; striking it again would fail the whole resolution.
    let struck_reply = match struck_line(reply_raw, "") {
        Some(struck) => struck,
        None => {
            let (_, body) = split_row(reply_raw)?;
            if !body.trim_start().starts_with("~~") || !is_completion_reply(body) {
                return None;
            }
            reply_raw.trim_end().to_string()
        }
    };
    let summary = match summary.map(str::trim).filter(|summary| !summary.is_empty()) {
        Some(summary) => format!("The operator answered “{reply_quote}” under this row. {summary}"),
        None => format!(
            "The operator answered “{reply_quote}” under this row: the operator action is done. Un-strike the row if it still needs doing."
        ),
    };
    let response = crate::struck_prompt_response::abbreviated_strike_response_lines(
        split_row(&struck_parent).map_or(&struck_parent[..], |(_, body)| body),
        row_indent(parent),
        &summary,
    );
    Some((
        Rewrite {
            line: node.line,
            text: struck_parent,
        },
        Rewrite {
            line: reply_line,
            text: struck_reply,
        },
        Insert {
            after: node.end_line,
            lines: response,
        },
    ))
}

/// What an explicit `queue consume --ack-text '<reply>'` resolves when the
/// text is an operator completion reply under an operator-verify row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionReplyAck {
    /// No operator-verify row carries that reply.
    None,
    /// Exactly one does: the resolved document and the parent row's work text.
    Resolved { document: String, parent: String },
    /// Several do; the parents' work texts. Nothing is resolved.
    Ambiguous(Vec<String>),
}

/// `#dupdone`: resolve the one operator-verify row whose completion reply
/// reads `reply` (`--ack-text 'done'`). The explicit acknowledgement is the
/// settling proof, so the committed-baseline gate does not apply; every other
/// guard of [`reconcile_finished_rows`] does.
pub fn resolve_completion_reply_ack(
    document: &str,
    reply: &str,
    in_flight: &dyn Fn(&str) -> bool,
    summary: Option<&str>,
) -> Result<CompletionReplyAck> {
    if !is_completion_reply(reply) {
        return Ok(CompletionReplyAck::None);
    }
    let tree = queue_tree(document, &QueueTreeOptions::default())?;
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    let open_todo_ids = agent_doc_element::todo::open_todo_ids(document);
    let wanted = reply.trim().trim_start_matches("- ").trim();
    let mut found = Vec::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some((reply_line, text)) = operator_done_reply(
            &lines,
            &tree.nodes,
            index,
            Some(document),
            in_flight,
            &open_todo_ids,
        ) && text.eq_ignore_ascii_case(wanted)
        {
            found.push((index, node, reply_line, text));
        }
    }
    let parent_text = |node: &QueueTreeNode| {
        split_row(line_at(&lines, node.line)).map_or_else(String::new, |(_, body)| {
            agent_doc_element_queue::struck_work_text(body).to_string()
        })
    };
    match found.as_slice() {
        [] => Ok(CompletionReplyAck::None),
        [(_, node, reply_line, text)] => {
            let Some((parent, reply_rewrite, response)) =
                resolve_done_reply(&lines, node, *reply_line, text, summary)
            else {
                return Ok(CompletionReplyAck::None);
            };
            match apply(document, vec![parent, reply_rewrite], vec![response]) {
                Some(resolved) => Ok(CompletionReplyAck::Resolved {
                    document: resolved,
                    parent: parent_text(node),
                }),
                None => Ok(CompletionReplyAck::None),
            }
        }
        many => Ok(CompletionReplyAck::Ambiguous(
            many.iter()
                .map(|(_, node, _, _)| parent_text(node))
                .collect(),
        )),
    }
}

/// The operator's completion reply directly under the operator-verify row at
/// `index`: its line and text, when the row may be resolved by it.
fn operator_done_reply(
    lines: &[&str],
    nodes: &[QueueTreeNode],
    index: usize,
    baseline: Option<&str>,
    in_flight: &dyn Fn(&str) -> bool,
    open_todo_ids: &HashSet<String>,
) -> Option<(usize, String)> {
    use agent_doc_document::queue_projection as qp;
    let node = &nodes[index];
    if node.closed || node.waiting_item {
        return None;
    }
    let (_, body) = split_row(line_at(lines, node.line))?;
    let work = agent_doc_element_queue::struck_work_text(body);
    if !crate::queue_continuation::head_awaits_operator_inline(work)
        || in_flight(work)
        || qp::owner_projection(body).is_some()
        || qp::lifecycle_marker(body).is_some_and(crate::document_queue::durable_backed_marker)
        || linked_todo_open(body, open_todo_ids)
    {
        return None;
    }
    // A multi-line row's continuation is not a child row.
    if node.end_line > node.line
        && !nodes
            .get(index + 1)
            .is_some_and(|child| child.line == node.line + 1 && child.depth > node.depth)
    {
        return None;
    }
    let mut reply: Option<(usize, String)> = None;
    for child in nodes[index + 1..]
        .iter()
        .take_while(|child| child.queue == node.queue && child.line <= node.end_line)
    {
        if child.depth <= node.depth {
            break;
        }
        let raw = line_at(lines, child.line);
        // A response recorded under the reply is not another row of work.
        if child.depth > node.depth + 1 {
            if line_is_response_record(raw) {
                continue;
            }
            return None;
        }
        let (_, child_body) = split_row(raw)?;
        if child.closed && !is_completion_reply(child_body) {
            continue;
        }
        let span_is_reply_record = (child.line..=child.end_line)
            .all(|line| line == child.line || line_is_response_record(line_at(lines, line)));
        if !span_is_reply_record
            || reply.is_some()
            || !is_completion_reply(child_body)
            || !settled(baseline, raw)
            || qp::owner_projection(child_body).is_some()
            || qp::lifecycle_marker(child_body)
                .is_some_and(crate::document_queue::durable_backed_marker)
        {
            return None;
        }
        reply = Some((child.line, completion_reply_quote(child_body)));
    }
    reply
}

/// The live rows (by queue tree node index) that may duplicate finished
/// work, each with the `possibly done by …` text the Loose ends digest shows.
/// Preflight and `queue sync` strike the certain duplicates first, so what
/// remains here is a looser match or a duplicate a strike guard kept live.
pub fn possible_finished_twins(document: &str, tree: &QueueTree) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    if !tree.nodes.iter().any(|node| node.closed) && !document.contains("**Queue prompt:**") {
        return out;
    }
    let twins = finished_twins(document, tree);
    if twins.is_empty() {
        return out;
    }
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    for (index, node) in tree.nodes.iter().enumerate() {
        if !live_top_level_row(node) {
            continue;
        }
        let Some((_, body)) = split_row(line_at(&lines, node.line)) else {
            continue;
        };
        if let Some((twin, _)) = best_twin(body, &twins) {
            out.insert(index, format!("possibly done by {}", twin.done_by()));
        }
    }
    out
}

#[cfg(test)]
#[path = "finished_duplicate_tests.rs"]
mod tests;
