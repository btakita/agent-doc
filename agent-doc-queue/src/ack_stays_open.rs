//! `#ackstrikeopenbacklog`: an acknowledged `do [#id]` head whose tracked item
//! stays open must say so on the struck row.
//!
//! `queue consume --ack-id <id>` strikes a reported id-backed row while its
//! backlog item or Todo stays open (`#nestedechostrike`, `#ackkeepsopen`). The
//! strike alone read as "done": haiven-dev `tasks/sdk/sdk-dev.md`
//! (2026-11-04) showed `- ### ~~do [#livestreamtree]~~` in the queue next to
//! an unchecked `- [ ] [#livestreamtree] ... (stays open: ...)` backlog item,
//! and nothing in the queue said the item was still open or why. Re-projecting
//! a live head is not an option: `#ackkeepsopen` exists because a restored
//! head was dispatched again in a `subagents` queue.
//!
//! The acknowledgement therefore annotates the struck row with the canonical
//! outside-the-wrapper note (`~~do [#id]~~ — auto-struck: acknowledged;
//! backlog #id stays open: <reason>`), the same `#qstrikenote` shape both
//! queue parsers already read as a completed row with clean work text. The
//! reason is the tracked item's own `(stays open: ...)` clause, so a wrong
//! reason is visible beside the struck head instead of hidden in the backlog.
//!
//! This module is pure: document text in, document text out.

use std::collections::HashSet;

use agent_doc_element::{element, todo};
use agent_doc_element_backlog::backlog;
use agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR;

/// The fixed lead of the acknowledgement note; also the idempotency marker.
pub const ACK_STAYS_OPEN_NOTE_LEAD: &str = "acknowledged; ";

/// Shown when the tracked item records no `(stays open: ...)` reason.
pub const NO_STAYS_OPEN_REASON: &str = "no reason recorded";

/// Longest reason, in characters, copied into the note.
const MAX_REASON_CHARS: usize = 240;

/// The kind of open tracked item an acknowledged head leaves open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTrackedKind {
    Backlog,
    Todo,
}

impl OpenTrackedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "Todo",
        }
    }
}

/// The open tracked item named by an acknowledged head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTrackedItem {
    pub id: String,
    pub kind: OpenTrackedKind,
    /// The item's first `(stays open: ...)` clause, sanitized for a queue
    /// line, or `None` when the item records none.
    pub reason: Option<String>,
}

impl OpenTrackedItem {
    /// The note appended after the strike wrapper.
    pub fn note(&self) -> String {
        format!(
            "{ACK_STAYS_OPEN_NOTE_LEAD}{} #{} stays open: {}",
            self.kind.label(),
            self.id,
            self.reason.as_deref().unwrap_or(NO_STAYS_OPEN_REASON),
        )
    }
}

/// The open backlog item (any backlog-family component) or open Todo whose
/// own id is `id`, with its stays-open reason. Backlog wins when both exist,
/// matching the `preserved=` token of the acknowledgement.
pub fn open_tracked_item(content: &str, id: &str) -> Option<OpenTrackedItem> {
    let id = backlog::normalize_pending_id(id).to_ascii_lowercase();
    if id.is_empty() {
        return None;
    }
    if let Ok(components) = element::parse(content) {
        for component in components
            .iter()
            .filter(|component| element::is_backlog_component(&component.name))
        {
            let (_, items, _) = backlog::parse_items(component.content(content));
            if let Some(item) = items
                .iter()
                .find(|item| !item.is_done() && item.id.eq_ignore_ascii_case(&id))
            {
                return Some(OpenTrackedItem {
                    id,
                    kind: OpenTrackedKind::Backlog,
                    reason: first_stays_open_reason(&item.text),
                });
            }
        }
    }
    let todo_item = todo::todo_item_owning_id(content, &id)?;
    if !todo_item.state.is_open() {
        return None;
    }
    let first_line = todo_item.text.lines().next().unwrap_or_default();
    Some(OpenTrackedItem {
        id,
        kind: OpenTrackedKind::Todo,
        reason: first_stays_open_reason(first_line),
    })
}

/// The first non-empty `(stays open: ...)` clause of `text`, reduced to one
/// queue-safe line: no strike markers, `[#x]` links written as `#x` (so the
/// note never reads as another id-backed directive), whitespace collapsed,
/// and capped at [`MAX_REASON_CHARS`].
pub fn first_stays_open_reason(text: &str) -> Option<String> {
    crate::queue_blocked::stays_open_clause_bodies(text)
        .into_iter()
        .map(sanitize_reason)
        .find(|reason| !reason.is_empty())
}

fn sanitize_reason(raw: &str) -> String {
    let mut text = raw.replace("~~", "");
    // `[#id]` -> `#id`: keep the citation readable without a link shape.
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("[#") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(']') {
            Some(close)
                if after[1..close]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')) =>
            {
                out.push_str(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > MAX_REASON_CHARS {
        let cut: String = text.chars().take(MAX_REASON_CHARS).collect();
        text = format!("{}…", cut.trim_end());
    }
    text
}

/// Append `note` after the strike wrapper of every queue row the successor
/// `after` struck relative to `before` among the acknowledged `node_keys`.
/// A row that already carries the acknowledgement note is left alone; a row
/// that already carries another note gains this one after `; `. Multi-line
/// `~~~done` fences never carry a note (`#multilinequeuehead`).
pub fn annotate_acknowledged_heads(
    before: &str,
    after: &str,
    node_keys: &[String],
    note: &str,
) -> String {
    let keys: HashSet<&str> = node_keys.iter().map(String::as_str).collect();
    let struck_before: HashSet<String> =
        agent_doc_markdown_ast::mutations::item_nodes_all(before, "queue")
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|node| node.item.struck)
                    .map(|node| node.node_key)
                    .collect()
            })
            .unwrap_or_default();
    let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(after, "queue") else {
        return after.to_string();
    };
    let mut edits: Vec<(usize, usize)> = nodes
        .iter()
        .filter(|node| {
            node.item.struck
                && keys.contains(node.node_key.as_str())
                && !struck_before.contains(&node.node_key)
                && node.item.surface
                    != agent_doc_markdown_ast::overlay::ItemSurface::MultilinePrompt
        })
        .map(|node| (node.item.start_byte, node.item.end_byte))
        .collect();
    if edits.is_empty() {
        return after.to_string();
    }
    edits.sort_by_key(|(start, _)| *start);
    edits.dedup();
    let mut out = after.to_string();
    for (start, end) in edits.into_iter().rev() {
        let annotated = annotate_struck_line(&out[start..end], note);
        out.replace_range(start..end, &annotated);
    }
    out
}

/// Append `note` to one struck single-line queue row.
pub fn annotate_struck_line(line: &str, note: &str) -> String {
    let (core, newline) = match line.strip_suffix('\n') {
        Some(rest) => (rest, "\n"),
        None => (line, ""),
    };
    let trimmed_end = core.trim_end();
    if trimmed_end.contains('\n') || trimmed_end.trim_start().starts_with("~~~") {
        return line.to_string();
    }
    if trimmed_end.contains(ACK_STAYS_OPEN_NOTE_LEAD.trim_end()) {
        return line.to_string();
    }
    let trailing_ws = &core[trimmed_end.len()..];
    if trimmed_end.contains(STRUCK_ANNOTATION_SEPARATOR) {
        return format!("{trimmed_end}; {note}{trailing_ws}{newline}");
    }
    if !trimmed_end.ends_with("~~") {
        return line.to_string();
    }
    format!("{trimmed_end}{STRUCK_ANNOTATION_SEPARATOR}{note}{trailing_ws}{newline}")
}

/// Closeout guidance printed after an acknowledgement leaves `item` open: the
/// acknowledgement is right only while the item really stays open. When the
/// answer resolves it, the agent closes the item (and any parent the same
/// answer resolves) instead of leaving a struck head beside open work.
pub fn ack_stays_open_guidance(document: &str, item: &OpenTrackedItem) -> String {
    let id = &item.id;
    let close = match item.kind {
        OpenTrackedKind::Backlog => format!("`agent-doc backlog {document} done {id}`"),
        OpenTrackedKind::Todo => format!("`--done {id}` on `agent-doc finalize {document}`"),
    };
    let reason = match &item.reason {
        Some(reason) => format!("stays open: {reason}"),
        None => format!(
            "stays open with {NO_STAYS_OPEN_REASON}; record why with a `(stays open: <reason>)` \
             clause on the item"
        ),
    };
    format!(
        "{} [#{id}] {reason}. The struck head now shows this (#ackstrikeopenbacklog). If the \
         answer resolves the item, close it with {close}, and close any parent item or waiting \
         row the same answer resolves; `--ack-id` only acknowledges the head. If it really stays \
         open, keep the reason accurate.",
        item.kind.label(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sdk-dev.md shape (haiven-dev, 2026-11-04): a reported
    /// `do [#livestreamtree]` head acknowledged while the backlog item stays
    /// open with a stays-open clause that cites the parent tree.
    fn sdk_dev_like(head: &str) -> String {
        format!(
            concat!(
                "## Run <!-- agent:run priority subagents go -->\n",
                "{head}\n",
                "- ### do [#p7d7]\n",
                "<!-- /agent:run -->\n\n",
                "## Backlog <!-- agent:backlog priority queue=\"main\" -->\n",
                "- [ ] [#livestreamtree] No livestream. Is this tree done? (stays open: no ",
                "livestream and nothing was trimmed, and [#tdoauthcodestream] still has ",
                "unstruck children)\n",
                "- [ ] [#p7d7] Is this done?\n",
                "<!-- /agent:backlog -->\n",
            ),
            head = head
        )
    }

    #[test]
    fn open_backlog_item_reason_is_its_stays_open_clause_without_link_shapes() {
        let content = sdk_dev_like("- ### do [#livestreamtree]");
        let item = open_tracked_item(&content, "#LiveStreamTree").expect("open backlog item");
        assert_eq!(item.kind, OpenTrackedKind::Backlog);
        assert_eq!(
            item.reason.as_deref(),
            Some(
                "no livestream and nothing was trimmed, and #tdoauthcodestream still has unstruck children"
            )
        );
        assert_eq!(
            item.note(),
            "acknowledged; backlog #livestreamtree stays open: no livestream and nothing was \
             trimmed, and #tdoauthcodestream still has unstruck children"
        );
        let missing = open_tracked_item(&content, "p7d7").expect("open backlog item");
        assert_eq!(missing.reason, None);
        assert!(missing.note().ends_with("stays open: no reason recorded"));
        assert!(open_tracked_item(&content, "absent").is_none());
    }

    #[test]
    fn annotated_struck_head_stays_a_completed_row_with_clean_work_text() {
        let before = sdk_dev_like("- ### do [#livestreamtree]");
        let nodes = agent_doc_markdown_ast::mutations::item_nodes_all(&before, "queue").unwrap();
        let key = nodes
            .iter()
            .find(|node| node.item.text.contains("livestreamtree"))
            .unwrap()
            .node_key
            .clone();
        let struck =
            crate::queue_consume::consume_queue_nodes_by_key(&before, std::slice::from_ref(&key))
                .unwrap();
        let item = open_tracked_item(&before, "livestreamtree").unwrap();
        let after =
            annotate_acknowledged_heads(&before, &struck, std::slice::from_ref(&key), &item.note());
        let expected_line = format!(
            "- ### ~~do [#livestreamtree]~~{STRUCK_ANNOTATION_SEPARATOR}{}\n",
            item.note()
        );
        assert!(after.contains(&expected_line), "{after}");
        assert!(after.contains("- ### do [#p7d7]\n"), "{after}");

        // Both parsers read the row as struck `do [#livestreamtree]`.
        let reread = agent_doc_markdown_ast::mutations::item_nodes_all(&after, "queue").unwrap();
        let row = reread.iter().find(|node| node.node_key == key).unwrap();
        assert!(row.item.struck);
        assert_eq!(row.item.text.trim(), "do [#livestreamtree]");
        let components = element::parse(&after).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        let entries = crate::document_queue::parse(queue.content(&after)).unwrap();
        assert!(entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Completed(prompt)
                if prompt.text == "do [#livestreamtree]"
        )));
        assert!(
            !entries.iter().any(|entry| matches!(
                entry,
                crate::document_queue::QueueEntry::Prompt(prompt)
                    if prompt.text.contains("livestreamtree")
            )),
            "the note must never make the struck row live again"
        );

        // Idempotent: a second pass over the annotated successor is a no-op.
        assert_eq!(
            annotate_acknowledged_heads(&before, &after, &[key], &item.note()),
            after
        );
    }

    #[test]
    fn annotation_skips_rows_already_struck_and_rows_outside_the_keys() {
        let before = sdk_dev_like("- ### ~~do [#livestreamtree]~~");
        let nodes = agent_doc_markdown_ast::mutations::item_nodes_all(&before, "queue").unwrap();
        let keys: Vec<String> = nodes.iter().map(|node| node.node_key.clone()).collect();
        assert_eq!(
            annotate_acknowledged_heads(&before, &before, &keys, "acknowledged; x"),
            before
        );
    }

    #[test]
    fn struck_line_with_an_existing_note_gains_the_ack_note_after_a_semicolon() {
        let line = format!("- ### ~~do [#a]~~{STRUCK_ANNOTATION_SEPARATOR}response: [#r-1]\n");
        assert_eq!(
            annotate_struck_line(&line, "acknowledged; backlog #a stays open: x"),
            format!(
                "- ### ~~do [#a]~~{STRUCK_ANNOTATION_SEPARATOR}response: [#r-1]; acknowledged; backlog #a stays open: x\n"
            )
        );
        assert_eq!(annotate_struck_line("- do [#a]\n", "n"), "- do [#a]\n");
        assert_eq!(annotate_struck_line("~~~\n", "n"), "~~~\n");
    }

    #[test]
    fn reason_sanitizer_keeps_one_queue_safe_line() {
        assert_eq!(
            first_stays_open_reason("x (stays open:  ~~a~~\n  b [#c] [not-id] (#d)) y").as_deref(),
            Some("a b #c [not-id] (#d)")
        );
        assert_eq!(
            first_stays_open_reason("x (stays open: ) (stays open: z)").as_deref(),
            Some("z")
        );
        let long = format!("(stays open: {})", "w".repeat(400));
        let reason = first_stays_open_reason(&long).unwrap();
        assert_eq!(reason.chars().count(), MAX_REASON_CHARS + 1);
        assert!(reason.ends_with('…'));
    }

    #[test]
    fn guidance_points_at_closing_the_item_and_its_resolved_parent() {
        let content = sdk_dev_like("- ### do [#livestreamtree]");
        let item = open_tracked_item(&content, "livestreamtree").unwrap();
        let guidance = ack_stays_open_guidance("tasks/sdk/sdk-dev.md", &item);
        assert!(
            guidance.contains("`agent-doc backlog tasks/sdk/sdk-dev.md done livestreamtree`"),
            "{guidance}"
        );
        assert!(guidance.contains("close any parent item"), "{guidance}");
        assert!(guidance.contains("#tdoauthcodestream still has unstruck children"));
        let bare = open_tracked_item(&content, "p7d7").unwrap();
        assert!(
            ack_stays_open_guidance("d.md", &bare).contains("`(stays open: <reason>)`"),
            "a missing reason asks for one"
        );
    }

    #[test]
    fn open_todo_item_is_reported_when_no_backlog_item_owns_the_id() {
        let content = concat!(
            "## Run <!-- agent:run -->\n",
            "- ### do [#checkit]\n",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#checkit] Verify on device (stays open: waits on the operator check)\n",
            "<!-- /agent:todo -->\n",
        );
        let item = open_tracked_item(content, "checkit").expect("open todo");
        assert_eq!(item.kind, OpenTrackedKind::Todo);
        assert_eq!(item.reason.as_deref(), Some("waits on the operator check"));
    }
}
