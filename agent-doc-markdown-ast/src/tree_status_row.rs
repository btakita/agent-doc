//! Reserved syntax of the queue tree-status query (`#treestatusquery`).
//!
//! The operator asks whether a queue tree or subtree has actionable or
//! deletable work by typing a status query, and agent-doc answers it from the
//! queue tree without a model turn:
//!
//! - a child line `- status?` under the node (or `- status? deep`);
//! - a `[status?]` (or `[status? deep]`) token on the node's own line;
//! - a child line `- deletable?` (or a `[deletable?]` token) asking only
//!   whether that prompt item tree is deletable.
//!
//! Preflight replaces the query with a binary-owned **Status** reply block at
//! the node's first child position:
//!
//! ```text
//! - do [#item]
//!   - **Status:** snapshot — 1 you · 2 agent · 1 deletable · 0 need review ▫️
//!     - **Next: you:** “…” — answer: … ▫️
//!     - **Next: agent:** “…” — let the running attempt finish ▫️
//!     - **Deletable:** “…” — every row is struck and its response says safe to delete ▫️
//!     - **Needs agent review:** “…” — skipped; … ▫️
//! ```
//!
//! Both the query line and the Status rows are inert queue text: they never
//! become queue prompts, operator answers, or backlog items. Ownership of a
//! Status row is its rendered shape, like every other `▫️` reply row: the
//! trailing glyph, a `**Status:**` label (or one of [`TREE_STATUS_CHILD_LABELS`]
//! directly beneath a `**Status:**` row).

use crate::overlay::strip_bullet;
use crate::reply_row::AGENT_DOC_REPLY_GLYPH;

/// The plain status query, typed as a child line.
pub const STATUS_QUERY: &str = "status?";
/// The deep status query: also queues an agent turn for unclassified rows.
pub const STATUS_QUERY_DEEP: &str = "status? deep";
/// The status query token on the node's own line.
pub const STATUS_QUERY_TOKEN: &str = "[status?]";
/// The deep status query token on the node's own line.
pub const STATUS_QUERY_DEEP_TOKEN: &str = "[status? deep]";
/// The deletability query: is this prompt item tree deletable?
pub const DELETABLE_QUERY: &str = "deletable?";

/// Label of the Status reply row.
pub const TREE_STATUS_ROW_LABEL: &str = "**Status:**";
/// Labels of the rows nested directly beneath a Status row.
pub const TREE_STATUS_CHILD_LABELS: [&str; 4] = [
    "**Next: you:**",
    "**Next: agent:**",
    "**Deletable:**",
    "**Needs agent review:**",
];

/// Which status query the operator typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusQueryKind {
    /// `status?`: answered by the binary alone.
    Plain,
    /// `status? deep`: answered by the binary, plus one agent turn for the
    /// rows it could not classify.
    Deep,
    /// `deletable?`: answered by the binary with whether this prompt item
    /// tree is safe to delete. Never a reason to keep that tree open.
    Deletable,
}

/// `text` without the `### ` navigation heading a top-level queue row may
/// carry.
fn without_heading(text: &str) -> &str {
    let trimmed = text.trim_start();
    trimmed.strip_prefix("### ").unwrap_or(trimmed)
}

fn normalized(text: &str) -> String {
    without_heading(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// The status query a list item's content (the text after its bullet) is, if
/// it is exactly one: `status?` or `status? deep`, case-insensitive, with any
/// whitespace.
pub fn status_query_kind(item_content: &str) -> Option<StatusQueryKind> {
    match normalized(item_content).as_str() {
        STATUS_QUERY => Some(StatusQueryKind::Plain),
        STATUS_QUERY_DEEP => Some(StatusQueryKind::Deep),
        DELETABLE_QUERY => Some(StatusQueryKind::Deletable),
        _ => None,
    }
}

/// [`status_query_kind`] over a markdown source line (`- status?`).
pub fn status_query_line_kind(line: &str) -> Option<StatusQueryKind> {
    strip_bullet(line).and_then(status_query_kind)
}

/// The byte range and kind of a `[status?]` / `[status? deep]` token in
/// `text`, case-insensitive.
pub fn find_status_query_token(text: &str) -> Option<(std::ops::Range<usize>, StatusQueryKind)> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(offset) = lower[from..].find('[') {
        let start = from + offset;
        let close = lower[start..].find(']')?;
        let end = start + close + 1;
        let inner = &text[start + 1..end - 1];
        // A token quoted inside an inline code span is documentation, not a
        // query.
        let in_code = text[..start].matches('`').count() % 2 == 1;
        if !in_code && let Some(kind) = status_query_kind(inner) {
            return Some((start..end, kind));
        }
        from = start + 1;
    }
    None
}

/// `text` without its status query token, with the space the token left
/// collapsed. Returns `None` when there is no token.
pub fn strip_status_query_token(text: &str) -> Option<(String, StatusQueryKind)> {
    let (range, kind) = find_status_query_token(text)?;
    let before = text[..range.start].trim_end_matches(' ');
    let after = text[range.end..].trim_start_matches(' ');
    let joined = match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_string(),
        (_, true) => before.to_string(),
        _ => format!("{before} {after}"),
    };
    Some((joined, kind))
}

fn glyph_content(body: &str) -> Option<&str> {
    let before = body.trim_end().strip_suffix(AGENT_DOC_REPLY_GLYPH)?;
    if !before.ends_with(char::is_whitespace) {
        return None;
    }
    let content = before.trim();
    (!content.is_empty()).then_some(content)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Whether a list item's content (the text after its bullet) is a rendered
/// `**Status:**` row: the label first (after an optional `### ` navigation
/// heading) and the reply glyph last.
pub fn is_tree_status_head_body(item_content: &str) -> bool {
    glyph_content(without_heading(item_content))
        .is_some_and(|content| content.starts_with(TREE_STATUS_ROW_LABEL))
}

/// Whether `lines[index]` is a binary-owned `**Status:**` row.
pub fn is_tree_status_head_row_at(lines: &[&str], index: usize) -> bool {
    lines
        .get(index)
        .and_then(|line| strip_bullet(line))
        .is_some_and(is_tree_status_head_body)
}

/// Whether `lines[index]` is any binary-owned Status row: the `**Status:**`
/// row, or a labelled row directly beneath one.
pub fn is_tree_status_row_at(lines: &[&str], index: usize) -> bool {
    if is_tree_status_head_row_at(lines, index) {
        return true;
    }
    let Some(line) = lines.get(index) else {
        return false;
    };
    let is_child = strip_bullet(line)
        .and_then(glyph_content)
        .is_some_and(|content| {
            TREE_STATUS_CHILD_LABELS
                .iter()
                .any(|label| content.starts_with(label))
        });
    if !is_child {
        return false;
    }
    let indent = indent_of(line);
    let parent = lines[..index]
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .find(|(_, line)| indent_of(line) < indent);
    parent.is_some_and(|(parent, _)| is_tree_status_head_row_at(lines, parent))
}

/// The exclusive end of the inert row at `opener_index` when it is a status
/// query line or a Status row. Only the row itself is reserved: an
/// operator-authored child beneath it stays ordinary queue text.
pub fn tree_status_child_end(lines: &[&str], opener_index: usize) -> Option<usize> {
    let line = lines.get(opener_index)?;
    (status_query_line_kind(line).is_some() || is_tree_status_row_at(lines, opener_index))
        .then_some(opener_index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_lines_and_tokens_are_recognized_exactly() {
        assert_eq!(status_query_kind("status?"), Some(StatusQueryKind::Plain));
        assert_eq!(status_query_kind(" Status? "), Some(StatusQueryKind::Plain));
        assert_eq!(
            status_query_kind("status?   DEEP"),
            Some(StatusQueryKind::Deep)
        );
        assert_eq!(status_query_kind("status? of the release"), None);
        assert_eq!(status_query_kind("what is the status?"), None);
        assert_eq!(
            status_query_kind("deletable?"),
            Some(StatusQueryKind::Deletable)
        );
        assert_eq!(
            status_query_kind(" Deletable? "),
            Some(StatusQueryKind::Deletable)
        );
        assert_eq!(status_query_kind("is this deletable?"), None);
        assert_eq!(
            status_query_line_kind("    - deletable?"),
            Some(StatusQueryKind::Deletable)
        );
        let (stripped, kind) = strip_status_query_token("do [#alpha] [deletable?]").unwrap();
        assert_eq!(
            (stripped.as_str(), kind),
            ("do [#alpha]", StatusQueryKind::Deletable)
        );
        assert_eq!(
            status_query_line_kind("    - status?"),
            Some(StatusQueryKind::Plain)
        );
        assert_eq!(status_query_line_kind("status?"), None, "a list row only");

        let (stripped, kind) = strip_status_query_token("do [#alpha] [status?]").unwrap();
        assert_eq!(
            (stripped.as_str(), kind),
            ("do [#alpha]", StatusQueryKind::Plain)
        );
        let (stripped, kind) = strip_status_query_token("[Status? deep] do [#alpha] now").unwrap();
        assert_eq!(
            (stripped.as_str(), kind),
            ("do [#alpha] now", StatusQueryKind::Deep)
        );
        assert!(strip_status_query_token("do [#alpha] [#status]").is_none());
        assert!(
            strip_status_query_token("document the `[status?]` token").is_none(),
            "a quoted token is not a query"
        );
    }

    #[test]
    fn status_rows_are_owned_only_in_their_rendered_shape() {
        let lines = [
            "- do [#alpha]",
            "  - **Status:** snapshot — 0 you · 1 agent · 0 deletable · 0 need review ▫️",
            "    - **Next: agent:** “do [#alpha]” — pick up this queued item ▫️",
            "    - operator note",
            "  - **Next: agent:** lookalike outside a Status row ▫️",
            "  - **Status:** no glyph",
        ];
        assert!(is_tree_status_row_at(&lines, 1));
        assert!(is_tree_status_row_at(&lines, 2));
        assert!(!is_tree_status_row_at(&lines, 3));
        assert!(!is_tree_status_row_at(&lines, 4));
        assert!(!is_tree_status_row_at(&lines, 5));
        assert_eq!(tree_status_child_end(&lines, 1), Some(2));
        assert_eq!(tree_status_child_end(&lines, 3), None);
        assert_eq!(tree_status_child_end(&["  - status? deep"], 0), Some(1));
        // A top-level row's navigation heading does not change ownership.
        let headed = [
            "- ### **Status:** snapshot — 0 you · 0 agent · 0 deletable · 0 need review ▫️",
            "  - **Deletable:** “x” — every row is struck ▫️",
        ];
        assert!(is_tree_status_row_at(&headed, 0));
        assert!(is_tree_status_row_at(&headed, 1));
        assert_eq!(
            status_query_line_kind("- ### status?"),
            Some(StatusQueryKind::Plain)
        );
    }
}
