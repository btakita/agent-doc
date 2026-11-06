//! `agent:recap`: the one current end-of-turn recap (`#agentrecap`).
//!
//! # Spec
//! - The component holds the last turn's recap and nothing else. A closeout
//!   replaces its whole body, whatever mode the marker or project config names,
//!   so it can never accumulate history.
//! - It is agent-authored and informational. Its body never starts a turn and is
//!   never read as a prompt or a queue row.
//! - A document that predates the component gains it the first time a closeout
//!   carries a recap, at the canonical slot directly below `agent:status`.
//! - A recap is never required. A closeout without one leaves the previous recap
//!   in place, and a blank recap counts as no recap.
//! - [`recap_format_findings`] checks only what is cheap and deterministic, and
//!   its findings are advisory. The format itself lives in `runbooks/recap.md`.
//!
//! # Agentic Contracts
//! - [`replace_recap_in_document`] changes nothing outside the recap section.
//! - [`normalize_recap_body`] is idempotent, so replaying a closeout is a no-op.
//!
//! # Evals
//! - replace_is_total_and_keeps_no_history
//! - replace_creates_the_section_below_status_and_nothing_else
//! - format_findings_accept_the_full_shape_and_any_omitted_section

use std::fmt;

use anyhow::{Context, Result};

use crate::element;

/// Component name, without the `agent:` prefix.
pub const RECAP_COMPONENT: &str = "recap";

/// Section inserted into a document that has no `agent:recap` yet.
pub const RECAP_SECTION: &str =
    "\n## Recap <!-- agent:recap patch=replace -->\n<!-- /agent:recap -->\n";

/// Fixed section labels, in the order a recap uses them. Every one is optional.
pub const RECAP_SECTION_LABELS: [&str; 3] = [
    "Queue rows this cycle:",
    "Still can't close:",
    DECISIONS_LABEL,
];

const DECISIONS_LABEL: &str = "Your decisions, most urgent first:";

/// Trim a recap to the body a closeout stores. `None` means the text carries no
/// recap at all, which leaves the document's previous recap in place.
pub fn normalize_recap_body(raw: &str) -> Option<String> {
    let body = raw.trim();
    (!body.is_empty()).then(|| format!("{body}\n"))
}

/// Replace the whole `agent:recap` body with `raw`, creating the section when
/// the document has none. A blank `raw` returns the document unchanged.
pub fn replace_recap_in_document(doc: &str, raw: &str) -> Result<String> {
    let Some(body) = normalize_recap_body(raw) else {
        return Ok(doc.to_string());
    };
    let doc = ensure_recap_component_in_document(doc)?;
    let components = element::parse(&doc).context("failed to parse components")?;
    let recap = components
        .iter()
        .find(|component| component.name == RECAP_COMPONENT)
        .context("document has no recap component after insertion")?;
    Ok(recap.replace_content(&doc, &body))
}

/// Insert an empty `agent:recap` section when the document has none.
fn ensure_recap_component_in_document(doc: &str) -> Result<String> {
    let components = element::parse(doc).context("failed to parse components")?;
    if components
        .iter()
        .any(|component| component.name == RECAP_COMPONENT)
    {
        return Ok(doc.to_string());
    }
    let offset =
        element::canonical_managed_component_insertion_offset(doc, &components, RECAP_COMPONENT);
    let (before, after) = doc.split_at(offset);
    let mut out = String::with_capacity(doc.len() + RECAP_SECTION.len() + 2);
    out.push_str(before);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(RECAP_SECTION);
    if !after.is_empty() && !after.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(after);
    Ok(out)
}

/// One deterministic departure from the shared recap format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecapFormatFinding {
    /// The recap must open with the outcome paragraph.
    MissingOutcomeParagraph,
    /// A recap uses plain section labels, not Markdown headings.
    Heading { line: String },
    /// Section labels must keep their fixed order.
    SectionOutOfOrder { label: &'static str },
    /// A section label must introduce a list.
    SectionWithoutList { label: &'static str },
    /// Operator decisions must be a numbered list.
    DecisionsNotNumbered,
}

impl fmt::Display for RecapFormatFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOutcomeParagraph => {
                f.write_str("open with the outcome paragraph, before any label or list")
            }
            Self::Heading { line } => {
                write!(
                    f,
                    "use a plain section label instead of the heading `{line}`"
                )
            }
            Self::SectionOutOfOrder { label } => {
                write!(
                    f,
                    "`{label}` is out of order; the labels have a fixed order"
                )
            }
            Self::SectionWithoutList { label } => {
                write!(f, "`{label}` must introduce a list; omit an empty section")
            }
            Self::DecisionsNotNumbered => {
                write!(f, "number the items under `{DECISIONS_LABEL}`")
            }
        }
    }
}

/// Check a recap body against the deterministic part of the shared format.
/// Findings are advisory: a closeout never fails on them.
pub fn recap_format_findings(body: &str) -> Vec<RecapFormatFinding> {
    let lines = prose_lines(body);
    let mut findings = Vec::new();
    if lines.first().is_none_or(|line| !is_outcome_paragraph(line)) {
        findings.push(RecapFormatFinding::MissingOutcomeParagraph);
    }
    let mut last_rank = None;
    for (index, line) in lines.iter().enumerate() {
        if is_heading(line) {
            findings.push(RecapFormatFinding::Heading {
                line: line.trim().to_string(),
            });
        }
        let Some(rank) = label_rank(line) else {
            continue;
        };
        let label = RECAP_SECTION_LABELS[rank];
        if last_rank.is_some_and(|last| rank < last) {
            findings.push(RecapFormatFinding::SectionOutOfOrder { label });
        }
        last_rank = Some(rank);
        let items = section_items(&lines[index + 1..]);
        if items.is_empty() {
            findings.push(RecapFormatFinding::SectionWithoutList { label });
        } else if label == DECISIONS_LABEL && items.iter().any(|item| !is_numbered_item(item)) {
            findings.push(RecapFormatFinding::DecisionsNotNumbered);
        }
    }
    findings
}

/// Non-blank lines outside fenced code, so a quoted example is never checked.
fn prose_lines(body: &str) -> Vec<&str> {
    let mut in_fence = false;
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence && !line.trim().is_empty()
        })
        .collect()
}

fn label_rank(line: &str) -> Option<usize> {
    let line = line.trim().replace('\u{2019}', "'");
    RECAP_SECTION_LABELS
        .iter()
        .position(|label| *label == line.trim_matches('*'))
}

/// Top-level list items directly under a label, up to the next prose line.
fn section_items<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .take_while(|line| line.starts_with(char::is_whitespace) || is_list_item(line))
        .filter(|line| is_list_item(line))
        .copied()
        .collect()
}

fn is_outcome_paragraph(line: &str) -> bool {
    !is_heading(line)
        && !is_list_item(line)
        && !line.trim_start().starts_with('>')
        && label_rank(line).is_none()
}

fn is_heading(line: &str) -> bool {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

fn is_list_item(line: &str) -> bool {
    ["- ", "* ", "+ "]
        .iter()
        .any(|marker| line.starts_with(marker))
        || is_numbered_item(line)
}

fn is_numbered_item(line: &str) -> bool {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_RECAP: &str = concat!(
        "The cycle is committed at abc123, and no workers are running. Nothing you need to read changed.\n\n",
        "The release is waiting on approval. The review is requested; I did not check the mirror.\n\n",
        "Queue rows this cycle:\n",
        "- Closed: the docs row.\n",
        "- Waiting on you (🙋): the release row.\n\n",
        "Still can't close:\n",
        "- do [#example]: the merge is still blocked.\n\n",
        "Your decisions, most urgent first:\n",
        "1. Release: ship or hold. I recommend hold.\n",
        "2. Docs: publish or hold.\n",
    );

    fn doc() -> String {
        concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status <!-- agent:status patch=replace -->\nReady.\n<!-- /agent:status -->\n\n",
            "## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
            "## Exchange <!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .to_string()
    }

    fn recap_body(doc: &str) -> String {
        element::parse(doc)
            .unwrap()
            .iter()
            .find(|component| component.name == RECAP_COMPONENT)
            .unwrap()
            .content(doc)
            .to_string()
    }

    #[test]
    fn replace_creates_the_section_below_status_and_nothing_else() {
        let before = doc();
        let after = replace_recap_in_document(&before, "\n\nDone.\n\n").unwrap();

        assert_eq!(recap_body(&after), "Done.\n");
        let status = after.find("<!-- /agent:status -->").unwrap();
        let recap = after.find("<!-- agent:recap patch=replace -->").unwrap();
        let active = after.find("<!-- agent:active priority -->").unwrap();
        assert!(status < recap && recap < active, "{after}");
        let without_body = after.replacen("Done.\n", "", 1);
        assert_eq!(without_body.replacen(RECAP_SECTION, "", 1), before);
    }

    #[test]
    fn replace_is_total_and_keeps_no_history() {
        let first = replace_recap_in_document(&doc(), "First turn.").unwrap();
        let second = replace_recap_in_document(&first, "Second turn.").unwrap();

        assert_eq!(recap_body(&second), "Second turn.\n");
        assert!(!second.contains("First turn."));
        assert_eq!(second.matches("<!-- agent:recap").count(), 1);
    }

    #[test]
    fn replay_of_the_same_recap_is_a_no_op() {
        let first = replace_recap_in_document(&doc(), "Same.\n").unwrap();
        assert_eq!(replace_recap_in_document(&first, "Same.\n").unwrap(), first);
    }

    #[test]
    fn blank_recap_keeps_the_previous_one_and_creates_nothing() {
        let without = doc();
        assert_eq!(
            replace_recap_in_document(&without, " \n\n").unwrap(),
            without
        );

        let with = replace_recap_in_document(&without, "Kept.").unwrap();
        assert_eq!(replace_recap_in_document(&with, "\n").unwrap(), with);
    }

    /// The opener may share a heading's line, and the first canonical successor
    /// may be the only anchor a document has. The section must land on its own
    /// lines above that heading instead of splitting it from its opener.
    #[test]
    fn replace_goes_above_a_successor_that_owns_a_heading() {
        let same_line = "## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let after = replace_recap_in_document(same_line, "Done.").unwrap();
        assert!(
            after.contains("## Exchange <!-- agent:exchange -->\n"),
            "{after}"
        );
        assert!(
            after.find("<!-- /agent:recap -->").unwrap() < after.find("## Exchange").unwrap(),
            "{after}"
        );

        let own_line = "## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let after = replace_recap_in_document(own_line, "Done.").unwrap();
        assert!(
            after.contains("## Exchange\n\n<!-- agent:exchange -->\n"),
            "{after}"
        );
        assert!(
            after.find("<!-- /agent:recap -->").unwrap() < after.find("## Exchange").unwrap(),
            "{after}"
        );
    }

    #[test]
    fn format_findings_accept_the_full_shape_and_any_omitted_section() {
        assert_eq!(recap_format_findings(FULL_RECAP), []);
        assert_eq!(recap_format_findings("One outcome paragraph.\n"), []);
        assert_eq!(
            recap_format_findings("Outcome.\n\nYour decisions, most urgent first:\n1. Ship.\n"),
            []
        );
    }

    #[test]
    fn format_findings_name_each_deterministic_departure() {
        assert_eq!(
            recap_format_findings(""),
            [RecapFormatFinding::MissingOutcomeParagraph]
        );
        assert_eq!(
            recap_format_findings("- a list first\n"),
            [RecapFormatFinding::MissingOutcomeParagraph]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\n### Decisions\n"),
            [RecapFormatFinding::Heading {
                line: "### Decisions".to_string()
            }]
        );
        assert_eq!(
            recap_format_findings(concat!(
                "Outcome.\n\n",
                "Your decisions, most urgent first:\n1. Ship.\n\n",
                "Still can't close:\n- do [#a]: blocked.\n",
            )),
            [RecapFormatFinding::SectionOutOfOrder {
                label: "Still can't close:"
            }]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\nStill can't close:\n\nNothing, really.\n"),
            [RecapFormatFinding::SectionWithoutList {
                label: "Still can't close:"
            }]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\nYour decisions, most urgent first:\n- Ship.\n"),
            [RecapFormatFinding::DecisionsNotNumbered]
        );
    }

    #[test]
    fn format_findings_read_nested_lists_and_ignore_fenced_examples() {
        assert_eq!(
            recap_format_findings(concat!(
                "Outcome.\n\n",
                "Queue rows this cycle:\n- Closed:\n  - one\n  - two\n\n",
                "```text\n# not a heading\nStill can't close:\n```\n",
            )),
            []
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Ship.\n   - detail\n"
            ),
            []
        );
    }

    #[test]
    fn format_labels_tolerate_a_typographic_apostrophe() {
        assert_eq!(
            recap_format_findings("Outcome.\n\nStill can\u{2019}t close:\n- do [#a]: blocked.\n"),
            []
        );
    }

    #[test]
    fn every_finding_renders_guidance() {
        let findings = [
            RecapFormatFinding::MissingOutcomeParagraph,
            RecapFormatFinding::Heading {
                line: "# x".to_string(),
            },
            RecapFormatFinding::SectionOutOfOrder { label: "a" },
            RecapFormatFinding::SectionWithoutList { label: "a" },
            RecapFormatFinding::DecisionsNotNumbered,
        ];
        for finding in findings {
            assert!(!finding.to_string().is_empty());
        }
    }
}
