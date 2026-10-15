//! `#subagentcompletionfeedback` — the grammar of the binary-owned subagent
//! completion cell that `agent-doc queue report-complete` appends to
//! `agent:exchange`, and its structural recognition.
//!
//! A completion cell is three contiguous lines, written only by the binary:
//!
//! ```text
//! ### Re: Subagent completed: `subagent:sdk` returned work for queue item “finish sdk” — agent-doc
//! Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
//! <!-- agent-doc-subagent-completion key=<hash> -->
//! ```
//!
//! The heading follows the ordinary `### Re: topic — attribution` shape with a
//! single spaced em dash: the topic names the worker and the queue item, so two
//! cells never share a topic, and the attribution is the binary itself.
//!
//! Recognition is structural, never a fuzzy text match on prose: a heading is
//! a binary completion cell only when it carries the completion heading grammar
//! AND is followed by the fixed binary body and the keyed completion marker (or,
//! for the short-lived v0.34 bodyless form, directly by the marker). Recovery
//! and closeout classifiers use [`completion_cell_line_ranges`] to keep these
//! cells out of "agent response" reasoning: they are not a direct patchback, not
//! a pending response to replay, and not a replayed heading.
//!
//! Two heading grammars exist in documents:
//! - current: [`HEADING_PREFIX`] … [`HEADING_ATTRIBUTION_SUFFIX`];
//! - legacy (≤ 0.35.503): [`LEGACY_HEADING_PREFIX`], which put the whole detail
//!   AFTER the em dash. Every legacy cell therefore normalized to the same topic
//!   `Subagent completed`, which made two or more uncommitted cells read as one
//!   replayed heading. Legacy cells stay recognized so an already-affected
//!   document is admitted without migration.

use std::ops::Range;

/// Current completion heading prefix; the topic continues with the owner/item.
pub const HEADING_PREFIX: &str = "### Re: Subagent completed: ";
/// The attribution that closes a current completion heading.
pub const HEADING_ATTRIBUTION_SUFFIX: &str = " — agent-doc";
/// The pre-fix heading prefix (detail after the em dash).
pub const LEGACY_HEADING_PREFIX: &str = "### Re: Subagent completed — agent-doc:";
/// The fixed binary-authored body line.
pub const BODY: &str = "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.";
/// The keyed marker that closes every completion cell.
pub const MARKER_PREFIX: &str = "<!-- agent-doc-subagent-completion key=";

/// The completion heading for `owner` and the one-line `item` text.
///
/// Em dashes and backticks inside the owner/item are neutralized so the heading
/// keeps exactly one spaced em dash (the attribution separator) and the owner
/// stays one code span.
pub fn heading(owner: &str, item: &str) -> String {
    let owner = owner.replace('`', "'").replace('—', "-");
    let item = item
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['“', '”'], "'")
        .replace('—', "-");
    format!(
        "{HEADING_PREFIX}`{owner}` returned work for queue item “{item}”{HEADING_ATTRIBUTION_SUFFIX}"
    )
}

/// The keyed marker line for a completion `key`.
pub fn marker(key: &str) -> String {
    format!("{MARKER_PREFIX}{key} -->")
}

/// The complete contiguous cell (no trailing newline).
pub fn cell(owner: &str, item: &str, key: &str) -> String {
    format!("{}\n{BODY}\n{}", heading(owner, item), marker(key))
}

/// True when `line` carries either completion heading grammar. This is the
/// heading shape only; whether the heading is a binary cell is decided by
/// [`completion_cell_line_ranges`].
pub fn is_completion_heading(line: &str) -> bool {
    let line = line.trim();
    let line = line.strip_suffix(" (HEAD)").unwrap_or(line);
    if let Some(detail) = line.strip_prefix(LEGACY_HEADING_PREFIX) {
        return !detail.trim().is_empty();
    }
    line.strip_prefix(HEADING_PREFIX)
        .and_then(|rest| rest.strip_suffix(HEADING_ATTRIBUTION_SUFFIX))
        .is_some_and(|topic| !topic.trim().is_empty())
}

/// True when `line` is a keyed completion marker.
pub fn is_marker_line(line: &str) -> bool {
    line.trim()
        .strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(" -->"))
        .is_some_and(|key| !key.is_empty() && !key.chars().any(char::is_whitespace))
}

/// Line ranges (0-based, over `text.lines()`) of every binary-attested
/// completion cell: heading, optional fixed body, keyed marker.
pub fn completion_cell_line_ranges(text: &str) -> Vec<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !is_completion_heading(lines[index]) {
            index += 1;
            continue;
        }
        let next = lines.get(index + 1).copied();
        let end = if next.is_some_and(is_marker_line) {
            Some(index + 2)
        } else if next.is_some_and(|line| line.trim() == BODY)
            && lines.get(index + 2).copied().is_some_and(is_marker_line)
        {
            Some(index + 3)
        } else {
            None
        };
        match end {
            Some(end) => {
                ranges.push(index..end);
                index = end;
            }
            None => index += 1,
        }
    }
    ranges
}

/// 0-based line indices of the headings that open a binary-attested cell.
pub fn completion_cell_heading_lines(text: &str) -> Vec<usize> {
    completion_cell_line_ranges(text)
        .into_iter()
        .map(|range| range.start)
        .collect()
}

/// `text` without its binary-attested completion cells, for drift
/// comparisons that must not count binary feedback as agent or operator text.
///
/// Each cell is removed together with the one blank separator line directly
/// before it, which `report-complete` inserts when the preceding exchange line
/// is not already blank. When that blank was already present the comparison
/// form is one blank line SHORTER than the original — callers compare for
/// drift, where a missing trailing blank is never prompt- or response-bearing.
/// Every other line, including a completion-shaped heading that is NOT followed
/// by the binary body and marker, is preserved byte for byte.
pub fn strip_completion_cells(text: &str) -> String {
    let ranges = completion_cell_line_ranges(text);
    if ranges.is_empty() {
        return text.to_string();
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut removed = vec![false; lines.len()];
    for range in ranges {
        for index in range.clone() {
            removed[index] = true;
        }
        if let Some(separator) = range.start.checked_sub(1)
            && !removed[separator]
            && lines[separator].trim().is_empty()
        {
            removed[separator] = true;
        }
    }
    lines
        .iter()
        .zip(removed)
        .filter(|(_, removed)| !removed)
        .map(|(line, _)| *line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_FOUR: &str = concat!(
        "### Re: Subagent completed — agent-doc: `subagent:gh262` returned work for queue item “#gh-fix https://github.com/example/project/issues/262”; coordinator integration and closeout are pending.\n",
        "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.\n",
        "<!-- agent-doc-subagent-completion key=a -->\n",
        "### Re: Subagent completed — agent-doc: `subagent:gh264` returned work for queue item “#gh-fix https://github.com/example/project/issues/264”; coordinator integration and closeout are pending.\n",
        "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.\n",
        "<!-- agent-doc-subagent-completion key=b -->\n",
        "### Re: Subagent completed — agent-doc: `subagent:gh263` returned work for queue item “#gh-fix https://github.com/example/project/issues/263”; coordinator integration and closeout are pending.\n",
        "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.\n",
        "<!-- agent-doc-subagent-completion key=c -->\n",
        "### Re: Subagent completed — agent-doc: `subagent:gh261` returned work for queue item “#gh-fix https://github.com/example/project/issues/261”; coordinator integration and closeout are pending.\n",
        "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.\n",
        "<!-- agent-doc-subagent-completion key=d -->\n",
    );

    #[test]
    fn current_heading_has_one_em_dash_and_a_unique_topic() {
        let one = heading("subagent:gh262", "#gh-fix https://example/262");
        let two = heading("subagent:gh264", "#gh-fix https://example/264");
        assert_eq!(one.matches(" — ").count(), 1, "{one}");
        assert!(one.ends_with(" — agent-doc"));
        assert_ne!(
            one.rsplit_once(" — ").unwrap().0,
            two.rsplit_once(" — ").unwrap().0
        );
        let dashed = heading("subagent:x", "split — on — dashes\nand lines");
        assert_eq!(dashed.matches('—').count(), 1, "{dashed}");
        assert!(dashed.contains("“split - on - dashes and lines”"));
        assert!(is_completion_heading(&one));
    }

    #[test]
    fn recognizes_current_legacy_and_bodyless_cells_structurally() {
        let current = format!("{}\n{}\n", cell("subagent:a", "finish a", "k1"), "tail");
        assert_eq!(completion_cell_line_ranges(&current), vec![0..3]);
        assert_eq!(completion_cell_line_ranges(LEGACY_FOUR).len(), 4);
        let bodyless = "### Re: Subagent completed — agent-doc: `subagent:a` returned work.\n<!-- agent-doc-subagent-completion key=x -->\n";
        assert_eq!(completion_cell_line_ranges(bodyless), vec![0..2]);
        assert_eq!(strip_completion_cells(LEGACY_FOUR), "");
        assert_eq!(strip_completion_cells(&current), "tail\n");
        // The separator blank `report-complete` inserts goes with each cell.
        let written = format!(
            "Done.\n\n{}\n\n{}\n<!-- agent:boundary:keep -->\n",
            cell("subagent:a", "a", "k1"),
            cell("subagent:b", "b", "k2"),
        );
        assert_eq!(
            strip_completion_cells(&written),
            "Done.\n<!-- agent:boundary:keep -->\n"
        );
    }

    #[test]
    fn heading_text_alone_is_not_binary_authority() {
        // An agent (or operator) writing the same heading words without the
        // binary body and keyed marker is NOT a completion cell.
        let forged = concat!(
            "### Re: Subagent completed: `subagent:a` returned work for queue item “a” — agent-doc\n",
            "\n",
            "I integrated it by hand.\n",
        );
        assert!(completion_cell_line_ranges(forged).is_empty());
        let edited_body = concat!(
            "### Re: Subagent completed: `subagent:a` returned work for queue item “a” — agent-doc\n",
            "Operator edited this body.\n",
            "<!-- agent-doc-subagent-completion key=x -->\n",
        );
        assert!(completion_cell_line_ranges(edited_body).is_empty());
        let ordinary = "### Re: Subagent completed — opus-5 · 2026-10-14T22:00-04:00\n<!-- agent-doc-subagent-completion key=x -->\n";
        assert!(
            completion_cell_line_ranges(ordinary).is_empty(),
            "a model-attributed heading is not the legacy binary grammar"
        );
        assert_eq!(strip_completion_cells(forged), forged);
    }
}
