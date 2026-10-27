//! Nested sub-prompts of a runnable list row (`#nestedsubprompts`).
//!
//! In a runnable list component (`agent:run`, its `agent:queue` / `agent:loop`
//! aliases) an indented list beneath a free-text row is that row's
//! **sub-prompts**: extra instructions for the same piece of work, authored as
//! separate lines so the operator never has to keep appending to one long row.
//!
//! ```text
//! - Sync PR + Jira state into this document.
//!   - Start with GitHub, read-only.
//!   - Never print the Jira token.
//! ```
//!
//! is ONE queue head. Its text is the parent row followed by the child rows,
//! verbatim; the children are never independent heads, never dispatched on
//! their own, and never carry a lifecycle projection of their own.
//!
//! This module owns the text-level contract every layer shares:
//!
//! * [`split_subprompt_block`] separates a head's text into the parent part
//!   (the row plus any bullet-less `#queuecruft` continuation) and the
//!   sub-prompt block (the first indented list line onward);
//! * [`head_without_subprompts`] is the parent part alone. The head's work
//!   identity is derived from it, so editing, adding, striking or removing a
//!   child never changes which work the row names (a claim stays put and the
//!   edit is steering for the claim's owner);
//! * [`subprompt_row_is_eligible`] decides whether an indented list row can be
//!   a sub-prompt at all. A tracked-id directive (`do [#id]`, a bare `[#id]`)
//!   cannot: a subtree that contains one keeps the `#queuenest` / `#f1s3`
//!   group semantics, where the parent is a non-executable label and each
//!   tracked leaf is its own head.
//!
//! The segmentation itself (which lines a parent absorbs) lives with the two
//! queue enumerators (`agent_doc_markdown_ast::overlay` and
//! `agent_doc_queue::document_queue`), which both call
//! `agent_doc_markdown_ast::overlay::queue_subprompt_block_end`.

use crate::{QueueItemIdentity, strip_attempt_projection_prefix, strip_intent_tags};

/// Leading indentation width of `line`: spaces count one column, a tab four.
pub fn line_indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

/// The content after a Markdown list marker, when `line` (any indentation) is a
/// list item: `- x`, `* x`, `+ x`, `N. x`, `N) x`, or an empty marker (`-`,
/// `N.`) the operator is still typing. `None` for anything else.
pub fn list_item_content(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches([' ', '\t']).trim_end();
    if matches!(trimmed, "-" | "*" | "+") {
        return Some("");
    }
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            return Some(rest);
        }
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let after = &trimmed[digits..];
    for marker in [". ", ") "] {
        if let Some(rest) = after.strip_prefix(marker) {
            return Some(rest);
        }
    }
    matches!(after, "." | ")").then_some("")
}

/// True when `line` is an indented list item: the only line shape that can
/// open (or extend) a sub-prompt block beneath a parent row.
pub fn is_indented_list_line(line: &str) -> bool {
    line_indent_width(line) > 0 && list_item_content(line).is_some()
}

/// Whether an indented list row whose content (after its marker) is `content`
/// can be a sub-prompt of the row above it.
///
/// Everything is, except queue-native rows that carry their own scheduling
/// meaning:
///
/// * a tracked-id directive (`do [#id]`, `do #id`, a bare `[#id]` / `#id`
///   head), including one behind a stale lifecycle projection or a strike:
///   tracked work keeps its own lifecycle, `--done` target and backlog mirror
///   (`#queuenest` / `#f1s3`);
/// * a `preset ...` / `dispatch ...` line.
///
/// A `re [#id]` reference, an `[#id]: note` annotation, a struck child
/// (`~~done~~`), an intent-tagged line (`#gh-fix <url>`) and an empty
/// placeholder are all sub-prompt text.
pub fn subprompt_row_is_eligible(content: &str) -> bool {
    let work = strip_attempt_projection_prefix(content).trim();
    let work = strip_strike(work);
    let lower = work.to_ascii_lowercase();
    if lower.starts_with("preset ") || lower.starts_with("dispatch ") {
        return false;
    }
    if lower.starts_with("re [#") || lower.starts_with("re #") {
        return true;
    }
    if work.is_empty() {
        return true;
    }
    !QueueItemIdentity::from_prompt(&strip_intent_tags(work)).is_id_backed()
}

fn strip_strike(text: &str) -> &str {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix("~~") {
        return inner.strip_suffix("~~").unwrap_or(inner).trim();
    }
    text
}

/// Split a queue head's text into its parent part and its sub-prompt block.
///
/// The block starts at the first line after the first that is an indented list
/// item ([`is_indented_list_line`]); the parent part is everything before it,
/// without the joining newline. `#queuecruft` continuation never carries a list
/// marker, so the split is unambiguous for every list-item head the queue
/// enumerators produce. Text without a block returns `(text, None)` unchanged.
pub fn split_subprompt_block(text: &str) -> (&str, Option<&str>) {
    let mut offset = 0usize;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index > 0 && is_indented_list_line(line.trim_end_matches(['\n', '\r'])) {
            let head = text[..offset].trim_end_matches(['\n', '\r']);
            return (head, Some(&text[offset..]));
        }
        offset += line.len();
    }
    (text, None)
}

/// The parent part of a head's text ([`split_subprompt_block`]).
pub fn head_without_subprompts(text: &str) -> &str {
    split_subprompt_block(text).0
}

/// True when `text` carries a sub-prompt block.
pub fn has_subprompts(text: &str) -> bool {
    split_subprompt_block(text).1.is_some()
}

/// The sub-prompt rows of `text`: each indented list line's content after its
/// marker, with any following continuation lines of that row joined by `\n`.
pub fn subprompt_rows(text: &str) -> Vec<String> {
    let Some(block) = split_subprompt_block(text).1 else {
        return Vec::new();
    };
    let mut rows: Vec<String> = Vec::new();
    for line in block.lines() {
        if let Some(content) = list_item_content(line) {
            rows.push(content.trim().to_string());
        } else if let Some(last) = rows.last_mut() {
            last.push('\n');
            last.push_str(line.trim());
        }
    }
    rows
}

/// Remove binary-owned attempt projections (lifecycle markers, the
/// `[#owner#generation]` annotation) that a child row carries.
///
/// Children never carry their own projection: the parent row's marker is the
/// item's progress (`#nestedsubprompts`). Rows written before sub-prompts were
/// folded into their parent were each projected as an independent head, so a
/// legacy child can still start with `🟡 [#subagent:x#1]`. The binary owns
/// those bytes, so the next projection pass removes them. Operator text
/// (including a struck child) is never touched. Returns `None` when nothing
/// changed.
pub fn strip_subprompt_projections(text: &str) -> Option<String> {
    let (head, block) = split_subprompt_block(text);
    let block = block?;
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    out.push_str(head);
    out.push('\n');
    for line in block.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let newline = &line[body.len()..];
        if let Some(content) = list_item_content(body)
            && !content.is_empty()
        {
            let stripped = strip_attempt_projection_prefix(content);
            let prefix = &content[..content.len() - stripped.len()];
            // Only a stamped attempt projection (`<marker> [#owner#gen]`) is
            // binary-owned; a bare pin on a child is the operator's.
            if prefix.contains("[#") && !stripped.trim().is_empty() {
                let prefix_len = body.len() - content.len();
                out.push_str(&body[..prefix_len]);
                out.push_str(stripped);
                out.push_str(newline);
                changed = true;
                continue;
            }
        }
        out.push_str(line);
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_separates_parent_from_its_nested_rows() {
        let text =
            "Parent work\n  continuation prose\n  - child one\n    child one wraps\n  - child two";
        let (head, block) = split_subprompt_block(text);
        assert_eq!(head, "Parent work\n  continuation prose");
        assert_eq!(
            block,
            Some("  - child one\n    child one wraps\n  - child two")
        );
        assert_eq!(
            subprompt_rows(text),
            vec![
                "child one\nchild one wraps".to_string(),
                "child two".to_string()
            ]
        );
        assert_eq!(split_subprompt_block("flat row"), ("flat row", None));
        assert_eq!(
            split_subprompt_block("row\n  wrapped prose only"),
            ("row\n  wrapped prose only", None)
        );
    }

    #[test]
    fn tracked_directives_are_not_subprompts_but_references_and_prose_are() {
        for ineligible in [
            "do [#a]",
            "🟡 [#subagent:x#1] do [#a]",
            "[#a]",
            "~~do [#a]~~",
            "preset #spec",
            "dispatch #spec",
        ] {
            assert!(!subprompt_row_is_eligible(ineligible), "{ineligible}");
        }
        for eligible in [
            "So this is the first subtask.",
            "🟡 [#subagent:pmsync#1] This is the second subtask.",
            "re [#a]",
            "[#a]: note about a",
            "~~done child~~",
            "#gh-fix https://github.com/o/r/issues/1",
            "",
        ] {
            assert!(subprompt_row_is_eligible(eligible), "{eligible:?}");
        }
    }

    #[test]
    fn legacy_child_projections_are_binary_owned_and_removed() {
        let text = "📥 Parent\n  - 🚧 [#subagent:pmsync#1] first\n  - 🟡 [#subagent:pmsync#1] second\n  - ~~done~~";
        assert_eq!(
            strip_subprompt_projections(text).as_deref(),
            Some("📥 Parent\n  - first\n  - second\n  - ~~done~~")
        );
        assert_eq!(
            strip_subprompt_projections("Parent\n  - first\n  - ~~done~~"),
            None
        );
        assert_eq!(strip_subprompt_projections("flat"), None);
    }
}
