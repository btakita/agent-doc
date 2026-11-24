//! `#pipeprompt` inline form: `- #review | #address` on one row.
//!
//! A row is an inline pipe only when splitting it on ` | ` / ` |> ` outside
//! code spans gives two or more segments and every segment starts with a
//! preset ([`agent_doc_element_queue::pipe::inline_pipe_segments`]). It is
//! the same statement as the nested form, so the two round-trip:
//!
//! ```text
//! - #review | #address            - #review |
//!                                   - #address
//! ```
//!
//! * **` | ` only:** the row is ONE task, a pipe statement whose stages are
//!   its segments. Nothing is rewritten: the stage readers
//!   ([`agent_doc_element_queue::pipe::statement_stage_pipelines`]) split the
//!   row, so the submission, `#presetorder` and `#directiveposition` see the
//!   same stages as the nested form.
//! * **` |> ` between segments:** each hand-off stage needs its own row
//!   (claim, generation, response, `✅`), so [`expand_inline_hand_offs`]
//!   writes the nested ` |>` form the row stands for, once, before any attempt
//!   holds the row. `- #rebase |> #address |> #adversarial-pr` becomes
//!   `- #rebase |>` over `- #address |>` over `- #adversarial-pr`; ` | `
//!   inside one stage stays inline (`#a | #b |> #c` → `#a | #b |>` over
//!   `#c`). The row keeps its q-id on stage 1; its children move under the
//!   last stage, which keeps the row's own trailing operator.
//!
//! A row an attempt already holds (claimed, running, reported, a live ledger
//! claim) is never rewritten: it runs as one task, its stages in order.

use agent_doc_element::element;
use agent_doc_element_queue::pipe::{
    EdgeOp, INLINE_HAND_OFF_JOIN, inline_pipe_segments_with, strip_edge_marker, with_edge_marker,
};

use crate::document_queue::{self, QueueEntry};
use crate::self_repair::{SelfRepair, SelfRepairKind};

/// Whether `tag` (`#review`) names a preset in `presets`, with or without
/// its `#`, ignoring case.
pub fn is_known_preset(presets: &[(String, String)], tag: &str) -> bool {
    let bare = tag.trim().trim_start_matches('#');
    !bare.is_empty()
        && presets.iter().any(|(name, _)| {
            name.trim()
                .trim_start_matches('#')
                .eq_ignore_ascii_case(bare)
        })
}

/// The nested ` |>` form of one open inline hand-off row, or `None`.
///
/// `content` is the row after its list marker (a top-level `### ` heading
/// kept in `prefix` by the caller); `child_indent` is the indent its first
/// stage row takes (the row's indent plus its list-marker width). Returns
/// the rewritten head content and the later stage rows, each a full line
/// without its newline, plus how far the row's own children shift right.
fn nested_hand_off_rows(
    content: &str,
    child_indent: usize,
    presets: &[(String, String)],
) -> Option<(String, Vec<String>, usize)> {
    let first_end = content.find('\n').unwrap_or(content.len());
    let first = &content[..first_end];
    let trailing = EdgeOp::of_row(first);
    let stripped = strip_edge_marker(first);
    let segments = inline_pipe_segments_with(&stripped, |tag| is_known_preset(presets, tag))?;
    // Stages: runs of segments split at each ` |> ` join.
    let mut stages: Vec<(usize, usize)> = Vec::new();
    let mut start = segments[0].range.start;
    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();
        if last || segment.next == EdgeOp::HandOff {
            stages.push((start, segment.range.end));
            if let Some(next) = segments.get(index + 1) {
                start = next.range.start;
            }
        }
    }
    if stages.len() < 2 {
        return None;
    }
    // Only binary-owned bookkeeping that holds no attempt may precede the
    // first segment: the seen glyph and priority pins.
    if !only_unattempted_markers(&stripped[..segments[0].range.start]) {
        return None;
    }
    let tail = &stripped[segments.last()?.range.end..];
    let head = format!("{}{tail}", &stripped[..stages[0].1]);
    let head = with_edge_marker(&head, EdgeOp::HandOff);
    let mut rows = Vec::with_capacity(stages.len() - 1);
    for (position, &(start, end)) in stages.iter().enumerate().skip(1) {
        let indent = child_indent + 2 * (position - 1);
        let text = &stripped[start..end];
        let op = if position + 1 == stages.len() {
            trailing
        } else {
            EdgeOp::HandOff
        };
        rows.push(format!(
            "{}- {}",
            " ".repeat(indent),
            with_edge_marker(text, op)
        ));
    }
    let shift = 2 * (stages.len() - 1);
    let mut head = head;
    head.push_str(&content[first_end..]);
    Some((head, rows, shift))
}

/// Whether `prefix` holds only the seen glyph, priority pins and spaces.
fn only_unattempted_markers(prefix: &str) -> bool {
    let icons = agent_doc_queue_icons::queue_icons();
    let allowed = [
        icons.seen.as_str(),
        icons.prioritized.as_str(),
        icons.agent_prioritized.as_str(),
        "📌",
        "📍",
    ];
    let mut rest = prefix.trim();
    while !rest.is_empty() {
        let Some(after) = allowed
            .iter()
            .filter(|marker| !marker.is_empty())
            .find_map(|marker| rest.strip_prefix(marker))
        else {
            return false;
        };
        rest = after.trim_start();
    }
    true
}

/// Rewrite every open inline hand-off row of the runnable components
/// (`agent:run`, `agent:queue`, `agent:loop`) into its nested ` |>` form.
///
/// `presets` are the document's effective presets: every segment must start
/// with one of them. `is_claimed(row)` is the claim ledger's answer for a
/// row; a claimed row, a row with an attempt projection, a struck row, a row
/// inside a folded block, and a row with anything but open rows under it are
/// left alone. Idempotent: the nested form has no ` |> ` between segments.
pub fn expand_inline_hand_offs(
    content: &str,
    presets: &[(String, String)],
    is_claimed: &dyn Fn(&str) -> bool,
) -> (String, Vec<SelfRepair>) {
    if !content.contains(INLINE_HAND_OFF_JOIN) || presets.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let Ok(components) = element::parse(content) else {
        return (content.to_string(), Vec::new());
    };
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut repairs = Vec::new();
    for component in components
        .iter()
        .filter(|component| matches!(component.name.as_str(), "queue" | "run" | "loop"))
    {
        let body_start = component.open_end;
        let body = &content[component.open_end..component.close_start];
        if !body.contains(INLINE_HAND_OFF_JOIN) {
            continue;
        }
        let Ok(spans) = document_queue::parse_spans(body) else {
            continue;
        };
        for (index, (entry, range)) in spans.iter().enumerate() {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            let span = &body[range.clone()];
            let line_end = span.find('\n').unwrap_or(span.len());
            let line = &span[..line_end];
            if !line.contains(INLINE_HAND_OFF_JOIN) {
                continue;
            }
            let Some((prefix, row)) = crate::hand_off::split_row_prefix(line) else {
                continue;
            };
            let Some((_, ordered, _)) = document_queue::split_list_item(line) else {
                continue;
            };
            let marker_width = ordered.map_or(2, |marker| marker.len() + 1);
            if is_claimed(row) {
                continue;
            }
            // The row's subtree: later entries indented deeper. Each must be
            // an open row that holds no attempt of its own.
            let mut block_end = range.end;
            let mut clean = true;
            for (later, later_range) in &spans[index + 1..] {
                let indent = match later {
                    QueueEntry::Prompt(child) | QueueEntry::Completed(child) => Some(child.indent),
                    QueueEntry::Freeform(raw) => raw
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .map(|line| line.len() - line.trim_start().len()),
                    _ => None,
                };
                match (later, indent) {
                    (_, Some(indent)) if indent <= prompt.indent => break,
                    (QueueEntry::Prompt(child), Some(_))
                        if agent_doc_element_queue::strip_attempt_projection_prefix(
                            child.text.trim(),
                        ) == child.text.trim() =>
                    {
                        block_end = later_range.end;
                    }
                    (_, None) => break,
                    _ => {
                        clean = false;
                        break;
                    }
                }
            }
            if !clean {
                continue;
            }
            let Some((head, stage_rows, shift)) =
                nested_hand_off_rows(row, prompt.indent + marker_width, presets)
            else {
                continue;
            };
            let mut replacement = format!("{prefix}{head}");
            for stage in &stage_rows {
                replacement.push('\n');
                replacement.push_str(stage);
            }
            // The row's own block (folded children, continuation lines) and
            // its nested rows move under the last stage.
            let rest = &body[range.start + line_end..block_end];
            for piece in rest.split_inclusive('\n') {
                if piece.trim().is_empty() {
                    replacement.push_str(piece);
                } else {
                    replacement.push_str(&" ".repeat(shift));
                    replacement.push_str(piece);
                }
            }
            let start = body_start + range.start;
            edits.push((start..body_start + block_end, replacement));
            repairs.push(SelfRepair {
                kind: SelfRepairKind::InlineHandOff,
                detail: row.to_string(),
                rewritten: Some((row.to_string(), head)),
            });
        }
    }
    if edits.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let mut out = content.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    (out, repairs)
}

#[cfg(test)]
#[path = "inline_pipe_tests.rs"]
mod tests;
