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
//! * **` |+` deep pipe** (joined ` | `): every descendant edge pipes too; the
//!   whole subtree flattens in document order into one pipeline.
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
//!   [`statement_pipelines`] renders a pipe statement as its pipelines joined
//!   with [`PIPE_PATH_JOIN`]. That function alone decides how children
//!   combine (ordered lists chain, unordered lists fan out, ` |+` chains its
//!   whole subtree).
//!
//! Folding (which source lines a block owns) lives with the queue
//! enumerators: `agent_doc_markdown_ast::overlay::edge_block_end`.

use std::ops::Range;

use crate::subprompts::{line_indent_width, list_item_content};

/// The authored pipe operator: a space and a pipe, as the row's last token.
pub const PIPE_MARKER: &str = " |";

/// The authored fold operator: a space and `>`, as the row's last token.
pub const FOLD_MARKER: &str = " >";

/// The authored deep-pipe operator: every descendant edge pipes too.
pub const DEEP_PIPE_MARKER: &str = " |+";

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
    /// Explicit ` |+`: a deep pipe. Every descendant edge pipes too, so the
    /// whole subtree flattens into one pipeline; descendants need no ` |`.
    DeepPipe,
    /// Explicit ` >`: the row and its whole child list are one task. Children
    /// are not addressable, so the join only labels lint paths.
    Fold,
}

impl EdgeOp {
    /// Every explicit operator, in parse order.
    pub const EXPLICIT: [EdgeOp; 3] = [EdgeOp::Pipe, EdgeOp::DeepPipe, EdgeOp::Fold];

    /// The operator a row's suffix gives the edges to its children.
    pub fn of_row(row: &str) -> Self {
        edge_marker(row).map_or(Self::Context, |(op, _)| op)
    }

    /// The row-final spelling, `None` for the default.
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Self::Context => None,
            Self::Pipe => Some(PIPE_MARKER),
            Self::DeepPipe => Some(DEEP_PIPE_MARKER),
            Self::Fold => Some(FOLD_MARKER),
        }
    }

    /// Join between a parent and a child in a rendered path.
    pub fn join(self) -> &'static str {
        match self {
            Self::Context => ": ",
            Self::Pipe | Self::DeepPipe => PIPE_PATH_JOIN,
            Self::Fold => " > ",
        }
    }

    /// Whether the operator pipes (` |` or ` |+`).
    pub fn pipes(self) -> bool {
        matches!(self, Self::Pipe | Self::DeepPipe)
    }

    /// Whether every descendant of the row belongs to its block without its
    /// own operator (` |+`, ` >`).
    pub fn covers_subtree(self) -> bool {
        matches!(self, Self::DeepPipe | Self::Fold)
    }
}

/// Row-final suffixes reserved for future edge operators: ` ||` (probably a
/// shell-style fallback) and ` |>` (the hand-off pipe, whose next stage runs
/// as a fresh worker), plus the near-miss ` |*`. None is implemented yet.
/// They are neither prose nor an operator; the lint reports them as reserved
/// ([`reserved_edge_hint`]).
pub const RESERVED_EDGE_SUFFIXES: [&str; 3] = [" ||", " |*", " |>"];

/// What the lint says about a reserved suffix ([`RESERVED_EDGE_SUFFIXES`]).
pub fn reserved_edge_hint(suffix: &str) -> &'static str {
    match suffix {
        " |*" => "reserved; did you mean ` |+` (deep pipe)?",
        " |>" => {
            "reserved for the hand-off pipe, which is not implemented yet; use ` |` to pipe within one task"
        }
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
    /// Whether the row's own list marker is ordered (`1.`, `N)`).
    pub ordered: bool,
}

/// The rows of a folded block: its head line, then every list row of its
/// block, in reading order. Continuation lines join their row.
pub fn statement_rows(text: &str) -> Vec<StatementRow> {
    let mut rows: Vec<(usize, String, bool)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            rows.push((0, line.to_string(), false));
            continue;
        }
        match list_item_content(line) {
            // A `%% … %%` list comment is an annotation, not a stage.
            Some(content) if is_list_comment(content) => {}
            Some(content) if line_indent_width(line) > 0 => {
                let ordered = crate::ordered_list_marker_len(line.trim_start()).is_some();
                rows.push((line_indent_width(line), content.to_string(), ordered));
            }
            _ if line.trim().is_empty() => {}
            _ => {
                if let Some((_, last, _)) = rows.last_mut() {
                    last.push('\n');
                    last.push_str(line.trim());
                }
            }
        }
    }
    let count = rows.len();
    (0..count)
        .map(|index| {
            let (indent, raw, ordered) = &rows[index];
            let has_children = index == 0 && count > 1
                || rows
                    .get(index + 1)
                    .is_some_and(|(next, _, _)| index > 0 && next > indent);
            StatementRow {
                indent: *indent,
                text: row_work_text(raw),
                op: EdgeOp::of_row(raw),
                has_children,
                ordered: *ordered,
            }
        })
        .collect()
}

/// A pipe statement as its pipelines, each joined with [`PIPE_PATH_JOIN`].
///
/// This is the one place that decides how a row's children combine
/// (`#pipeprompt` combination rule, operator decision 2026-11-24):
///
/// * under ` |`, an **ordered** child list (`1.`, `N)`) CHAINS in numbered
///   order, each step taking the previous step's result
///   (`draft | 1st | 2nd`), while an **unordered** list (`- `) FANS OUT:
///   each child takes the parent's result independently, one pipeline per
///   branch (`draft | under 200 words`, `draft | no internal ids`);
/// * ` |+` overrides the list type: its whole subtree flattens in document
///   order into one chain;
/// * a ` >` row inside a statement is one stage whose list is its context;
/// * a row without an operator ends its branch.
///
/// Every pipeline still runs inside the ONE task; fan-out is framed as
/// "apply each of these to the parent's result". The phase-2 ` |>` rule
/// (sequential hand-offs for ordered children, parallel workers for
/// unordered ones) will live here too.
pub fn statement_pipelines(text: &str) -> Vec<String> {
    let rows = statement_rows(text);
    if rows.is_empty() {
        return Vec::new();
    }
    // Children of each row: the next rows one level deeper.
    let depth = |index: usize| if index == 0 { 0 } else { rows[index].indent };
    let children = |index: usize| -> Vec<usize> {
        let own = depth(index);
        let mut out = Vec::new();
        let mut child_depth = None;
        for later in index + 1..rows.len() {
            let at = depth(later);
            if at <= own {
                break;
            }
            match child_depth {
                None => {
                    child_depth = Some(at);
                    out.push(later);
                }
                Some(expected) if at <= expected => {
                    out.push(later);
                    child_depth = Some(at);
                }
                _ => {}
            }
        }
        out
    };
    fn subtree(rows: &[StatementRow], index: usize, depth: &dyn Fn(usize) -> usize) -> Vec<usize> {
        let own = depth(index);
        let mut out = vec![index];
        out.extend((index + 1..rows.len()).take_while(|&later| depth(later) > own));
        out
    }
    fn render(
        rows: &[StatementRow],
        index: usize,
        depth: &dyn Fn(usize) -> usize,
        children: &dyn Fn(usize) -> Vec<usize>,
    ) -> Vec<String> {
        let row = &rows[index];
        let kids = children(index);
        if kids.is_empty() || !row.op.pipes() {
            return vec![row.text.clone()];
        }
        if row.op == EdgeOp::DeepPipe {
            return vec![
                subtree(rows, index, depth)
                    .into_iter()
                    .map(|at| rows[at].text.as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(PIPE_PATH_JOIN),
            ];
        }
        if kids.iter().all(|&kid| rows[kid].ordered) {
            // Chain: each step takes the previous step's result.
            let mut paths = vec![row.text.clone()];
            for kid in kids {
                let steps = render(rows, kid, depth, children);
                paths = paths
                    .iter()
                    .flat_map(|path| {
                        steps
                            .iter()
                            .map(move |step| format!("{path}{PIPE_PATH_JOIN}{step}"))
                    })
                    .collect();
            }
            return paths;
        }
        // Fan out: each child takes the parent's result independently.
        kids.into_iter()
            .flat_map(|kid| render(rows, kid, depth, children))
            .map(|branch| format!("{}{PIPE_PATH_JOIN}{branch}", row.text))
            .collect()
    }
    render(&rows, 0, &depth, &children)
}

/// Rows of a pipe statement that have children but no operator. A statement
/// with such a row is malformed under the strict grammar. Rows inside a
/// ` >` fold or below a ` |+` deep pipe need no operator of their own.
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
            EdgeOp::Fold | EdgeOp::DeepPipe => fold_depth = Some(depth),
            EdgeOp::Context if row.has_children => out.push(row),
            _ => {}
        }
    }
    out
}

/// Binary-owned stage glyph on the statement row a worker is running
/// (`#pipeprompt` stage indicator).
pub const STAGE_RUNNING_GLYPH: &str = "▶️";

/// Binary-owned stage glyph on a statement row whose stage finished.
pub const STAGE_DONE_GLYPH: &str = "✅";

/// `row` (one row's content after its list marker) without a leading stage
/// glyph. Stage glyphs are bookkeeping, never work text.
pub fn strip_stage_glyph(row: &str) -> &str {
    let trimmed = row.trim_start();
    for glyph in [STAGE_RUNNING_GLYPH, STAGE_DONE_GLYPH] {
        if let Some(rest) = trimmed.strip_prefix(glyph)
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            return rest.trim_start();
        }
    }
    row
}

/// A folded statement's text with the stage glyph of every child list row
/// repainted for `stage` (1-based over [`statement_rows`]; the head is stage
/// 1). Rows before the stage show [`STAGE_DONE_GLYPH`], the stage row shows
/// [`STAGE_RUNNING_GLYPH`], later rows none. The head row is never painted:
/// its own lifecycle glyph already says it runs. `None` strips every stage
/// glyph (the statement is not running). Every other byte is kept.
pub fn project_stage_glyphs(text: &str, stage: Option<usize>) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut ordinal = 1usize;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\n', '\r']);
        let content = (index > 0 && line_indent_width(body) > 0)
            .then(|| list_item_content(body))
            .flatten()
            .filter(|content| !content.trim().is_empty() && !is_list_comment(content));
        let Some(content) = content else {
            out.push_str(line);
            continue;
        };
        ordinal += 1;
        let marker_len = body.len() - content.len();
        let work = strip_stage_glyph(content);
        let glyph = match stage {
            Some(stage) if ordinal < stage => Some(STAGE_DONE_GLYPH),
            Some(stage) if ordinal == stage => Some(STAGE_RUNNING_GLYPH),
            _ => None,
        };
        out.push_str(&body[..marker_len]);
        if let Some(glyph) = glyph {
            out.push_str(glyph);
            out.push(' ');
        }
        out.push_str(work);
        out.push_str(&line[body.len()..]);
    }
    out
}

/// `text` with every child row's stage glyph removed ([`project_stage_glyphs`]
/// with no stage): the form identity and edit comparisons read.
pub fn strip_stage_glyphs(text: &str) -> String {
    project_stage_glyphs(text, None)
}

fn is_list_comment(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.len() >= 4 && trimmed.starts_with("%%") && trimmed.ends_with("%%")
}

/// One row's work text: projection prefix, generated id and the explicit
/// operator removed.
pub fn row_work_text(raw: &str) -> String {
    let work = crate::strip_attempt_projection_prefix(strip_stage_glyph(raw)).trim();
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
        assert_eq!(reserved_edge_suffix("Collect |+"), None);
        assert_eq!(EdgeOp::of_row("Collect |+"), EdgeOp::DeepPipe);
        assert_eq!(
            EdgeOp::of_row("Collect |+ [#q-0123456789ab]"),
            EdgeOp::DeepPipe
        );
        assert_eq!(EdgeOp::DeepPipe.join(), " | ");
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
            statement_pipelines(text),
            vec![
                "Collect the open issues | group them by component | draft a triage summary | under 200 words",
                "Collect the open issues | group them by component | draft a triage summary | no internal ids",
            ],
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
        // A ` |+` deep pipe declares its whole subtree piped.
        let deep = "Collect |+\n  - group\n    - draft\n      - under 200 words";
        assert!(unpiped_parent_rows(deep).is_empty());
        assert_eq!(
            statement_pipelines(deep),
            vec!["Collect | group | draft | under 200 words"]
        );
    }

    #[test]
    fn stage_glyphs_paint_children_and_strip_cleanly() {
        let text =
            "Collect |\n  - group |\n    - draft |\n      - under 200 words\n      - %% note %%";
        let painted = project_stage_glyphs(text, Some(3));
        assert_eq!(
            painted,
            "Collect |\n  - ✅ group |\n    - ▶️ draft |\n      - under 200 words\n      - %% note %%"
        );
        assert_eq!(project_stage_glyphs(&painted, Some(3)), painted);
        assert_eq!(strip_stage_glyphs(&painted), text);
        assert_eq!(project_stage_glyphs(text, Some(1)), text);
        assert_eq!(
            statement_pipelines(&painted),
            vec!["Collect | group | draft | under 200 words"]
        );
        assert_eq!(strip_stage_glyph("▶️ draft |"), "draft |");
        assert_eq!(strip_stage_glyph("▶️draft"), "▶️draft");
    }

    /// The combination rule, one case per cell.
    #[test]
    fn ordered_children_chain_unordered_children_fan_out() {
        // ` |` + ordered list: one chain in numbered order.
        assert_eq!(
            statement_pipelines("draft |\n  1. outline\n  2. write\n  3. trim"),
            vec!["draft | outline | write | trim"]
        );
        // ` |` + unordered list: each child takes the parent's result.
        assert_eq!(
            statement_pipelines("draft |\n  - under 200 words\n  - no internal ids"),
            vec!["draft | under 200 words", "draft | no internal ids"]
        );
        // ` |+` + unordered list: the whole subtree chains anyway.
        assert_eq!(
            statement_pipelines("draft |+\n  - outline\n    - sections\n  - trim"),
            vec!["draft | outline | sections | trim"]
        );
        // A chained step that itself fans out multiplies the branches.
        assert_eq!(
            statement_pipelines(
                "collect |\n  1. group |\n    - by component\n    - by age\n  2. summarize"
            ),
            vec![
                "collect | group | by component | summarize",
                "collect | group | by age | summarize",
            ]
        );
        // A ` >` stage keeps its list as its own context.
        assert_eq!(
            statement_pipelines("collect |\n  - summarize >\n    - focus on CI"),
            vec!["collect | summarize"]
        );
    }
}
