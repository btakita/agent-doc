//! Row-final edge operators: the ` |` pipe (`#pipeprompt`) and the ` >` fold
//! (`#foldprompt`).
//!
//! Every parent→child edge in a runnable list component (`agent:run` and its
//! `agent:queue` / `agent:loop` aliases) carries an operator, given by the
//! parent row's suffix ([`EdgeOp`]):
//!
//! * **default** (no suffix, joined `: `): the parent is context and every
//!   child is its own leaf. Today's behaviour.
//! * **` |` pipe** (joined ` | `): the row's result flows into its children,
//!   like a YAML block scalar and a shell pipe at once, in reading order.
//! * **` >` fold**: the row and its whole child list are ONE task; the list
//!   is the row's context.
//!
//! ```text
//! - Collect the open agent-doc GitHub issues |
//!   - group them by component |
//!     - draft a triage summary |
//!       - under 200 words
//!       - no internal ids
//! ```
//!
//! The pipe grammar is strict: a row's children belong to the statement only
//! when that row ends in ` |` (or ` >`, which folds its own list into the
//! stage). Inside a statement, a row with children and no operator makes the
//! statement malformed. The queue enumerators then keep today's leaf-only
//! shape and the lint reports the row.
//!
//! A statement or fold is ONE structured prompt with one queue head, its top
//! row. That row alone carries the q-id, claim, strike and response tree.
//!
//! This module owns the text-level contract every layer shares:
//!
//! * [`edge_marker`] finds the operator on one row, after binary-owned
//!   suffixes and never inside a code span;
//! * [`strip_edge_marker`] removes it from a head's first line, so adding or
//!   removing an operator never changes a row's work identity;
//! * [`statement_rows`] reads a folded block back as rows, and
//!   [`statement_pipeline`] renders a pipe statement as one pipeline joined
//!   with [`PIPE_PATH_JOIN`]. That function alone decides how children
//!   combine.
//!
//! Folding (which source lines a block owns) lives with the queue
//! enumerators: `agent_doc_markdown_ast::overlay::edge_block_end`.

use std::ops::Range;

use crate::subprompts::{line_indent_width, list_item_content};

/// The authored pipe operator: a space and a pipe, as the row's last token.
pub const PIPE_MARKER: &str = " |";

/// The authored fold operator: a space and `>`, as the row's last token.
pub const FOLD_MARKER: &str = " >";

/// Join between a statement row and its child in a rendered path. Above the
/// statement, the ordinary nest-vector join `: ` still applies.
pub const PIPE_PATH_JOIN: &str = " | ";

/// The operator on a parent→child edge, given by the parent row's suffix.
///
/// The row-final operator is parsed through this registry, so a later
/// operator ([`RESERVED_EDGE_SUFFIXES`]) only means a new variant here plus
/// its [`EdgeOp::marker`] and [`EdgeOp::join`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeOp {
    /// Default: the parent is context and each child is its own leaf. Paths
    /// join with `: `.
    Context,
    /// Explicit ` |`: the parent's result flows into its children. Paths join
    /// with ` | `.
    Pipe,
    /// Explicit ` >`: the row and its whole child list are one task. Children
    /// are not addressable, so the join only labels lint paths.
    Fold,
}

impl EdgeOp {
    /// Every explicit operator, in parse order.
    pub const EXPLICIT: [EdgeOp; 2] = [EdgeOp::Pipe, EdgeOp::Fold];

    /// The operator a row's suffix gives the edges to its children.
    pub fn of_row(row: &str) -> Self {
        edge_marker(row).map_or(Self::Context, |(op, _)| op)
    }

    /// The row-final spelling, `None` for the default.
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Self::Context => None,
            Self::Pipe => Some(PIPE_MARKER),
            Self::Fold => Some(FOLD_MARKER),
        }
    }

    /// Join between a parent and a child in a rendered path.
    pub fn join(self) -> &'static str {
        match self {
            Self::Context => ": ",
            Self::Pipe => PIPE_PATH_JOIN,
            Self::Fold => " > ",
        }
    }
}

/// Row-final suffixes reserved for future edge operators: ` ||` (probably a
/// shell-style fallback), ` |+` (the deep pipe, whose descendants pipe without
/// each needing ` |`), ` |>` (the hand-off pipe, whose next stage runs as a
/// fresh worker), and the near-miss ` |*`. None is implemented yet. They are
/// neither prose nor an operator; the lint reports them as reserved
/// ([`reserved_edge_hint`]).
pub const RESERVED_EDGE_SUFFIXES: [&str; 4] = [" ||", " |+", " |*", " |>"];

/// What the lint says about a reserved suffix ([`RESERVED_EDGE_SUFFIXES`]).
pub fn reserved_edge_hint(suffix: &str) -> &'static str {
    match suffix {
        " |+" => "reserved for the deep pipe, which is not implemented yet",
        " |*" => "reserved; did you mean ` |+` (deep pipe)?",
        " |>" => "reserved for the hand-off pipe, which is not implemented yet; use ` |` to pipe within one task",
        _ => "reserved for a future edge operator",
    }
}

/// The reserved edge suffix `row` ends in, if any ([`RESERVED_EDGE_SUFFIXES`]).
pub fn reserved_edge_suffix(row: &str) -> Option<&'static str> {
    let first = row.lines().next()?;
    let authored = authored_prefix(first);
    RESERVED_EDGE_SUFFIXES.into_iter().find(|suffix| {
        authored.strip_suffix(suffix).is_some_and(|body| {
            !body.trim().is_empty()
                && !code_span_ranges(authored)
                    .iter()
                    .any(|span| span.contains(&(body.len() + 1)))
        })
    })
}

/// The explicit operator on `row` (one row's content after its list marker,
/// any lifecycle prefix kept) and the byte range of its marker, or `None`.
///
/// The marker must be the last authored token. Trailing binary-owned suffixes
/// are skipped first: a report-state annotation, `<!-- … -->` markers,
/// `⟦…⟧` annotations, a generated `[#q-…]` id, and a strike wrapper
/// (`~~row |~~`, `~~row |~~ — auto-struck: …`). A marker inside a code span is
/// literal text, and a ` | ` in the middle of a row is never a marker.
pub fn edge_marker(row: &str) -> Option<(EdgeOp, Range<usize>)> {
    let first = row.lines().next()?;
    let authored = authored_prefix(first);
    EdgeOp::EXPLICIT.into_iter().find_map(|op| {
        let marker = op.marker()?;
        let body = authored.strip_suffix(marker)?;
        if body.trim().is_empty()
            || body.ends_with(['|', '>', '-', '='])
            || body.trim_start().starts_with('|')
        {
            return None;
        }
        let at = body.len();
        if code_span_ranges(authored)
            .iter()
            .any(|span| span.contains(&(at + 1)))
        {
            return None;
        }
        Some((op, at..at + marker.len()))
    })
}

/// Byte range of the ` |` marker on `row` ([`edge_marker`]), or `None`.
pub fn pipe_marker_range(row: &str) -> Option<Range<usize>> {
    edge_marker(row).and_then(|(op, range)| (op == EdgeOp::Pipe).then_some(range))
}

/// Whether `row` ends in the ` |` operator.
pub fn ends_with_pipe(row: &str) -> bool {
    pipe_marker_range(row).is_some()
}

/// Whether `row` ends in the ` >` operator.
pub fn ends_with_fold(row: &str) -> bool {
    EdgeOp::of_row(row) == EdgeOp::Fold
}

/// `text` without the explicit operator on its first line. Text without one
/// is returned unchanged. Later lines (a folded block's children) are kept.
pub fn strip_edge_marker(text: &str) -> String {
    let first_end = text.find('\n').unwrap_or(text.len());
    let Some((_, range)) = edge_marker(&text[..first_end]) else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..range.start]);
    out.push_str(&text[range.end..]);
    out
}

/// `line` with `op`'s marker before its binary-owned suffixes, replacing any
/// other explicit operator. [`EdgeOp::Context`] removes the operator.
pub fn with_edge_marker(line: &str, op: EdgeOp) -> String {
    if EdgeOp::of_row(line) == op {
        return line.to_string();
    }
    let line = strip_edge_marker(line);
    let Some(marker) = op.marker() else {
        return line;
    };
    let at = authored_prefix(&line).len();
    let mut out = String::with_capacity(line.len() + marker.len());
    out.push_str(&line[..at]);
    out.push_str(marker);
    out.push_str(&line[at..]);
    out
}

/// Whether `row` closes a code span whose content ends in ` |`
/// (`` run `ls |` ``): a pipe the author probably meant, which the queue reads
/// as literal code. Reported by the lint, never parsed as the operator.
pub fn pipe_marker_in_code_span(row: &str) -> bool {
    let Some(first) = row.lines().next() else {
        return false;
    };
    let authored = authored_prefix(first);
    code_span_ranges(authored).iter().any(|span| {
        span.end == authored.len() && {
            let run = authored[..span.end]
                .chars()
                .rev()
                .take_while(|ch| *ch == '`')
                .count();
            let inner = &authored[span.start + run..span.end - run];
            inner.trim_start().len() > 1 && inner.trim_end().ends_with(PIPE_MARKER)
        }
    })
}

/// One row of a folded block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementRow {
    /// Indentation width of the row's list marker. The head row reports 0;
    /// only relative depth matters.
    pub indent: usize,
    /// The row's work text: projection prefix, generated id and the operator
    /// removed, continuation lines joined with `\n`.
    pub text: String,
    /// The operator the row's suffix gives its children.
    pub op: EdgeOp,
    /// Whether a deeper row follows before the list returns to this depth.
    pub has_children: bool,
}

/// The rows of a folded block: its head line, then every list row of its
/// block, in reading order. Continuation lines join their row.
pub fn statement_rows(text: &str) -> Vec<StatementRow> {
    let mut rows: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            rows.push((0, line.to_string()));
            continue;
        }
        match list_item_content(line) {
            // A `%% … %%` list comment is an annotation, not a stage.
            Some(content) if is_list_comment(content) => {}
            Some(content) if line_indent_width(line) > 0 => {
                rows.push((line_indent_width(line), content.to_string()));
            }
            _ if line.trim().is_empty() => {}
            _ => {
                if let Some((_, last)) = rows.last_mut() {
                    last.push('\n');
                    last.push_str(line.trim());
                }
            }
        }
    }
    let count = rows.len();
    (0..count)
        .map(|index| {
            let (indent, raw) = &rows[index];
            let has_children = index == 0 && count > 1
                || rows
                    .get(index + 1)
                    .is_some_and(|(next, _)| index > 0 && next > indent);
            StatementRow {
                indent: *indent,
                text: row_work_text(raw),
                op: EdgeOp::of_row(raw),
                has_children,
            }
        })
        .collect()
}

/// A pipe statement as one pipeline: every row in document order joined with
/// [`PIPE_PATH_JOIN`], like a vertical `a | b | c`.
///
/// This is the one place that decides how a ` |` row's children combine
/// (`#pipeprompt` v1 execution rule): they CHAIN in document order, so
/// `draft a triage summary |` over `under 200 words` and `no internal ids`
/// reads `… draft a triage summary | under 200 words | no internal ids`, and
/// the whole statement is ONE task. A fan-out rule (one path per leaf) would
/// change only this function and the submission that renders it.
pub fn statement_pipeline(text: &str) -> String {
    statement_rows(text)
        .into_iter()
        .map(|row| row.text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(PIPE_PATH_JOIN)
}

/// Rows of a pipe statement that have children but no operator. A statement
/// with such a row is malformed under the strict grammar. Rows inside a
/// ` >` fold are that fold's context and need no operator.
pub fn unpiped_parent_rows(text: &str) -> Vec<StatementRow> {
    let mut out = Vec::new();
    let mut fold_depth: Option<usize> = None;
    for (index, row) in statement_rows(text).into_iter().enumerate() {
        let depth = if index == 0 { 0 } else { row.indent };
        if fold_depth.is_some_and(|at| depth > at) {
            continue;
        }
        fold_depth = None;
        match row.op {
            EdgeOp::Fold => fold_depth = Some(depth),
            EdgeOp::Context if row.has_children => out.push(row),
            _ => {}
        }
    }
    out
}

fn is_list_comment(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.len() >= 4 && trimmed.starts_with("%%") && trimmed.ends_with("%%")
}

/// One row's work text: projection prefix, generated id and the explicit
/// operator removed.
pub fn row_work_text(raw: &str) -> String {
    let work = crate::strip_attempt_projection_prefix(raw).trim();
    let work = strip_edge_marker(work);
    let work = crate::strip_generated_queue_prompt_id_annotation(&work);
    let work = strip_edge_marker(work.trim());
    work.trim().to_string()
}

/// The authored prefix of one line: binary-owned trailing suffixes and a
/// strike wrapper removed, until nothing more changes. The result is always a
/// prefix of `line`, so byte offsets into it are offsets into `line`.
fn authored_prefix(line: &str) -> &str {
    let mut s = line.trim_end();
    loop {
        let before = s.len();
        s = crate::strip_queue_report_state_annotation(s).trim_end();
        if s.ends_with("-->")
            && let Some(open) = s.rfind("<!--")
        {
            s = s[..open].trim_end();
        }
        if s.ends_with('⟧')
            && let Some(open) = s.rfind('⟦')
        {
            s = s[..open].trim_end();
        }
        if let Some((work, id)) = s.rsplit_once(" [#")
            && let Some(id) = id.strip_suffix(']')
            && id.len() > 2
            && id[..2].eq_ignore_ascii_case("q-")
            && id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            s = work.trim_end();
        }
        if let Some(at) = s.find("~~ — auto-struck: ")
            && s[..at].contains("~~")
        {
            s = &s[..at + 2];
        }
        if let Some(inner) = s.strip_suffix("~~")
            && inner.contains("~~")
        {
            s = inner.trim_end();
        }
        if s.len() == before {
            return s;
        }
    }
}

/// Byte ranges of the code spans on one line, delimiters included. A run of
/// N backticks opens a span closed by the next run of exactly N backticks; an
/// unmatched run is literal text.
fn code_span_ranges(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let run_at = |at: usize| bytes[at..].iter().take_while(|b| **b == b'`').count();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = run_at(i);
        let mut j = i + open;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run = run_at(j);
                if run == open {
                    closed = Some(j + run);
                    break;
                }
                j += run;
            } else {
                j += 1;
            }
        }
        match closed {
            Some(end) => {
                spans.push(i..end);
                i = end;
            }
            None => i += open,
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_the_last_authored_token_only() {
        for row in [
            "Collect the open issues |",
            "Collect the open issues | [#q-0123456789ab]",
            "🚧 [#subagent:x#1] Collect the open issues | [#q-0123456789ab]",
            "Collect |  ⟦effect:2⟧",
            "Collect | <!-- note -->",
            "~~Collect |~~",
            "~~Collect | [#q-0123456789ab]~~",
            "~~Collect |~~ — auto-struck: done",
            "#review |",
        ] {
            assert!(ends_with_pipe(row), "{row}");
        }
        for row in [
            "Collect the open issues",
            "pending | completed",
            "a | b in the middle",
            "Collect|",
            "Collect ||",
            "Collect |+",
            "Collect |*",
            "Collect |>",
            "|",
            " |",
            "| a | b |",
            "run `ls |`",
            "see [docs](https://x \"a |\")",
        ] {
            assert!(!ends_with_pipe(row), "{row}");
            assert!(!ends_with_fold(row), "{row}");
        }
        // An unmatched backtick is literal text, so the marker still counts.
        assert!(ends_with_pipe("a ` b |"));
        for row in [
            "Summarize the thread >",
            "Summarize >  [#q-0123456789ab]",
            "~~Summarize >~~",
        ] {
            assert!(ends_with_fold(row), "{row}");
        }
        for row in ["a -> b", "a => b", "a >> b", "run `cat >`", "a > b c", ">"] {
            assert!(!ends_with_fold(row), "{row}");
        }
    }

    #[test]
    fn strip_and_insert_keep_binary_suffixes() {
        assert_eq!(
            strip_edge_marker("Collect | [#q-0123456789ab]\n  - child |"),
            "Collect [#q-0123456789ab]\n  - child |"
        );
        assert_eq!(
            strip_edge_marker("Summarize > [#q-0123456789ab]"),
            "Summarize [#q-0123456789ab]"
        );
        assert_eq!(
            strip_edge_marker("pending | completed"),
            "pending | completed"
        );
        assert_eq!(
            with_edge_marker("Collect [#q-0123456789ab]", EdgeOp::Pipe),
            "Collect | [#q-0123456789ab]"
        );
        assert_eq!(with_edge_marker("Collect |", EdgeOp::Pipe), "Collect |");
        assert_eq!(with_edge_marker("Collect |", EdgeOp::Fold), "Collect >");
        assert_eq!(with_edge_marker("Collect >", EdgeOp::Context), "Collect");
    }

    #[test]
    fn edge_operator_registry_reads_the_row_suffix() {
        assert_eq!(EdgeOp::of_row("Collect |"), EdgeOp::Pipe);
        assert_eq!(EdgeOp::of_row("Collect >"), EdgeOp::Fold);
        assert_eq!(EdgeOp::of_row("Collect"), EdgeOp::Context);
        assert_eq!(EdgeOp::of_row("Collect ||"), EdgeOp::Context);
        assert_eq!(EdgeOp::Pipe.join(), " | ");
        assert_eq!(EdgeOp::Context.join(), ": ");
        assert_eq!(reserved_edge_suffix("Collect ||"), Some(" ||"));
        assert_eq!(reserved_edge_suffix("Collect |+"), Some(" |+"));
        assert_eq!(
            reserved_edge_suffix("Collect |* [#q-0123456789ab]"),
            Some(" |*")
        );
        assert_eq!(reserved_edge_suffix("Collect |>"), Some(" |>"));
        assert_eq!(reserved_edge_suffix("Collect |"), None);
        assert_eq!(reserved_edge_suffix("Collect >"), None);
        assert_eq!(reserved_edge_suffix("a -> b"), None);
        assert_eq!(reserved_edge_suffix("run `a ||`"), None);
        assert!(reserved_edge_hint(" |*").contains("` |+`"));
        assert!(reserved_edge_hint(" |>").contains("hand-off pipe"));
        assert!(!reserved_edge_hint(" ||").contains("did you mean"));
    }

    #[test]
    fn code_span_pipe_is_reported_not_parsed() {
        assert!(pipe_marker_in_code_span("run `ls |`"));
        assert!(pipe_marker_in_code_span("run `ls |` [#q-0123456789ab]"));
        assert!(!pipe_marker_in_code_span("run `ls` |"));
        assert!(!pipe_marker_in_code_span("run `|`"));
        assert!(!pipe_marker_in_code_span("run `a | b`"));
    }

    #[test]
    fn statement_chains_its_rows_in_document_order() {
        let text = "Collect the open issues | [#q-0123456789ab]\n  - group them by component |\n    - draft a triage summary |\n      - under 200 words\n      - no internal ids";
        assert_eq!(
            statement_pipeline(text),
            "Collect the open issues | group them by component | draft a triage summary | under 200 words | no internal ids",
        );
        assert!(unpiped_parent_rows(text).is_empty());
        let rows = statement_rows(text);
        assert_eq!(rows.len(), 5);
        assert!(rows[0].op == EdgeOp::Pipe && rows[0].has_children);
        assert!(!rows[3].has_children && rows[3].op == EdgeOp::Context);
    }

    #[test]
    fn unpiped_inner_parent_is_reported() {
        let text = "Collect |\n  - group them\n    - by component";
        let rows = unpiped_parent_rows(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "group them");
        // A ` >` stage folds its own list; rows under it need no operator.
        let folded = "Collect |\n  - summarize >\n    - context\n      - more context";
        assert!(unpiped_parent_rows(folded).is_empty());
    }
}
