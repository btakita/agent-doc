//! Single-line queue tree edit (`#patchruncomponent`, `agent-doc queue edit`).
//!
//! `agent-doc patch FILE run --id main` replaces a whole queue body. The
//! coordinator often needs less: replace one stale `%%` note under a row, fix
//! one row's text, or drop one note line, while the operator keeps typing in
//! the same queue. This module plans exactly that edit from one document cut:
//!
//! - **Selection.** `--line N` names a 1-based document line; `--row ID` names
//!   the row whose own id (`do [#ID]`, `[#ID]`, or its generated `[#q-…]`) is
//!   `ID`; `--find TEXT` names the one line whose text equals `TEXT`, inside
//!   `--row`'s subtree when both are given. Every selection must resolve to
//!   exactly one line strictly inside a queue body (`agent:run`, `agent:queue`,
//!   or `agent:loop`), optionally limited to one queue by `--id`.
//! - **Guard.** `--expect TEXT` refuses unless the selected line still reads
//!   `TEXT`, so a line number taken from an earlier read cannot silently edit
//!   a different line after the operator inserted rows above it.
//! - **Edit.** A replacement keeps the line's indentation and list marker and
//!   replaces the rest; a delete removes the one line and refuses when the
//!   line has nested children (that is `queue delete-tree`'s job).
//! - **Validation.** The successor must parse, keep the same component
//!   sequence, and keep a valid queue graph, so an edit can never open, close,
//!   or rename a component or break an `after=` edge.
//!
//! Text is compared after trimming and dropping one leading list marker, so
//! `- %% note %%` and `%% note %%` name the same line.
//!
//! This module is pure; `agent_doc_queue_io::queue_row_edit` holds the
//! queue-edit lease, reads the authority cut, guards claimed rows, and submits
//! the successor through the editor/CRDT compare-and-swap path.

use agent_doc_element::element;
use agent_doc_element_queue::QueueItemIdentity;
use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::queue_graph::QueueGraph;

/// Which line to edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowEditSelector {
    /// 1-based document line.
    Line(usize),
    /// The row whose own id is this (`q-…`, `#q-…`, or `[#q-…]` accepted).
    Row(String),
    /// The one line whose text equals this, optionally inside a row's subtree.
    Find { text: String, under: Option<String> },
}

/// What to do with the selected line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowEditOp {
    /// Replace the line's text after its indentation and list marker.
    Replace(String),
    /// Remove the line; refused when it has nested children.
    Delete,
}

/// A planned single-line queue edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowEditPlan {
    /// Canonical id of the queue holding the line.
    pub queue: String,
    /// 1-based document line that was selected.
    pub line: usize,
    /// The line as it read in the planning cut (no line ending).
    pub before: String,
    /// The line as it will read (`None` for a delete).
    pub after: Option<String>,
    /// `false` when the replacement equals the current line.
    pub changed: bool,
    /// The complete successor document.
    #[serde(skip)]
    pub document: String,
}

/// One line of a queue body with its location.
#[derive(Debug, Clone)]
struct BodyLine {
    queue: String,
    /// 1-based document line.
    number: usize,
    /// Byte range of the line, line ending excluded.
    start: usize,
    end: usize,
    /// Byte length of the line ending (`0`, `1`, or `2`).
    eol: usize,
}

impl BodyLine {
    fn text<'a>(&self, document: &'a str) -> &'a str {
        &document[self.start..self.end]
    }
}

/// Split a line into `(indent, list marker incl. trailing space, rest)`.
fn split_line(line: &str) -> (&str, &str, &str) {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (indent, rest) = line.split_at(indent_len);
    let marker_len = list_marker_len(rest);
    let (marker, rest) = rest.split_at(marker_len);
    (indent, marker, rest)
}

/// Byte length of a leading list marker and its following whitespace
/// (`- `, `* `, `+ `, `12. `, `3) `), or `0`.
fn list_marker_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let marker_end = match bytes.first() {
        Some(b'-' | b'*' | b'+') => 1,
        Some(b'0'..=b'9') => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            match bytes.get(digits) {
                Some(b'.' | b')') if digits <= 9 => digits + 1,
                _ => return 0,
            }
        }
        _ => return 0,
    };
    match bytes.get(marker_end) {
        Some(b' ' | b'\t') => {
            let spaces = bytes[marker_end..]
                .iter()
                .take_while(|b| matches!(b, b' ' | b'\t'))
                .count();
            marker_end + spaces
        }
        // A bare marker line (`-`) is an empty list item.
        None => marker_end,
        _ => 0,
    }
}

/// The comparison key for a line: trimmed, one list marker dropped.
fn line_key(text: &str) -> &str {
    let trimmed = text.trim();
    let (_, _, rest) = split_line(trimmed);
    rest.trim()
}

fn indent_width(text: &str) -> usize {
    text.chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Normalize an id argument: `q-1`, `#q-1`, and `[#q-1]` all name `q-1`.
fn normalize_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    trimmed
        .strip_prefix('#')
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

/// The work text of a row line: list marker and `###` heading dropped.
pub fn row_work_text(line: &str) -> &str {
    let key = line_key(line);
    let unheaded = key.trim_start_matches('#');
    if unheaded.len() != key.len() && unheaded.starts_with([' ', '\t']) {
        unheaded.trim_start()
    } else {
        key
    }
}

/// `true` when the row line's own id is `id` (already normalized).
fn row_has_own_id(line: &str, id: &str) -> bool {
    let work = row_work_text(line);
    if work.is_empty() {
        return false;
    }
    let generated = agent_doc_element_queue::generated_queue_prompt_id(work)
        .or_else(|| agent_doc_element_queue::struck_generated_prompt_id(work).map(str::to_string));
    if generated.is_some_and(|own| own.eq_ignore_ascii_case(id)) {
        return true;
    }
    let unstruck = work.trim_start_matches("~~").trim_end_matches("~~");
    matches!(
        agent_doc_element_queue::queue_head_identity(unstruck),
        QueueItemIdentity::Id(own) if own.eq_ignore_ascii_case(id)
    )
}

fn queue_body_lines(document: &str, queue_id: Option<&str>) -> Result<Vec<BodyLine>> {
    let components = element::parse(document)?;
    let graph = QueueGraph::parse(document, &components).map_err(anyhow::Error::new)?;
    let wanted = queue_id.map(|id| {
        graph
            .block(id)
            .map(|block| block.occurrence)
            .ok_or_else(|| anyhow!("queue id `{id}` not found"))
    });
    let wanted = wanted.transpose()?;
    let mut lines = Vec::new();
    for block in graph.blocks() {
        if wanted.is_some_and(|occurrence| occurrence != block.occurrence) {
            continue;
        }
        let body_start = block.component.open_end;
        let body_end = block.component.close_start;
        let mut offset = body_start;
        while offset < body_end {
            let rest = &document[offset..body_end];
            let (len, eol) = match rest.find('\n') {
                Some(at) if at > 0 && rest.as_bytes()[at - 1] == b'\r' => (at - 1, 2),
                Some(at) => (at, 1),
                None => (rest.len(), 0),
            };
            lines.push(BodyLine {
                queue: block.id.clone(),
                number: document[..offset].bytes().filter(|b| *b == b'\n').count() + 1,
                start: offset,
                end: offset + len,
                eol,
            });
            offset += len + eol;
        }
    }
    Ok(lines)
}

/// Index range `(first, last)` into `lines` of the subtree rooted at `root`:
/// following lines of the same queue indented deeper than the root. Blank
/// lines continue the subtree only when a deeper line follows them.
fn subtree_range(document: &str, lines: &[BodyLine], root: usize) -> (usize, usize) {
    let root_indent = indent_width(lines[root].text(document));
    let mut last = root;
    for (index, line) in lines.iter().enumerate().skip(root + 1) {
        if line.queue != lines[root].queue {
            break;
        }
        let text = line.text(document);
        if text.trim().is_empty() {
            continue;
        }
        if indent_width(text) <= root_indent {
            break;
        }
        last = index;
    }
    (root, last)
}

fn describe_candidates(document: &str, lines: &[BodyLine], hits: &[usize]) -> String {
    hits.iter()
        .take(5)
        .map(|index| {
            let line = &lines[*index];
            format!("L{} `{}`", line.number, line.text(document).trim())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn select(
    document: &str,
    lines: &[BodyLine],
    selector: &RowEditSelector,
    queue_id: Option<&str>,
) -> Result<usize> {
    let scope = queue_id
        .map(|id| format!("queue id `{id}`"))
        .unwrap_or_else(|| "any queue".to_string());
    let find_row = |id: &str| -> Result<usize> {
        let id = normalize_id(id);
        if id.is_empty() {
            bail!("`--row` needs an id");
        }
        let hits = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| row_has_own_id(line.text(document), &id))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => bail!("no row in {scope} has its own id `{id}`"),
            _ => bail!(
                "row id `{id}` names {} rows in {scope} ({}); select one with `--line N --expect TEXT`",
                hits.len(),
                describe_candidates(document, lines, &hits)
            ),
        }
    };
    match selector {
        RowEditSelector::Line(number) => lines
            .iter()
            .position(|line| line.number == *number)
            .ok_or_else(|| {
                anyhow!(
                    "line {number} is not inside the body of {scope} (queue component markers are not editable here; use `agent-doc queue attr` for marker attributes)"
                )
            }),
        RowEditSelector::Row(id) => find_row(id),
        RowEditSelector::Find { text, under } => {
            let key = line_key(text);
            if key.is_empty() {
                bail!("`--find` needs nonblank text");
            }
            let (first, last) = match under {
                Some(id) => {
                    let root = find_row(id)?;
                    subtree_range(document, lines, root)
                }
                None => (0, lines.len().saturating_sub(1)),
            };
            let hits = (first..=last.min(lines.len().saturating_sub(1)))
                .filter(|index| !lines.is_empty() && line_key(lines[*index].text(document)) == key)
                .collect::<Vec<_>>();
            let within = under
                .as_deref()
                .map(|id| format!("the subtree of row `{}`", normalize_id(id)))
                .unwrap_or(scope);
            match hits.as_slice() {
                [one] => Ok(*one),
                [] => bail!("no line in {within} reads `{key}`"),
                _ => bail!(
                    "{} lines in {within} read `{key}` ({}); select one with `--line N --expect TEXT`",
                    hits.len(),
                    describe_candidates(document, lines, &hits)
                ),
            }
        }
    }
}

/// Component identity used to prove an edit kept the component sequence.
fn component_sequence(document: &str) -> Result<Vec<(String, Option<String>)>> {
    Ok(element::parse(document)?
        .into_iter()
        .map(|component| {
            let id = component.attrs.get("id").cloned();
            (component.name, id)
        })
        .collect())
}

/// Plan one single-line edit inside a queue body. See the module docs.
pub fn plan_row_edit(
    document: &str,
    queue_id: Option<&str>,
    selector: &RowEditSelector,
    expect: Option<&str>,
    op: &RowEditOp,
) -> Result<RowEditPlan> {
    if let RowEditOp::Replace(text) = op {
        if text.contains(['\n', '\r']) {
            bail!(
                "the replacement must be one line; edit one line per call (or use `agent-doc queue follow-up` to add rows)"
            );
        }
        if text.trim().is_empty() {
            bail!("the replacement text is blank; use `--delete` to remove the line");
        }
    }
    let lines = queue_body_lines(document, queue_id)?;
    let index = select(document, &lines, selector, queue_id)?;
    let line = &lines[index];
    let before = line.text(document).to_string();
    if let Some(expected) = expect {
        let want = line_key(expected);
        if line_key(&before) != want {
            bail!(
                "line {} no longer reads `{want}`; it reads `{}`. Re-read the document and retry",
                line.number,
                before.trim()
            );
        }
    }

    let (after, successor) = match op {
        RowEditOp::Replace(text) => {
            let (indent, marker, _) = split_line(&before);
            let text = text.trim();
            // A replacement that brings its own list marker replaces the old
            // one; otherwise the line keeps its marker.
            let marker = if list_marker_len(text) > 0 {
                ""
            } else {
                marker
            };
            let after = format!("{indent}{marker}{text}");
            let mut successor = String::with_capacity(document.len() + after.len());
            successor.push_str(&document[..line.start]);
            successor.push_str(&after);
            successor.push_str(&document[line.end..]);
            (Some(after), successor)
        }
        RowEditOp::Delete => {
            let (_, last) = subtree_range(document, &lines, index);
            if last != index {
                bail!(
                    "line {} has {} nested line(s); deleting it would re-parent them. Use `agent-doc queue delete-tree --line {}` for a subtree",
                    line.number,
                    last - index,
                    line.number
                );
            }
            let mut successor = String::with_capacity(document.len());
            successor.push_str(&document[..line.start]);
            successor.push_str(&document[line.end + line.eol..]);
            (None, successor)
        }
    };
    let changed = successor != document;

    if changed {
        let before_components = component_sequence(document)?;
        let after_components = component_sequence(&successor)
            .map_err(|err| anyhow!("the edit breaks the document's components: {err}"))?;
        if before_components != after_components {
            bail!(
                "the edit would open, close, or rename a component; a queue line edit must not carry component markers"
            );
        }
        let components = element::parse(&successor)?;
        QueueGraph::parse(&successor, &components)
            .map_err(|err| anyhow!("the edit breaks the queue graph: {err}"))?;
    }

    Ok(RowEditPlan {
        queue: line.queue.clone(),
        line: line.number,
        before,
        after,
        changed,
        document: successor,
    })
}

/// The text report `agent-doc queue edit` prints.
pub fn render_row_edit_report(file: &str, plan: &RowEditPlan) -> String {
    match (&plan.after, plan.changed) {
        (_, false) => format!(
            "{file}: line {} in queue id `{}` already reads that; nothing changed.\n",
            plan.line, plan.queue
        ),
        (Some(after), true) => format!(
            "{file}: edited line {} in queue id `{}`\n- before: {}\n- after:  {}\n",
            plan.line,
            plan.queue,
            plan.before.trim(),
            after.trim()
        ),
        (None, true) => format!(
            "{file}: deleted line {} in queue id `{}`: {}\n",
            plan.line,
            plan.queue,
            plan.before.trim()
        ),
    }
}

#[cfg(test)]
#[path = "row_edit_tests.rs"]
mod tests;
