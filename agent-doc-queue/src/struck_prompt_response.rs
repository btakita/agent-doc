//! `#strucknoresponse`: a struck operator prompt always carries its answer.
//!
//! The operator reads a struck queue row as "the agent handled this". When a
//! pass strikes an operator prompt and writes nothing beneath it, a question
//! the operator typed reads as answered while nobody answered it. Live
//! 2026-11-22 (haiven-dev `tasks/sdk/sdk-dev.md`): the `#qftbklgstrike`
//! semantic strike buried an operator's follow-up question `Is this done?`
//! (typed under a 🙋 decision row) as "completed by" an unrelated done item
//! from another document, and the operator's decision answer `Apache-2.0`
//! (typed under a license question) as "completed by" an unrelated id whose
//! archived continuation happened to mention Apache-2.0. A third operator
//! question sat struck with no response at all, so the tree glyph pass offered
//! it for deletion.
//!
//! This module owns the three shared pieces:
//!
//! - [`is_operator_reply_shape`]: a question or a short decision answer. Such
//!   a row is a reply to someone, never a restatement of finished work, so no
//!   lexical completion match may strike it.
//! - [`abbreviated_strike_response_lines`]: the abbreviated response tree a
//!   binary strike writes under the row it strikes, so a strike is never bare.
//! - [`unanswered_struck_prompt_rows`]: struck rows already in a document that
//!   break the rule. They are reported, never rewritten: un-striking restores
//!   operator text from a guess about who struck it, and a struck row can be
//!   one the operator struck on purpose. A report names the row so the agent
//!   answers it or the operator un-strikes or deletes it.
//!
//! This module is pure: document text in, values out.

use crate::document_queue::{QueueEntry, QueuePrompt};

/// The `#qftbklgstrike` note baked inside a semantic strike.
pub const SEMANTIC_STRIKE_MARKER: &str = "(#qftbklgstrike)";

/// Fewer content words than this makes a row a short answer, not a task.
const MIN_TASK_CONTENT_WORDS: usize = 4;

/// Text of a queue row with the binary-owned decorations removed: attempt
/// projection, tree glyphs, the strike wrapper, a baked `— auto-struck:` note,
/// and trailing `[#…]` ids.
pub fn row_prose(text: &str) -> String {
    let mut body = agent_doc_element_queue::strip_attempt_projection_prefix(text.trim())
        .trim()
        .to_string();
    if let Some(inner) = body
        .strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
    {
        body = inner.trim().to_string();
    }
    for glyph in ["🗑\u{fe0f} ", "🗑 ", "🔒 "] {
        if let Some(rest) = body.strip_prefix(glyph) {
            body = rest.trim_start().to_string();
        }
    }
    if let Some(at) = body.find(" — auto-struck:") {
        body.truncate(at);
    }
    let mut body = body.trim_end().to_string();
    // Trailing reserved ids (`[#q-…]`, `[operator-verify]`) are not prose.
    loop {
        let trimmed = body.trim_end();
        let Some(open) = trimmed.rfind('[') else {
            break;
        };
        if !trimmed.ends_with(']') || trimmed[open..].contains(' ') {
            break;
        }
        body = trimmed[..open].trim_end().to_string();
    }
    body
}

/// Whether the row's prose asks a question.
pub fn is_question(text: &str) -> bool {
    row_prose(text).trim_end().ends_with('?')
}

/// Content words: alphanumeric runs of two or more characters.
fn content_words(text: &str) -> usize {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .count()
}

/// `#strucknoresponse`: a question, or a short decision answer
/// (`Apache-2.0`, `yes`, `0.1.0-preview.2`). Such a row answers or asks
/// someone; it never restates finished work, so a lexical completion match
/// must not strike it.
pub fn is_operator_reply_shape(text: &str) -> bool {
    let prose = row_prose(text);
    prose.trim_end().ends_with('?') || content_words(&prose) < MIN_TASK_CONTENT_WORDS
}

/// Deterministic public response id for a binary strike of `row_text`.
pub fn strike_response_id(row_text: &str) -> String {
    let hash = agent_doc_hash::content_hash(&format!("strike-response:{}", row_text.trim()));
    format!("r-{}", &hash[..12])
}

/// `#strucknoresponse`: the abbreviated response tree a binary strike writes
/// under the row it strikes, one line each, indented under `row_indent`:
///
/// ```text
///   - **Response [#r-…]**
///     - **Summary:** <summary>
///     - **State:** Complete.
/// ```
pub fn abbreviated_strike_response_lines(
    row_text: &str,
    row_indent: usize,
    summary: &str,
) -> Vec<String> {
    let pad = " ".repeat(row_indent + 2);
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    vec![
        format!("{pad}- **Response [#{}]**", strike_response_id(row_text)),
        format!("{pad}  - **Summary:** {summary}"),
        format!("{pad}  - **State:** Complete."),
    ]
}

/// The same lines as queue entries, to follow the struck entry.
pub fn abbreviated_strike_response_entries(
    row_text: &str,
    row_indent: usize,
    summary: &str,
) -> Vec<QueueEntry> {
    abbreviated_strike_response_lines(row_text, row_indent, summary)
        .into_iter()
        .map(QueueEntry::Freeform)
        .collect()
}

/// A struck operator row that breaks `#strucknoresponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnansweredStruckRow {
    /// 1-based line in the document.
    pub line: usize,
    /// The row as it reads in the document, trimmed.
    pub row: String,
    pub reason: UnansweredStruckReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnansweredStruckReason {
    /// The `#qftbklgstrike` semantic strike struck an operator reply (a nested
    /// row, a question, or a short answer) as completed work.
    SemanticStrikeOfReply,
    /// A struck question with no response, consumed-by or closed-by row.
    QuestionWithoutResponse,
}

impl std::fmt::Display for UnansweredStruckRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = match self.reason {
            UnansweredStruckReason::SemanticStrikeOfReply => {
                "the #qftbklgstrike completion match struck an operator reply as finished work, and no response answers it"
            }
            UnansweredStruckReason::QuestionWithoutResponse => {
                "this struck question has no response beneath it"
            }
        };
        write!(
            f,
            "struck operator prompt has no response (#strucknoresponse): line {} `{}`: {why}. Answer it, or the operator un-strikes it to re-queue it (or deletes it if it no longer applies)",
            self.line, self.row
        )
    }
}

fn prompt_of(entry: &QueueEntry) -> Option<&QueuePrompt> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt),
        _ => None,
    }
}

fn freeform_indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn answers_row(line: &str) -> bool {
    let item = line.trim_start();
    let item = item.strip_prefix("- ").unwrap_or(item);
    [
        "**Response [#r-",
        "**Consumed by:**",
        "**Closed by:**",
        "**State:**",
    ]
    .iter()
    .any(|label| item.starts_with(label))
        || agent_doc_markdown_ast::overlay::is_generated_response_structure_row(item)
}

/// Whether some row nested under entry `index` answers it.
fn has_answer_beneath(entries: &[QueueEntry], index: usize, own_indent: usize) -> bool {
    for entry in &entries[index + 1..] {
        match entry {
            QueueEntry::Freeform(text) => {
                let first = text.lines().next().unwrap_or_default();
                if first.trim().is_empty() {
                    continue;
                }
                if freeform_indent(first) <= own_indent {
                    return false;
                }
                if text.lines().any(answers_row) {
                    return true;
                }
            }
            other => match prompt_of(other) {
                Some(prompt) if prompt.indent > own_indent => {
                    if answers_row(&prompt.text) {
                        return true;
                    }
                }
                _ => return false,
            },
        }
    }
    false
}

/// Struck queue rows that break `#strucknoresponse`, in document order.
pub fn unanswered_struck_prompt_rows(document: &str) -> Vec<UnansweredStruckRow> {
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for component in components.iter().filter(|c| c.name == "queue") {
        let body = component.content(document);
        let Ok(entries) = crate::document_queue::parse(body) else {
            continue;
        };
        let body_start_line = document[..component.open_end].matches('\n').count() + 1;
        let mut search_from = 0usize;
        for (index, entry) in entries.iter().enumerate() {
            let QueueEntry::Completed(prompt) = entry else {
                continue;
            };
            let semantic = prompt.text.contains(SEMANTIC_STRIKE_MARKER);
            let reason = if semantic && (prompt.indent > 0 || is_operator_reply_shape(&prompt.text))
            {
                UnansweredStruckReason::SemanticStrikeOfReply
            } else if is_question(&prompt.text) {
                UnansweredStruckReason::QuestionWithoutResponse
            } else {
                continue;
            };
            if has_answer_beneath(&entries, index, prompt.indent) {
                continue;
            }
            let first_line = prompt.text.lines().next().unwrap_or_default().trim();
            let Some(offset) = body[search_from..].find(first_line) else {
                continue;
            };
            let at = search_from + offset;
            search_from = at + first_line.len();
            let line = body_start_line + body[..at].matches('\n').count();
            let line_start = body[..at].rfind('\n').map_or(0, |start| start + 1);
            let line_end = body[at..].find('\n').map_or(body.len(), |end| at + end);
            let row = body[line_start..line_end].trim().to_string();
            rows.push(UnansweredStruckRow { line, row, reason });
        }
    }
    rows
}

#[cfg(test)]
#[path = "struck_prompt_response_tests.rs"]
mod tests;
