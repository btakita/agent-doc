//! Archive a closed Waiting-on-you item's subtree (`#waitingarchive`).
//!
//! The Waiting-on-you tree is regenerated from the open `agent:todo` items on
//! every closeout. A child whose Todo closed used to drop out of that
//! regeneration, and its whole subtree went with it: operator answers, struck
//! rows, attempts and offered Choices were deleted without a trace.
//!
//! Instead, every child segment the regenerated tree does not keep moves,
//! byte for byte, into `agent:done` under a one-line receipt. The receipt names
//! the item and where the packet came from; the packet lines follow it as a
//! quoted continuation (`  > ` + the original line), so:
//!
//! - no operator byte is lost (strip one `  > ` prefix per line to recover the
//!   original subtree exactly);
//! - no quoted row is a list item, so done-id scanners
//!   (`extract_done_item_own_ids`, `collect_done_item_own_ids`) never read a
//!   packet's `[#id]` references as completed work;
//! - the receipt itself spells ids as code spans, never `[#id]`, so archiving
//!   does not resolve any tracked id by itself.
//!
//! When the done component declares `archive=`, the next done-archive flush
//! moves the inline entry into that file with the rest of the inline history.
//!
//! This module is pure.

use std::collections::HashSet;

use agent_doc_element::element;

/// Hidden sentinel on a receipt line, so tooling can find archived packets.
pub const WAITING_ARCHIVE_MARKER: &str = "<!-- agent-doc:waiting-archive -->";

/// The prefix that quotes each archived line beneath its receipt.
pub const WAITING_ARCHIVE_QUOTE: &str = "  > ";

/// One child segment of the Waiting-on-you root: a row at the child indent
/// and every line beneath it up to the next such row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingSegment {
    /// The segment's exact source text, including its trailing newline.
    pub raw: String,
    /// The head row's own id (lowercased), when it has one.
    pub id: Option<String>,
    /// Whether the head row is struck.
    pub struck: bool,
}

fn list_indent(line: &str) -> Option<usize> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
        .then(|| line.len() - trimmed.len())
}

fn head_work(line: &str) -> &str {
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed);
    agent_doc_element_queue::struck_work_text(body)
}

fn segment_head_id(line: &str) -> Option<String> {
    let work = crate::waiting_status::without_status_token(head_work(line));
    let work = work.trim().trim_start_matches("~~");
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(work);
    crate::document_queue::queue_prompt_reference_ids(work)
        .into_iter()
        .next()
        .map(|id| id.to_ascii_lowercase())
}

/// Split the lines below the root row of a Waiting-on-you tree into child
/// segments. `tree` is the root line plus its descendants; `root_indent` is
/// the root row's list indent.
pub fn waiting_tree_segments(tree: &str, root_indent: usize) -> Vec<WaitingSegment> {
    let child_indent = root_indent + 2;
    let mut segments: Vec<WaitingSegment> = Vec::new();
    for line in tree.split_inclusive('\n').skip(1) {
        let starts_segment =
            segments.is_empty() || list_indent(line).is_some_and(|indent| indent <= child_indent);
        if starts_segment {
            let head = line.trim_end_matches(['\n', '\r']);
            let is_row = list_indent(head).is_some();
            segments.push(WaitingSegment {
                raw: String::new(),
                id: is_row.then(|| segment_head_id(head)).flatten(),
                struck: is_row && head_work(head).trim_start().starts_with("~~"),
            });
        }
        segments
            .last_mut()
            .expect("a segment was just started")
            .raw
            .push_str(line);
    }
    segments
        .into_iter()
        .filter(|segment| !segment.raw.trim().is_empty())
        .collect()
}

/// Whether the segment holds a list row the binary did not author.
pub fn segment_has_operator_rows(raw: &str) -> bool {
    crate::queue_consume::waiting_child_has_operator_rows(raw)
}

/// Spell every `[#id]` reference as a code span so the receipt line names
/// ids without making any of them a done entry's own id.
fn code_span_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[#") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        match after.find(']') {
            Some(end) if !after[..end].contains(char::is_whitespace) => {
                out.push_str(&format!("`#{}`", &after[..end]));
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("[#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn receipt_text(segment: &WaitingSegment) -> String {
    let head = segment.raw.lines().next().unwrap_or_default();
    let work = crate::waiting_status::without_status_token(head_work(head));
    let mut text = work
        .replace(crate::queue_consume::WAITING_ON_YOU_ITEM_MARKER, "")
        .replace("~~", "");
    let mut rest = text.as_str();
    let mut stripped = String::with_capacity(rest.len());
    while let Some(at) = rest.find("⟦effect:") {
        stripped.push_str(&rest[..at]);
        match rest[at..].find('⟧') {
            Some(end) => rest = &rest[at + end + '⟧'.len_utf8()..],
            None => {
                stripped.push_str(&rest[at..]);
                rest = "";
            }
        }
    }
    stripped.push_str(rest);
    text = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = code_span_ids(text.trim());
    const LIMIT: usize = 160;
    if text.chars().count() <= LIMIT {
        return text;
    }
    let mut cut = text.chars().take(LIMIT).collect::<String>();
    if let Some(space) = cut.rfind(' ') {
        cut.truncate(space);
    }
    format!("{}…", cut.trim_end())
}

/// Why a segment left the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveReason {
    /// The item's Todo is closed (or gone).
    TodoClosed,
    /// Every row under the item closed, so the response closeout checked its
    /// Todo off (`#waitingdonetodoopen`).
    TreeComplete,
}

impl ArchiveReason {
    fn phrase(self) -> &'static str {
        match self {
            Self::TodoClosed => "its Todo closed",
            Self::TreeComplete => "every row under it closed, so closeout checked its Todo off",
        }
    }
}

/// The receipt line plus the quoted packet, as one done entry.
pub fn render_archive_entry(
    date: &str,
    queue: &str,
    segment: &WaitingSegment,
    reason: ArchiveReason,
) -> String {
    let rows = segment
        .raw
        .lines()
        .filter(|line| list_indent(line).is_some())
        .count();
    let noun = if rows == 1 { "row" } else { "rows" };
    let subject = match &segment.id {
        Some(id) => format!("waiting item `#{id}`"),
        None => "a Waiting-on-you row".to_string(),
    };
    let mut entry = format!(
        "- {date} archived {subject} ({}): {} — its {rows} {noun} moved here from the Waiting on you tree of queue `{queue}` {WAITING_ARCHIVE_MARKER}\n",
        reason.phrase(),
        receipt_text(segment),
    );
    for line in segment.raw.split_inclusive('\n') {
        let line = line.trim_end_matches(['\n', '\r']);
        entry.push_str(WAITING_ARCHIVE_QUOTE.trim_end());
        if !line.is_empty() {
            entry.push(' ');
            entry.push_str(line);
        }
        entry.push('\n');
    }
    entry
}

/// The original segment text recovered from an archive entry's quoted lines.
pub fn unquote_archive_entry(entry: &str) -> String {
    let mut out = String::new();
    for line in entry.lines().skip(1) {
        let quoted = WAITING_ARCHIVE_QUOTE.trim_end();
        let Some(rest) = line.strip_prefix(quoted) else {
            continue;
        };
        out.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        out.push('\n');
    }
    out
}

/// Append archive entries to the document's `agent:done` component, creating
/// the canonical Done section when the document has none. Returns `None` when
/// the document has no place for one (no tracked-work component), so the
/// caller keeps the segments where they are instead of losing them.
pub fn append_to_done(document: &str, entries: &[String]) -> anyhow::Result<Option<String>> {
    if entries.is_empty() {
        return Ok(Some(document.to_string()));
    }
    let mut content = document.to_string();
    let has_done = element::parse(&content)?
        .iter()
        .any(|component| element::is_backlog_done_component(&component.name));
    if !has_done {
        match insert_done_component(&content)? {
            Some(inserted) => content = inserted,
            None => return Ok(None),
        }
    }
    let done = element::parse(&content)?
        .into_iter()
        .find(|component| element::is_backlog_done_component(&component.name))
        .ok_or_else(|| anyhow::anyhow!("agent:done component missing after insertion"))?;
    let existing = done.content(&content);
    let existing_lines = existing.lines().collect::<HashSet<_>>();
    let mut body = existing.to_string();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    for entry in entries {
        // A replay of an older document must not archive the same packet twice.
        let receipt = entry.lines().next().unwrap_or_default();
        if existing_lines.contains(receipt) {
            continue;
        }
        body.push_str(entry);
    }
    Ok(Some(done.replace_content(&content, &body)))
}

/// Insert the canonical empty Done section at its queue-centric slot. Unlike
/// the backlog reaper's insertion, a queue or Todo is anchor enough: the
/// archived packet came from one. Existing components are never moved.
fn insert_done_component(content: &str) -> anyhow::Result<Option<String>> {
    let components = element::parse(content)?;
    if !components
        .iter()
        .any(|component| element::canonical_managed_component_rank(&component.name).is_some())
    {
        return Ok(None);
    }
    let at = element::canonical_managed_component_insertion_offset(content, &components, "done");
    let mut out = String::with_capacity(content.len() + 64);
    out.push_str(&content[..at]);
    if !out.is_empty() && !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(agent_doc_element_done::EMPTY_DONE_COMPONENT);
    let rest = &content[at..];
    if !rest.is_empty() && !rest.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(rest);
    Ok(Some(out))
}

/// `YYYY-MM-DD` of a response timestamp, else the timestamp unchanged.
pub fn archive_date(timestamp: &str) -> String {
    let date = timestamp.get(..10).unwrap_or(timestamp);
    let bytes = date.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit());
    if shaped {
        date.to_string()
    } else {
        timestamp.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "\
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - ⟦📥 you: 0 · agent: 1 · done: 1⟧ [#alpha] Approve alpha [#subagent-operator-request-x]. <!-- agent-doc:waiting-item --> ⟦effect:2⟧
    - **Next:** agent — pick up your reply ▫️
    - yes, approved [#q-1]
    - ~~older~~

  - ⟦you: 1 · agent: 0 · done: 0⟧ [#beta] Approve beta. <!-- agent-doc:waiting-item -->
";

    #[test]
    fn segments_split_at_child_rows_and_keep_every_byte() {
        let segments = waiting_tree_segments(TREE, 0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id.as_deref(), Some("alpha"));
        assert_eq!(segments[1].id.as_deref(), Some("beta"));
        let rejoined = segments
            .iter()
            .map(|segment| segment.raw.as_str())
            .collect::<String>();
        let below_root = TREE.split_once('\n').unwrap().1;
        assert_eq!(rejoined, below_root);
    }

    #[test]
    fn an_archive_entry_is_one_receipt_over_a_byte_exact_quoted_packet() {
        let segment = &waiting_tree_segments(TREE, 0)[0];
        let entry = render_archive_entry("2026-11-04", "main", segment, ArchiveReason::TodoClosed);
        let receipt = entry.lines().next().unwrap();
        assert_eq!(
            receipt,
            "- 2026-11-04 archived waiting item `#alpha` (its Todo closed): `#alpha` Approve alpha `#subagent-operator-request-x`. — its 4 rows moved here from the Waiting on you tree of queue `main` <!-- agent-doc:waiting-archive -->"
        );
        assert_eq!(unquote_archive_entry(&entry), segment.raw);
        assert!(
            agent_doc_element_backlog::backlog::extract_done_item_own_ids(&entry).is_empty(),
            "archiving never resolves a tracked id: {entry}"
        );
        assert!(
            agent_doc_element_done::collect_done_item_own_ids(&entry).is_empty(),
            "{entry}"
        );
    }

    #[test]
    fn append_creates_done_and_never_duplicates_a_receipt() {
        let document = "<!-- agent:todo -->\n- [x] [#alpha] Approve alpha.\n<!-- /agent:todo -->\n";
        let segment = &waiting_tree_segments(TREE, 0)[0];
        let entry = render_archive_entry("2026-11-04", "main", segment, ArchiveReason::TodoClosed);
        let once = append_to_done(document, std::slice::from_ref(&entry))
            .unwrap()
            .unwrap();
        assert!(once.contains("<!-- agent:done -->"), "{once}");
        assert_eq!(once.matches(WAITING_ARCHIVE_MARKER).count(), 1);
        let twice = append_to_done(&once, std::slice::from_ref(&entry))
            .unwrap()
            .unwrap();
        assert_eq!(twice, once);
        assert_eq!(
            append_to_done("no components here\n", &[entry]).unwrap(),
            None
        );
    }

    #[test]
    fn archive_date_takes_the_day_of_a_timestamp() {
        assert_eq!(archive_date("2026-11-04T02:52:53.000Z"), "2026-11-04");
        assert_eq!(archive_date("later"), "later");
    }
}
