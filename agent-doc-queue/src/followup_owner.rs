//! `#humanfollowupowner`: who owns a worker hand-back's action item.
//!
//! `queue report-complete` turns each **Action items** bullet into a follow-up
//! row (`follow up on an action item from …: <item>`,
//! [`crate::handback_followups`]). A row with no owner reads as agent work: it
//! paints `📥`, a `go` queue dispatches it with no review, and the Stop hook's
//! `#transientcloseout` gate refuses every turn end while it is pending. Many
//! action items are not agent work at all: another reviewer approving a pull
//! request, an org owner or repo admin changing a setting, an operator
//! decision. No agent can close those rows, so the session looped forever
//! (haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-13: 39 of 41 pending rows).
//!
//! # Owner
//!
//! A worker labels each bullet with its owner (the subagent brief asks for
//! it): `agent: …` is agent work, `operator: …`, `human: …`, and
//! `human (<who>): …` are not. A label may be bold (`**human (tmstorm):**`).
//! The label stays in the row text, so the owner the worker wrote is the one
//! every later reader sees.
//!
//! An unlabelled bullet (every hand-back written before the label existed) is
//! classified by [`human_owner_signal`]. The classifier is deliberately
//! conservative in one direction: any phrase that names an action only a
//! person can take makes the bullet human-owned. Misreading agent work as
//! human parks one row where the operator sees it and can hand it back;
//! misreading human work as agent work is the loop this module exists to end.
//!
//! # Representation
//!
//! A human-owned row carries the existing `[operator-verify]` execution tag
//! after its work text (and before the binary's trailing `[#q-…]` id). That
//! tag already means "no agent drain scope runs this": free-text heads parse
//! it inline (`#ftimmediate`), so the row is never drainable or dispatchable,
//! preflight projects `⏳` awaiting operator onto it instead of `📥`
//! (`#queueawaitop`), and the `#transientcloseout` gate treats a pending row
//! that carries it as waiting on the operator (`#stopquiescentrows`). The
//! operator strikes the row once the action is done, or answers it inline
//! (`…: done`, `#opverifyanswered`) to hand the remaining work back to an
//! agent. The tag lives in the work text, so it is durable; the `⏳`/`📥`
//! glyph is only projection.
//!
//! # Existing documents
//!
//! [`reclassify_human_followup_rows`] tags the live, unstarted
//! `follow up on an action item` rows whose item is human-owned, in place and
//! idempotently. Preflight queue maintenance runs it on every cycle, and
//! `agent-doc queue reclassify-followups <FILE>` runs it on demand.

use anyhow::Result;

use agent_doc_document::queue_projection::{self, QueueLifecycleMarker};
use agent_doc_element_backlog::backlog;

/// The work-text prefix of every row `report-complete` writes for an action
/// item ([`crate::handback_followups::action_item_followup_text`]).
pub const FOLLOWUP_ROW_PREFIX: &str = "follow up on an action item from ";

/// The execution tag a human-owned follow-up row carries.
pub const OPERATOR_OWNED_TAG: &str = backlog::OPERATOR_VERIFY_TAG;

/// Who owns an action item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionItemOwner {
    /// An agent can do it; the follow-up row stays dispatchable (`📥`).
    Agent,
    /// Only a person can do it; the row waits on the operator (`⏳`).
    Human,
}

/// Why an action item was classified as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerEvidence {
    /// The worker labelled the bullet (`agent:`, `operator:`, `human (x):`).
    Label,
    /// An unlabelled bullet named an action only a person can take.
    Signal(&'static str),
    /// An unlabelled bullet with no such phrase.
    Default,
}

/// The owner a worker wrote at the start of an action item, if any:
/// `agent:` → [`ActionItemOwner::Agent`]; `operator:`, `human:`, and
/// `human (<who>):` → [`ActionItemOwner::Human`]. Case-insensitive; the label
/// may be bold or italic (`**human (tmstorm):**`, `**Agent**:`).
pub fn explicit_owner_label(item: &str) -> Option<ActionItemOwner> {
    let emphasis = |c: char| c == '*' || c == '_';
    let rest = item.trim_start().trim_start_matches(emphasis);
    let word_end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let owner = match rest[..word_end].to_ascii_lowercase().as_str() {
        "agent" => ActionItemOwner::Agent,
        "operator" | "human" => ActionItemOwner::Human,
        _ => return None,
    };
    let mut tail = rest[word_end..].trim_start_matches(emphasis).trim_start();
    if let Some(inner) = tail.strip_prefix('(') {
        let close = inner.find(')')?;
        tail = inner[close + 1..].trim_start_matches(emphasis).trim_start();
    }
    tail.starts_with(':').then_some(owner)
}

/// Whole words (lowercase) whose presence names an action only a person can
/// take. Matched against the item with inline code masked, so a path such as
/// `approve.yml` never matches.
const HUMAN_WORDS: &[&str] = &[
    // Another person's review decision: an author can never approve their
    // own pull request, and a re-review is the reviewer's act.
    "re-review",
    "re-reviews",
    "rereview",
    "non-author",
    "maintainer",
    "maintainers",
    // Settings only an owner or admin can change.
    "admin",
    "admins",
    "administrator",
    // An operator decision or confirmation.
    "decide",
    "decides",
    "confirm",
    "confirms",
    // Authority the agent does not hold.
    "authorize",
    "authorized",
    "authorization",
    "sso",
];

/// Word prefixes with the same meaning as [`HUMAN_WORDS`]:
/// `approve`, `approves`, `approved`, `approval`, `approving`.
const HUMAN_WORD_PREFIXES: &[&str] = &["approv"];

/// Phrases (lowercase, single-spaced words) with the same meaning.
const HUMAN_PHRASES: &[&str] = &[
    "someone other than",
    "someone else",
    "a reviewer",
    "another reviewer",
    "org owner",
    "organization owner",
    "repo owner",
    "repository owner",
    "code owner",
    "ask the operator",
    "sign in",
    "signed in",
    "log in",
    "logged in",
];

/// The first phrase in an unlabelled action item that names an action only a
/// person can take, or `None` when it names none (`#humanfollowupowner`).
///
/// The vocabulary covers the shapes worker hand-backs use for human work:
/// approving or re-reviewing a pull request (`approve`, `approval`,
/// `re-review`, `non-author`, `someone other than the author`, `a reviewer`,
/// `maintainer`), a setting only an owner or admin can change (`org owner`,
/// `repo admin`, `admin`), an operator decision (`decide`, `confirm`, `ask
/// the operator`), and authority the agent does not hold (`not authorized`,
/// `sign in`, `SSO`). Plain agent verbs (`merge`, `review`, `rebase`, `fix`,
/// `address`) are not signals: "address the review" and "rebase after #240
/// merges" are agent work.
pub fn human_owner_signal(item: &str) -> Option<&'static str> {
    let masked = mask_code_spans(item).to_ascii_lowercase();
    let words: Vec<&str> = masked
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .map(|word| word.trim_matches('-'))
        .filter(|word| !word.is_empty())
        .collect();
    for word in &words {
        if let Some(signal) = HUMAN_WORDS.iter().find(|signal| *signal == word) {
            return Some(signal);
        }
        if let Some(signal) = HUMAN_WORD_PREFIXES
            .iter()
            .find(|prefix| word.starts_with(*prefix))
        {
            return Some(signal);
        }
    }
    let joined = format!(" {} ", words.join(" "));
    HUMAN_PHRASES
        .iter()
        .find(|phrase| joined.contains(&format!(" {phrase} ")))
        .copied()
}

/// The owner of an action item and why: the worker's label when present,
/// otherwise [`human_owner_signal`], otherwise agent.
pub fn classify_action_item(item: &str) -> (ActionItemOwner, OwnerEvidence) {
    if let Some(owner) = explicit_owner_label(item) {
        return (owner, OwnerEvidence::Label);
    }
    match human_owner_signal(item) {
        Some(signal) => (ActionItemOwner::Human, OwnerEvidence::Signal(signal)),
        None => (ActionItemOwner::Agent, OwnerEvidence::Default),
    }
}

/// [`classify_action_item`]'s owner alone.
pub fn action_item_owner(item: &str) -> ActionItemOwner {
    classify_action_item(item).0
}

/// Whether `text` already carries [`OPERATOR_OWNED_TAG`].
pub fn carries_operator_owned_tag(text: &str) -> bool {
    backlog::item_operator_verify_required(text)
}

/// `row` with [`OPERATOR_OWNED_TAG`] after its work text, before the
/// binary's trailing `[#q-…]` id annotation when it has one. A row that
/// already carries the tag is returned unchanged.
pub fn tag_operator_owned(row: &str) -> String {
    if carries_operator_owned_tag(row) {
        return row.to_string();
    }
    let base = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(row);
    if base == row {
        return format!("{} {OPERATOR_OWNED_TAG}", row.trim_end());
    }
    let mut split = row
        .char_indices()
        .zip(base.chars())
        .find(|((_, a), b)| a != b)
        .map_or(base.len().min(row.len()), |((index, _), _)| index);
    while !row.is_char_boundary(split) {
        split -= 1;
    }
    let (work, annotation) = row.split_at(split);
    format!(
        "{} {OPERATOR_OWNED_TAG} {}",
        work.trim_end(),
        annotation.trim_start()
    )
}

/// The follow-up row for `item`, tagged when the item is human-owned.
pub fn owned_followup_row(row: String, item: &str) -> String {
    match action_item_owner(item) {
        ActionItemOwner::Human => tag_operator_owned(&row),
        ActionItemOwner::Agent => row,
    }
}

/// The action item a `follow up on an action item from <origin>: <item>` work
/// text quotes. `<origin>` is `` `id` `` or `queue item “label”`.
pub fn followup_row_action_item(work: &str) -> Option<&str> {
    let rest = work.trim_start().strip_prefix(FOLLOWUP_ROW_PREFIX)?;
    let after_origin = if let Some(quoted) = rest.strip_prefix('`') {
        let close = quoted.find('`')?;
        &quoted[close + 1..]
    } else {
        let quoted = rest.strip_prefix("queue item “")?;
        let close = quoted.find('”')?;
        &quoted[close + '”'.len_utf8()..]
    };
    after_origin.strip_prefix(": ")
}

/// One follow-up row [`reclassify_human_followup_rows`] tagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclassifiedFollowup {
    /// The row text before, without its list bullet.
    pub before: String,
    /// The row text after.
    pub after: String,
    /// Why the item is human-owned.
    pub evidence: OwnerEvidence,
}

/// The reclassified text of one live queue row (its text after the list
/// bullet), or `None` when it is not an unstarted human-owned follow-up row
/// missing its tag.
///
/// Only a row whose lifecycle is bare or `📥` qualifies: a claimed, running,
/// reported, or lost row has an attempt keyed by its text, and a `🙋` row is
/// already operator-owned. A `📥` row becomes `⏳`; preflight repaints the
/// same glyph from the tag.
pub fn reclassify_followup_row(text: &str) -> Option<(String, OwnerEvidence)> {
    if text.contains('\n') || text.trim_start().starts_with("~~") {
        return None;
    }
    let (heading, rest) = match text.strip_prefix("### ") {
        Some(rest) => ("### ", rest),
        None => ("", text),
    };
    let marker = queue_projection::lifecycle_marker(rest);
    if !matches!(marker, None | Some(QueueLifecycleMarker::Pending))
        || queue_projection::operator_action_marked(rest)
        || carries_operator_owned_tag(rest)
    {
        return None;
    }
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(rest);
    let item = followup_row_action_item(work)?;
    let item = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item);
    let (owner, evidence) = classify_action_item(&item);
    if owner != ActionItemOwner::Human {
        return None;
    }
    let mut tagged = tag_operator_owned(rest);
    if marker == Some(QueueLifecycleMarker::Pending) {
        tagged = queue_projection::apply_awaiting_operator_marker(&tagged);
    }
    Some((format!("{heading}{tagged}"), evidence))
}

/// Tag every live, unstarted human-owned `follow up on an action item` row in
/// every queue block (`#humanfollowupowner`). Lines are rewritten in place;
/// every other byte is kept. Idempotent: a tagged row is never touched again,
/// so a second run returns `None`.
pub fn reclassify_human_followup_rows(
    content: &str,
) -> Result<Option<(String, Vec<ReclassifiedFollowup>)>> {
    let components = agent_doc_element::element::parse(content)?;
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut rows = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body_start = component.open_end;
        let body = component.content(content);
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            let line_start = body_start + offset;
            offset += line.len();
            let content_line = line.trim_end_matches(['\n', '\r']);
            let indent = content_line.len() - content_line.trim_start().len();
            let Some(text) = content_line[indent..].strip_prefix("- ") else {
                continue;
            };
            if !text.contains(FOLLOWUP_ROW_PREFIX) {
                continue;
            }
            let Some((after, evidence)) = reclassify_followup_row(text) else {
                continue;
            };
            let text_start = line_start + indent + 2;
            edits.push((text_start, text_start + text.len(), after.clone()));
            rows.push(ReclassifiedFollowup {
                before: text.to_string(),
                after,
                evidence,
            });
        }
    }
    if edits.is_empty() {
        return Ok(None);
    }
    let mut updated = content.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        updated.replace_range(start..end, &replacement);
    }
    Ok(Some((updated, rows)))
}

/// `text` with the contents of inline code spans replaced by spaces.
fn mask_code_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for ch in text.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
        } else if in_code {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests;
