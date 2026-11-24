//! `#subagentcompletionfeedback` — the grammar of the binary-owned subagent
//! completion cell that `agent-doc queue report-complete` appends to
//! `agent:exchange`, and its structural recognition.
//!
//! A completion cell contains the worker's response, written only by the binary:
//!
//! ```text
//! ### Re: Subagent completed: `subagent:sdk` returned work for queue item “finish sdk” — agent-doc
//! <!-- agent-doc-subagent-response sha256=<body hash> -->
//! > **Subagent response:**
//! >
//! > Implemented the requested change.
//! <!-- agent-doc-subagent-completion key=<hash> -->
//! ```
//!
//! The heading follows the ordinary `### Re: topic — attribution` shape with a
//! single spaced em dash: the topic names the worker and the queue item, so two
//! cells never share a topic, and the attribution is the binary itself.
//!
//! Recognition is structural, never a fuzzy text match on prose: a heading is
//! a binary completion cell only when it carries the completion heading grammar
//! AND is followed by a hash-attested quoted worker response and the keyed
//! completion marker (optionally with its exact queue-prompt echo between the
//! heading and digest, or one of the legacy fixed/bodyless forms). Recovery
//! and closeout classifiers use [`completion_cell_line_ranges`] to keep these
//! cells out of "agent response" reasoning: they are not a direct patchback, not
//! a pending response to replay, and not a replayed heading.
//! Once the matching queue item is proven integrated and struck, closeout uses
//! [`strip_completion_cells_matching`] to retire the transient cell in the same
//! authoritative document successor as the queue strike. Unmatched reports stay
//! visible and continue to provide durable recovery evidence.
//!
//! Two heading grammars exist in documents:
//! - current: [`HEADING_PREFIX`] … [`HEADING_ATTRIBUTION_SUFFIX`];
//! - legacy (≤ 0.35.503): [`LEGACY_HEADING_PREFIX`], which put the whole detail
//!   AFTER the em dash. Every legacy cell therefore normalized to the same topic
//!   `Subagent completed`, which made two or more uncommitted cells read as one
//!   replayed heading. Legacy cells stay recognized so an already-affected
//!   document is admitted without migration.

use std::ops::Range;

/// Binary-attested evidence that a worker returned work for one queue item.
/// The key attests the cell, but does not by itself prove which current claim
/// generation produced it; callers reconcile that lineage separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEvidence {
    pub owner: String,
    pub item: String,
    pub key: String,
}

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
/// The digest marker that opens a current worker-response body.
pub const RESPONSE_DIGEST_PREFIX: &str = "<!-- agent-doc-subagent-response sha256=";

/// The completion heading for `owner` and the one-line `item` text.
///
/// Em dashes and backticks inside the owner/item are neutralized so the heading
/// keeps exactly one spaced em dash (the attribution separator) and the owner
/// stays one code span.
pub fn heading(owner: &str, item: &str) -> String {
    let owner = owner.replace('`', "'").replace('—', "-");
    let item = heading_item_text(item);
    format!(
        "{HEADING_PREFIX}`{owner}` returned work for queue item “{item}”{HEADING_ATTRIBUTION_SUFFIX}"
    )
}

/// The one-line item text a completion heading records for queue item
/// `item`: whitespace collapsed, curly quotes and em dashes neutralized. A
/// cell's [`CompletionEvidence::item`] is this form of its row's text, so a
/// reader matching a cell to a row must compare both forms of the row.
pub fn heading_item_text(item: &str) -> String {
    item.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['“', '”'], "'")
        .replace('—', "-")
}

/// The keyed marker line for a completion `key`.
pub fn marker(key: &str) -> String {
    format!("{MARKER_PREFIX}{key} -->")
}

/// The complete contiguous cell (no trailing newline).
pub fn cell(owner: &str, item: &str, key: &str) -> String {
    format!("{}\n{BODY}\n{}", heading(owner, item), marker(key))
}

/// A current completion cell carrying the worker's actual response.
///
/// The response is blockquoted so headings, component markers, and other
/// markdown emitted by a worker remain visible without becoming live session
/// structure. The digest makes an edited body cease to be binary evidence.
/// A response that labels its sections is rendered into the response
/// structure first (`#workerresponsestructure`,
/// [`crate::worker_handback::structure_response`]); an unlabelled response is
/// quoted verbatim.
pub fn cell_with_response(
    owner: &str,
    item: &str,
    key: &str,
    response: &str,
) -> anyhow::Result<String> {
    let response = response.trim();
    anyhow::ensure!(
        !response.is_empty(),
        "subagent completion response must not be empty"
    );
    let structured = crate::worker_handback::structure_response(response);
    let response = structured.trim();
    let mut body = String::from("> **Subagent response:**\n>");
    for line in response.lines() {
        let line = line.trim_end();
        body.push('\n');
        body.push('>');
        if !line.is_empty() {
            body.push(' ');
            body.push_str(line);
        }
    }
    let digest = agent_doc_hash::content_hash(&body);
    Ok(format!(
        "{}\n{RESPONSE_DIGEST_PREFIX}{digest} -->\n{body}\n{}",
        heading(owner, item),
        marker(key),
    ))
}

/// True when `line` carries either completion heading grammar. This is the
/// heading shape only; whether the heading is a binary cell is decided by
/// [`completion_cell_line_ranges`].
pub fn is_completion_heading(line: &str) -> bool {
    let line = line.trim();
    let line = line.strip_suffix(" (HEAD)").unwrap_or(line);
    let line = split_heading_response_id(line).0;
    if let Some(detail) = line.strip_prefix(LEGACY_HEADING_PREFIX) {
        return !detail.trim().is_empty();
    }
    line.strip_prefix(HEADING_PREFIX)
        .and_then(|rest| rest.strip_suffix(HEADING_ATTRIBUTION_SUFFIX))
        .is_some_and(|topic| !topic.trim().is_empty())
}

/// `#exchangeresponseid`: split the public response id a binary heading
/// carries at its end (`… — agent-doc [#r-…]`) off the heading.
///
/// Only the trailing id counts. The queue item a heading quotes may itself
/// mention a response id (`“`[#r-…]` was not printed …”`); that id belongs to
/// another response and is never this cell's.
pub fn split_heading_response_id(heading: &str) -> (&str, Option<&str>) {
    let trimmed = heading.trim_end();
    if let Some(open) = trimmed.rfind(" [#r-") {
        let id = &trimmed[open + 1..];
        if agent_doc_markdown_ast::exchange_tree::is_compact_response_id(id) {
            return (&trimmed[..open], Some(id));
        }
    }
    (heading, None)
}

/// `#exchangeresponseid`: `line` (a heading line, its ending kept) with its
/// trailing response id set to `response_id`. `None` when it already carries
/// exactly that id or `response_id` is not a public response id.
pub fn heading_line_with_response_id(line: &str, response_id: &str) -> Option<String> {
    if !agent_doc_markdown_ast::exchange_tree::is_compact_response_id(response_id) {
        return None;
    }
    let content_end = line.trim_end_matches(['\n', '\r']).len();
    let (content, ending) = line.split_at(content_end);
    let content = content.trim_end();
    let (content, head) = match content.strip_suffix(" (HEAD)") {
        Some(rest) => (rest, " (HEAD)"),
        None => (content, ""),
    };
    let (base, existing) = split_heading_response_id(content);
    if existing == Some(response_id) {
        return None;
    }
    Some(format!("{} {response_id}{head}{ending}", base.trim_end()))
}

/// `#exchangeresponseid`: the trailing response id of each attested
/// completion cell's heading, by completion key.
pub fn completion_cell_response_ids(text: &str) -> std::collections::HashMap<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    completion_cell_line_ranges(text)
        .into_iter()
        .filter_map(|range| {
            let evidence = completion_evidence_for_range(&lines, &range)?;
            let heading = lines[range.start].trim();
            let heading = heading.strip_suffix(" (HEAD)").unwrap_or(heading);
            let id = split_heading_response_id(heading).1?;
            Some((evidence.key, id.to_string()))
        })
        .collect()
}

/// `#exchangeresponseid`: tag the heading of the attested completion cell
/// keyed `key` with `response_id`, the id of the response tree a queue row
/// shows for that cell's hand-back. The heading is outside the response
/// digest, so the cell stays attested. `None` when no such cell exists or it
/// already carries that id.
pub fn annotate_completion_cell_response_id(
    text: &str,
    key: &str,
    response_id: &str,
) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let range = completion_cell_line_ranges(text)
        .into_iter()
        .find(|range| {
            completion_evidence_for_range(&lines, range).is_some_and(|evidence| evidence.key == key)
        })?;
    replace_line(text, range.start, |line| {
        heading_line_with_response_id(line, response_id)
    })
}

/// Replace 0-based line `index` of `text` (ending included) through `edit`.
pub(crate) fn replace_line(
    text: &str,
    index: usize,
    edit: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    let mut start = 0usize;
    for (at, line) in text.split_inclusive('\n').enumerate() {
        if at == index {
            let replacement = edit(line)?;
            let mut out = String::with_capacity(text.len() + replacement.len());
            out.push_str(&text[..start]);
            out.push_str(&replacement);
            out.push_str(&text[start + line.len()..]);
            return Some(out);
        }
        start += line.len();
    }
    None
}

/// A retired completion cell kept as a plain hand-back record
/// ([`retain_completion_cells_as_handback_records`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandbackRecord {
    /// 0-based line of the record heading in the scanned text.
    pub line: usize,
    pub owner: String,
    pub item: String,
    /// The response id the heading carries, when tagged.
    pub response_id: Option<String>,
    /// The worker's response, unquoted.
    pub response: String,
}

/// Every hand-back record in `text`: the record heading followed directly by
/// the quoted `> **Subagent response:**` body it kept from its cell.
pub fn handback_records(text: &str) -> Vec<HandbackRecord> {
    let lines: Vec<&str> = text.lines().collect();
    let mut records = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let trimmed = trimmed.strip_suffix(" (HEAD)").unwrap_or(trimmed);
        let (heading, response_id) = split_heading_response_id(trimmed);
        let Some(detail) = heading.strip_prefix(HANDBACK_RECORD_HEADING_PREFIX) else {
            continue;
        };
        let Some(owner_end) = detail.strip_prefix('`').and_then(|rest| rest.find('`')) else {
            continue;
        };
        let owner = detail[1..owner_end + 1].to_string();
        let Some(item) = detail[owner_end + 2..]
            .trim_start()
            .strip_prefix("for queue item “")
            .and_then(|rest| rest.strip_suffix('”'))
        else {
            continue;
        };
        if lines.get(index + 1).map(|line| line.trim()) != Some("> **Subagent response:**") {
            continue;
        }
        let response = lines[index + 2..]
            .iter()
            .take_while(|line| line.trim_start().starts_with('>'))
            .map(|line| {
                line.strip_prefix("> ")
                    .or_else(|| line.strip_prefix('>'))
                    .unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if owner.is_empty() || item.is_empty() || response.is_empty() {
            continue;
        }
        records.push(HandbackRecord {
            line: index,
            owner,
            item: item.to_string(),
            response_id: response_id.map(str::to_string),
            response,
        });
    }
    records
}

/// `#exchangeresponseid`: tag the hand-back record heading at 0-based `line`
/// with `response_id`. `None` when that line is no record heading or already
/// carries the id.
pub fn annotate_handback_record_response_id(
    text: &str,
    line: usize,
    response_id: &str,
) -> Option<String> {
    handback_records(text)
        .iter()
        .any(|record| record.line == line)
        .then_some(())?;
    replace_line(text, line, |heading| {
        heading_line_with_response_id(heading, response_id)
    })
}

/// True when `line` is a keyed completion marker.
pub fn is_marker_line(line: &str) -> bool {
    line.trim()
        .strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(" -->"))
        .is_some_and(|key| !key.is_empty() && !key.chars().any(char::is_whitespace))
}

/// Line ranges (0-based, over `text.lines()`) of every binary-attested
/// completion cell: heading, optional matching queue-prompt echo, attested
/// response or optional legacy fixed body, keyed marker.
pub fn completion_cell_line_ranges(text: &str) -> Vec<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !is_completion_heading(lines[index]) {
            index += 1;
            continue;
        }
        let digest_index = completion_response_digest_index(&lines, index);
        let next = lines.get(index + 1).copied();
        let end = if let Some(digest_index) = digest_index {
            let expected_digest = response_digest(lines[digest_index])
                .expect("completion response digest index must name a digest");
            let marker_index = lines[digest_index + 1..]
                .iter()
                .position(|line| is_marker_line(line))
                .map(|offset| digest_index + 1 + offset);
            marker_index.and_then(|marker_index| {
                let body = lines[digest_index + 1..marker_index].join("\n");
                (!body.is_empty() && agent_doc_hash::content_hash(&body) == expected_digest)
                    .then_some(marker_index + 1)
            })
        } else if next.is_some_and(is_marker_line) {
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

/// Digest line for a current completion cell. Ordinarily it immediately
/// follows the heading. A historic queue-response canonicalizer inserted the
/// cell's own `> **Queue prompt:**` echo between those two lines; accept that
/// shape only when the quoted prompt matches the item encoded in the heading.
fn completion_response_digest_index(lines: &[&str], heading_index: usize) -> Option<usize> {
    let next_index = heading_index.checked_add(1)?;
    if lines
        .get(next_index)
        .copied()
        .and_then(response_digest)
        .is_some()
    {
        return Some(next_index);
    }

    let (_, heading_item) = completion_heading_owner_item(lines.get(heading_index)?.trim())?;
    let mut index = next_index;
    while lines.get(index).is_some_and(|line| line.trim().is_empty()) {
        index += 1;
    }
    if lines.get(index)?.trim() != "> **Queue prompt:**" {
        return None;
    }
    index += 1;

    let mut prompt_lines = Vec::new();
    loop {
        let line = *lines.get(index)?;
        if response_digest(line).is_some() {
            break;
        }
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        let quoted = line.trim_start().strip_prefix('>')?.trim_start();
        if !quoted.is_empty() {
            prompt_lines.push(quoted);
        }
        index += 1;
    }
    if prompt_lines.is_empty() {
        return None;
    }
    let prompt = heading_item_text(&prompt_lines.join(" "));
    let prompt = agent_doc_element_queue::strip_queue_report_state_annotation(&prompt);
    (prompt == heading_item).then_some(index)
}

fn response_digest(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(RESPONSE_DIGEST_PREFIX)?
        .strip_suffix(" -->")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

/// Parse every structurally binary-attested completion cell whose heading
/// carries an owner and exact queue-item text. A heading-shaped prose line is
/// never evidence: it must be part of a range accepted by
/// [`completion_cell_line_ranges`].
pub fn completion_evidence(text: &str) -> Vec<CompletionEvidence> {
    let lines: Vec<&str> = text.lines().collect();
    completion_cell_line_ranges(text)
        .into_iter()
        .filter_map(|range| completion_evidence_for_range(&lines, &range))
        .collect()
}

/// Recover the full hash-attested worker response for one completion id.
/// Legacy/bodyless cells deliberately return `None`: absence is not response
/// proof.
pub fn completion_response(text: &str, expected_key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for range in completion_cell_line_ranges(text) {
        let Some(evidence) = completion_evidence_for_range(&lines, &range) else {
            continue;
        };
        if evidence.key != expected_key {
            continue;
        }
        let digest_index = completion_response_digest_index(&lines, range.start)?;
        response_digest(lines.get(digest_index)?)?;
        let body = lines.get(digest_index + 1..range.end.checked_sub(1)?)?;
        let first = body.first()?.trim();
        if first != "> **Subagent response:**" {
            return None;
        }
        let response = body
            .iter()
            .skip(1)
            .map(|line| {
                line.strip_prefix("> ")
                    .or_else(|| line.strip_prefix('>'))
                    .unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        return (!response.is_empty()).then_some(response);
    }
    None
}

/// Rebuild attested completion cells whose worker response contains a complete
/// strict-template transport envelope, recomputing the response digest while
/// preserving the completion key and queue-item lineage.
pub fn normalize_completion_response_transport(text: &str) -> anyhow::Result<String> {
    let evidence = completion_evidence(text);
    let mut normalized = text.to_string();
    for completion in evidence {
        let Some(response) = completion_response(&normalized, &completion.key) else {
            continue;
        };
        let cleaned = crate::worker_handback::response_text_without_template_transport(&response);
        if cleaned == response {
            continue;
        }
        let replacement = cell_with_response(
            &completion.owner,
            &completion.item,
            &completion.key,
            &cleaned,
        )?;
        let Some(updated) =
            replace_attested_completion_cell(&normalized, &completion.key, &replacement)?
        else {
            continue;
        };
        normalized = updated;
    }
    Ok(normalized)
}

/// Replace one structurally attested completion cell by key.
///
/// The caller supplies a complete replacement cell. Both the existing and
/// replacement cells must carry `expected_key`, so ordinary prose containing a
/// marker-shaped line can never be rewritten. The original trailing newline is
/// preserved.
pub fn replace_attested_completion_cell(
    text: &str,
    expected_key: &str,
    replacement: &str,
) -> anyhow::Result<Option<String>> {
    let replacement_evidence = completion_evidence(replacement);
    let replacement_ranges = completion_cell_line_ranges(replacement);
    anyhow::ensure!(
        replacement_evidence.len() == 1
            && replacement_evidence[0].key == expected_key
            && replacement_ranges.len() == 1
            && replacement_ranges[0].start == 0
            && replacement_ranges[0].end == replacement.lines().count(),
        "replacement must be exactly one completion cell for key {expected_key}"
    );

    let lines: Vec<&str> = text.lines().collect();
    let Some(range) = completion_cell_line_ranges(text).into_iter().find(|range| {
        completion_evidence_for_range(&lines, range)
            .is_some_and(|evidence| evidence.key == expected_key)
    }) else {
        return Ok(None);
    };

    let mut line_starts = vec![0];
    line_starts.extend(
        text.match_indices('\n')
            .map(|(index, _)| index + 1)
            .filter(|index| *index < text.len()),
    );
    let start = line_starts[range.start];
    let end = line_starts.get(range.end).copied().unwrap_or(text.len());
    let kept_trailing_newline = text[start..end].ends_with('\n');
    let mut updated = String::with_capacity(text.len() + replacement.len() + 1);
    updated.push_str(&text[..start]);
    if needs_leading_separator(&text[..start]) {
        updated.push('\n');
    }
    updated.push_str(replacement);
    if kept_trailing_newline {
        updated.push('\n');
    }
    updated.push_str(&text[end..]);
    Ok(Some(updated))
}

/// `#subagentcompletioncell`: whether a completion cell placed right after
/// `before` needs a blank separator line first. A cell glued to the preceding
/// response makes that response's closing prose the block directly above a
/// `### Re:` heading, which every prompt classifier reads as an answered
/// operator prompt. Component and boundary markers need no separator.
pub fn needs_leading_separator(before: &str) -> bool {
    let Some(prior) = before.strip_suffix('\n') else {
        return false;
    };
    let previous = prior.rsplit('\n').next().unwrap_or(prior).trim();
    !previous.is_empty() && !previous.starts_with("<!-- agent:")
}

fn completion_evidence_for_range(
    lines: &[&str],
    range: &Range<usize>,
) -> Option<CompletionEvidence> {
    let heading = lines.get(range.start)?.trim();
    let marker = lines.get(range.end.checked_sub(1)?)?.trim();
    let key = marker
        .strip_prefix(MARKER_PREFIX)?
        .strip_suffix(" -->")?
        .to_string();
    let (owner, item) = completion_heading_owner_item(heading)?;
    (!owner.is_empty() && !item.is_empty()).then_some(CompletionEvidence { owner, item, key })
}

fn completion_heading_owner_item(heading: &str) -> Option<(String, String)> {
    let heading = split_heading_response_id(heading).0;
    let detail = if let Some(detail) = heading
        .strip_prefix(HEADING_PREFIX)
        .and_then(|rest| rest.strip_suffix(HEADING_ATTRIBUTION_SUFFIX))
    {
        detail
    } else {
        heading.strip_prefix(LEGACY_HEADING_PREFIX)?
    };
    let detail = detail.trim();
    let owner_end = detail.strip_prefix('`')?.find('`')? + 1;
    let owner = detail.get(1..owner_end)?.to_string();
    let rest = detail.get(owner_end + 1..)?.trim_start();
    let item = rest
        .strip_prefix("returned work for queue item “")?
        .split_once('”')?
        .0
        .to_string();
    (!owner.is_empty() && !item.is_empty()).then_some((owner, item))
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
/// is not already blank — unless the line after the cell is content, in which
/// case that blank stays as the only separator (see
/// [`strip_completion_cells_matching`]). When that blank was already present
/// and the cell ends the exchange, the comparison form is one blank line
/// SHORTER than the original — callers compare for drift, where a missing
/// trailing blank is never prompt- or response-bearing.
/// Every other line, including a completion-shaped heading that is NOT followed
/// by an attested response (or legacy body) and marker, is preserved byte for byte.
pub fn strip_completion_cells(text: &str) -> String {
    strip_completion_cells_matching(text, |_| true)
}

/// Remove only structurally binary-attested completion cells selected by
/// `should_strip`.
///
/// Queue closeout uses this to replace the matching transient "awaiting
/// integration" notice with the authoritative integrated response while
/// preserving unrelated worker reports. Heading-shaped prose without the
/// attested response (or legacy body) and keyed marker is never presented to the
/// predicate or removed.
///
/// `#completioncellstripglue`: removal never glues the text above a cell to
/// the text below it. The blank separator before a cell is retired with it
/// only when the cell is followed by a blank line, a component/boundary
/// marker, or the end of the text; when content follows, that blank stays.
/// A cell an older binary glued between two content lines is replaced by one
/// blank line. Selected cells separated only by blank lines retire as one
/// group. No byte outside the removed cells and their own separator changes.
pub fn strip_completion_cells_matching(
    text: &str,
    mut should_strip: impl FnMut(&CompletionEvidence) -> bool,
) -> String {
    let evidence_lines: Vec<&str> = text.lines().collect();
    let ranges = completion_cell_line_ranges(text)
        .into_iter()
        .filter(|range| {
            completion_evidence_for_range(&evidence_lines, range)
                .is_some_and(|evidence| should_strip(&evidence))
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return text.to_string();
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    // `#completioncellstripglue`: selected cells separated only by blank lines
    // retire as one group, so the decision below sees the real neighbours.
    let mut groups: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match groups.last_mut() {
            Some(group)
                if lines[group.end..range.start]
                    .iter()
                    .all(|line| line.trim().is_empty()) =>
            {
                group.end = range.end;
            }
            _ => groups.push(range),
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for group in groups {
        for line in &lines[cursor..group.start] {
            out.push_str(line);
        }
        cursor = group.end;
        let next_needs_separator = lines
            .get(group.end)
            .is_some_and(|line| is_separable_content(line));
        let previous = group.start.checked_sub(1).map(|index| lines[index]);
        match previous {
            // The blank before the cell is the separator `report-complete`
            // inserted. It goes with the cell unless it is now the only thing
            // keeping the text above from gluing to the text below.
            Some(blank) if blank.trim().is_empty() => {
                if !next_needs_separator {
                    out.truncate(out.len() - blank.len());
                }
            }
            // A cell an older binary glued directly under content: never let
            // its removal join that content to the content below.
            Some(above) if next_needs_separator && is_separable_content(above) => {
                out.push_str(if above.ends_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                });
            }
            _ => {}
        }
    }
    for line in &lines[cursor..] {
        out.push_str(line);
    }
    out
}

/// The heading of a retired completion cell kept as the worker's hand-back
/// record ([`retain_completion_cells_as_handback_records`]).
pub const HANDBACK_RECORD_HEADING_PREFIX: &str = "### Re: Subagent hand-back: ";

/// The heading a kept hand-back record carries for `owner` and `item`.
pub fn handback_record_heading(owner: &str, item: &str) -> String {
    let owner = owner.replace('`', "'").replace('—', "-");
    let item = heading_item_text(item);
    format!("{HANDBACK_RECORD_HEADING_PREFIX}`{owner}` for queue item “{item}”")
}

/// Turn the attested completion cells selected by `keep` into plain
/// `agent:exchange` records of the worker's hand-back, in place
/// (`#handbackrowstructure`).
///
/// A struck queue row whose response tree carries only the salient part of a
/// worker hand-back points at the rest (What changed, Verification, Notes,
/// Next actions) in `agent:exchange`. Retiring that row's completion cell must
/// not delete the only full copy, yet the cell must stop being completion
/// evidence. A kept cell therefore loses its heading grammar, its response
/// digest, and its keyed marker: it becomes the heading from
/// [`handback_record_heading`] followed by the same quoted response lines,
/// byte for byte. Cells `keep` does not select, and every byte outside the
/// kept cells, are unchanged. Returns the text and the kept cells' evidence.
pub fn retain_completion_cells_as_handback_records(
    text: &str,
    mut keep: impl FnMut(&CompletionEvidence) -> bool,
) -> (String, Vec<CompletionEvidence>) {
    let evidence_lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut ranges = Vec::new();
    for range in completion_cell_line_ranges(text) {
        let Some(evidence) = completion_evidence_for_range(&evidence_lines, &range) else {
            continue;
        };
        let Some(digest_index) = completion_response_digest_index(&evidence_lines, range.start)
        else {
            continue;
        };
        if digest_index + 1 >= range.end.saturating_sub(1) || !keep(&evidence) {
            continue;
        }
        ranges.push((range, digest_index));
        kept.push(evidence);
    }
    if ranges.is_empty() {
        return (text.to_string(), kept);
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for ((range, digest_index), evidence) in ranges.iter().zip(&kept) {
        for line in &lines[cursor..range.start] {
            out.push_str(line);
        }
        let newline = if lines[range.start].ends_with("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        out.push_str(&handback_record_heading(&evidence.owner, &evidence.item));
        // `#exchangeresponseid`: the record keeps the response id its cell's
        // heading carried, so the row's tree still resolves to it.
        let cell_heading = lines[range.start].trim();
        let cell_heading = cell_heading.strip_suffix(" (HEAD)").unwrap_or(cell_heading);
        if let Some(response_id) = split_heading_response_id(cell_heading).1 {
            out.push(' ');
            out.push_str(response_id);
        }
        out.push_str(newline);
        // The quoted response lines, without the digest and the keyed marker.
        for line in &lines[digest_index + 1..range.end - 1] {
            out.push_str(line);
        }
        let marker = lines[range.end - 1];
        if !marker.ends_with('\n') && out.ends_with('\n') {
            out.truncate(out.len() - if out.ends_with("\r\n") { 2 } else { 1 });
        }
        cursor = range.end;
    }
    for line in &lines[cursor..] {
        out.push_str(line);
    }
    (out, kept)
}

/// A line that must stay a separate markdown block from an adjacent content
/// line: anything except a blank line or an agent-doc component/boundary
/// marker, which delimit blocks on their own.
fn is_separable_content(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with("<!-- agent:") && !line.starts_with("<!-- /agent:")
}

/// `current` without the binary-attested completion cells that `baseline`
/// does not already carry (matched by keyed marker).
///
/// `#statussynccommitrefusal`: `queue report-complete` publishes its cell to
/// the current authority without attributing it to the document baseline, so
/// a later focused write (`status set`) leaves the baseline exactly "current
/// minus the pending cells". Comparisons that ask whether the baseline is a
/// faithful projection of current authority use this form so pending binary
/// feedback is never mistaken for agent or operator drift. A cell whose body
/// was edited is no longer attested and is therefore kept.
pub fn strip_completion_cells_absent_from(current: &str, baseline: &str) -> String {
    strip_completion_cells_matching(current, |evidence| {
        !baseline.contains(&marker(&evidence.key))
    })
}

/// Whether the ONLY difference between `baseline` and `current` is one or more
/// binary-attested completion cells that `baseline` does not yet carry.
///
/// Blank lines are ignored in the comparison because [`strip_completion_cells`]
/// removes a cell's blank separator, which may already have been present.
pub fn only_pending_completion_cells_between(baseline: &str, current: &str) -> bool {
    if baseline == current {
        return false;
    }
    let stripped = strip_completion_cells_absent_from(current, baseline);
    if stripped == current {
        return false;
    }
    let non_blank = |text: &str| -> Vec<String> {
        crate::transient_markers::normalize_transient_agent_doc_markers(text)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    };
    non_blank(&stripped) == non_blank(baseline)
}

/// Copy binary-attested completion cells present in `source` but absent from
/// `base` into `base`'s exchange component.
///
/// This is deliberately narrower than a document merge: only cells whose
/// heading, response digest, and keyed marker pass
/// [`completion_cell_line_ranges`] are eligible. Operator prose and every
/// other component remain byte-for-byte owned by `base`. The cells are
/// inserted before the exchange boundary when one exists, matching
/// `queue report-complete` projection semantics.
pub fn merge_missing_attested_completion_cells(base: &str, source: &str) -> Option<String> {
    merge_missing_attested_completion_cells_matching(base, source, |_| true)
}

/// Predicate-filtered form of [`merge_missing_attested_completion_cells`].
///
/// Callers that own queue lifecycle use this to avoid resurrecting a feedback
/// cell after its matching work row was deliberately closed in `base`.
pub fn merge_missing_attested_completion_cells_matching(
    base: &str,
    source: &str,
    mut should_merge: impl FnMut(&CompletionEvidence) -> bool,
) -> Option<String> {
    let base_components = agent_doc_element::element::parse(base).ok()?;
    let source_components = agent_doc_element::element::parse(source).ok()?;
    let mut base_exchanges = base_components
        .iter()
        .filter(|component| component.name == "exchange");
    let base_exchange = base_exchanges.next()?;
    if base_exchanges.next().is_some() {
        return None;
    }
    let mut source_exchanges = source_components
        .iter()
        .filter(|component| component.name == "exchange");
    let source_exchange = source_exchanges.next()?;
    if source_exchanges.next().is_some() {
        return None;
    }

    let base_body = base_exchange.content(base);
    let source_body = source_exchange.content(source);
    let source_lines = source_body.lines().collect::<Vec<_>>();
    let additions = completion_cell_line_ranges(source_body)
        .into_iter()
        .filter_map(|range| {
            let evidence = completion_evidence_for_range(&source_lines, &range)?;
            (should_merge(&evidence) && !base_body.contains(&marker(&evidence.key)))
                .then(|| source_lines[range].join("\n"))
        })
        .collect::<Vec<_>>();
    if additions.is_empty() {
        return None;
    }
    let additions = additions.join("\n\n");
    let boundary_id = base_body.lines().find_map(|line| {
        line.trim()
            .strip_prefix("<!-- agent:boundary:")?
            .strip_suffix(" -->")
            .filter(|id| !id.is_empty())
    });
    Some(match boundary_id {
        Some(boundary_id) => base_exchange.append_with_boundary(base, &additions, boundary_id),
        None => base_exchange.append_with_caret(base, &additions, None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_cell_with_structured_response(
        owner: &str,
        item: &str,
        key: &str,
        response: &str,
    ) -> String {
        let mut body = String::from("> **Subagent response:**\n>");
        for line in response.lines() {
            body.push('\n');
            body.push('>');
            if !line.is_empty() {
                body.push(' ');
                body.push_str(line);
            }
        }
        let digest = agent_doc_hash::content_hash(&body);
        format!(
            "{}\n{RESPONSE_DIGEST_PREFIX}{digest} -->\n{body}\n{}",
            heading(owner, item),
            marker(key),
        )
    }

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
    fn current_cell_carries_and_attests_the_quoted_worker_response() {
        assert!(cell_with_response("subagent:a", "finish a", "k1", " \n ").is_err());
        let current = cell_with_response(
            "subagent:a",
            "finish a",
            "k1",
            "Implemented the fix.\n\n### Tests\n\n- 4 passed",
        )
        .unwrap();
        assert!(current.contains("> Implemented the fix."));
        assert!(current.contains("> ### Tests"));
        assert!(current.contains("> - 4 passed"));
        assert_eq!(completion_cell_line_ranges(&current), vec![0..10]);
        assert_eq!(completion_evidence(&current)[0].key, "k1");
        assert_eq!(
            completion_response(&current, "k1").as_deref(),
            Some("Implemented the fix.\n\n### Tests\n\n- 4 passed")
        );
        assert!(completion_response(&current, "other-key").is_none());

        let edited = current.replace("> - 4 passed", "> - 5 passed");
        assert!(
            completion_cell_line_ranges(&edited).is_empty(),
            "edited worker prose must not retain binary completion authority"
        );
        assert!(completion_response(&edited, "k1").is_none());
        assert!(completion_response(&cell("subagent:a", "finish a", "k1"), "k1").is_none());
    }

    #[test]
    fn current_cell_excludes_strict_template_transport_from_response_structure() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "**Summary:** implemented the fix\n",
            "**Verification:** focused tests passed\n",
            "<!-- /patch:exchange -->\n",
            "<!-- patch:recap -->\n",
            "Parent-session recap only.\n",
            "<!-- /patch:recap -->\n",
        );
        let current = cell_with_response("subagent:a", "finish a", "k1", response).unwrap();
        let recorded = completion_response(&current, "k1").unwrap();

        assert!(recorded.contains("**Summary**\n\nimplemented the fix"));
        assert!(!recorded.contains("patch:exchange"), "{recorded}");
        assert!(!recorded.contains("patch:recap"), "{recorded}");
        assert!(!recorded.contains("Parent-session recap"), "{recorded}");
    }

    #[test]
    fn legacy_attested_cell_transport_normalizes_and_reattests() {
        let legacy_response = concat!(
            "**Summary**\n\n",
            "<!-- patch:exchange -->\n",
            "implemented the fix\n\n",
            "**Verification**\n\n",
            "focused tests passed\n",
            "<!-- /patch:exchange -->\n",
            "<!-- patch:recap -->\n",
            "Parent-session recap only.\n",
            "<!-- /patch:recap -->",
        );
        let legacy =
            legacy_cell_with_structured_response("subagent:a", "finish a", "k1", legacy_response);
        assert!(
            legacy.contains("patch:recap"),
            "fixture must reproduce legacy structure"
        );

        let normalized = normalize_completion_response_transport(&legacy).unwrap();
        assert!(!normalized.contains("patch:exchange"), "{normalized}");
        assert!(!normalized.contains("patch:recap"), "{normalized}");
        assert!(!normalized.contains("Parent-session recap"), "{normalized}");
        assert_eq!(completion_evidence(&normalized)[0].key, "k1");
        assert!(completion_response(&normalized, "k1").is_some());
        assert_eq!(
            normalize_completion_response_transport(&normalized).unwrap(),
            normalized
        );
    }

    /// `#subagentcompletioncell`: refreshing a cell that an older binary glued
    /// directly under a response separates it; an already separated cell, or
    /// one directly under a component marker, is replaced in place.
    #[test]
    fn replacing_a_glued_cell_separates_it_from_the_response_above() {
        let old = cell_with_response("subagent:a", "a", "k", "old").unwrap();
        let new = cell_with_response("subagent:a", "a", "k", "new").unwrap();
        let glued = format!("### Re: r — m\n\nShould I push?\n{old}\n<!-- agent:boundary:b -->\n");
        let replaced = replace_attested_completion_cell(&glued, "k", &new)
            .unwrap()
            .unwrap();
        assert_eq!(
            replaced,
            format!("### Re: r — m\n\nShould I push?\n\n{new}\n<!-- agent:boundary:b -->\n")
        );
        for before in [
            "### Re: r — m\n\nShould I push?\n\n",
            "<!-- agent:exchange -->\n",
            "",
        ] {
            let text = format!("{before}{old}\n");
            assert_eq!(
                replace_attested_completion_cell(&text, "k", &new)
                    .unwrap()
                    .unwrap(),
                format!("{before}{new}\n")
            );
        }
    }

    #[test]
    fn replaces_only_the_attested_completion_cell_with_the_same_key() {
        let old = cell_with_response("subagent:a", "finish a", "k1", "Tests: 4 passed.").unwrap();
        let other = cell_with_response("subagent:b", "finish b", "k2", "Tests: 2 passed.").unwrap();
        let replacement =
            cell_with_response("subagent:a", "finish a", "k1", "Tests: 9 passed.").unwrap();
        let text = format!("before\n{old}\n{other}\nafter\n");

        let updated = replace_attested_completion_cell(&text, "k1", &replacement)
            .unwrap()
            .expect("matching attested cell");
        assert!(updated.contains("Tests: 9 passed."));
        assert!(!updated.contains("Tests: 4 passed."));
        assert!(updated.contains("Tests: 2 passed."));
        assert!(updated.ends_with("after\n"));
        assert!(replace_attested_completion_cell(&text, "missing", &replacement).is_err());
    }

    /// `#completion-cell-queue-echo`: an older canonicalizer could place the
    /// queue prompt echo between the binary heading and its response digest.
    /// The cell remains binary evidence only when the echo names the same item
    /// and the worker response still satisfies its digest.
    #[test]
    fn recognizes_attested_completion_with_matching_queue_prompt_echo() {
        let item = "enforce hidden-game visibility before credential signing";
        let current = cell_with_response(
            "subagent:game-visibility",
            item,
            "k1",
            "Implemented and verified.",
        )
        .unwrap();
        let with_echo = current.replacen(
            "\n<!-- agent-doc-subagent-response",
            &format!(
                "\n\n> **Queue prompt:**\n>\n> {item} ⟦local⟧\n\n<!-- agent-doc-subagent-response"
            ),
            1,
        );

        assert_eq!(completion_evidence(&with_echo)[0].item, item);
        assert_eq!(
            completion_response(&with_echo, "k1").as_deref(),
            Some("Implemented and verified.")
        );
        assert_eq!(strip_completion_cells(&with_echo), "");

        let mismatched = with_echo.replace(
            "> enforce hidden-game visibility before credential signing ⟦local⟧",
            "> publish unrelated work ⟦local⟧",
        );
        assert!(completion_cell_line_ranges(&mismatched).is_empty());
        let edited = with_echo.replace("> Implemented and verified.", "> Edited by operator.");
        assert!(completion_cell_line_ranges(&edited).is_empty());
    }

    /// `#handbackrowstructure`: a kept cell stays in place as a plain record
    /// of the hand-back; it is no longer completion evidence, and the other
    /// cell and every surrounding byte are unchanged.
    #[test]
    fn a_kept_completion_cell_becomes_a_plain_handback_record() {
        let kept = cell_with_response(
            "subagent:sample",
            "do [#sample]",
            "k1",
            "**Summary**\n\nDone.\n\n**What changed**\n\n- `a.rs`",
        )
        .unwrap();
        let other = cell_with_response("subagent:other", "do [#other]", "k2", "Other.").unwrap();
        let text = format!("Above.\n\n{kept}\n\n{other}\n\nBelow.\n");
        let (out, evidence) =
            retain_completion_cells_as_handback_records(&text, |cell| cell.key == "k1");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].key, "k1");
        assert!(
            out.contains("### Re: Subagent hand-back: `subagent:sample` for queue item “do [#sample]”\n> **Subagent response:**\n>\n> **Summary**\n"),
            "{out}"
        );
        assert!(
            out.contains("> - `a.rs`\n\n### Re: Subagent completed"),
            "{out}"
        );
        assert!(!out.contains(&marker("k1")), "{out}");
        assert!(out.contains(&marker("k2")), "{out}");
        let remaining = completion_evidence(&out);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].key, "k2");
        assert!(
            out.starts_with("Above.\n\n") && out.ends_with("\n\nBelow.\n"),
            "{out}"
        );
        let (again, none) =
            retain_completion_cells_as_handback_records(&out, |cell| cell.key == "k1");
        assert_eq!(again, out);
        assert!(none.is_empty());
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

    /// `#exchangeresponseid`: a heading tagged with its tree's response id is
    /// still the same attested cell; the quoted item's own id is not the
    /// cell's.
    #[test]
    fn a_tagged_heading_stays_an_attested_cell() {
        let item = "`[#r-35be26482ddb]` was not printed";
        let current = format!(
            "{}\n",
            cell_with_response("subagent:a", item, "k1", "Done.").unwrap()
        );
        assert_eq!(
            split_heading_response_id(current.lines().next().unwrap()).1,
            None
        );
        let tagged = annotate_completion_cell_response_id(&current, "k1", "[#r-0123456789ab]")
            .expect("tagged");
        let heading = tagged.lines().next().unwrap();
        assert!(
            heading.ends_with(" — agent-doc [#r-0123456789ab]"),
            "{heading}"
        );
        assert!(is_completion_heading(heading));
        assert_eq!(completion_evidence(&tagged), completion_evidence(&current));
        assert_eq!(completion_response(&tagged, "k1").as_deref(), Some("Done."));
        assert_eq!(
            completion_cell_response_ids(&tagged)
                .get("k1")
                .map(String::as_str),
            Some("[#r-0123456789ab]")
        );
        assert!(annotate_completion_cell_response_id(&tagged, "k1", "[#r-0123456789ab]").is_none());
        assert!(
            annotate_completion_cell_response_id(&current, "other", "[#r-0123456789ab]").is_none()
        );

        let (record, kept) = retain_completion_cells_as_handback_records(&tagged, |_| true);
        assert_eq!(kept.len(), 1);
        let records = handback_records(&record);
        assert_eq!(records.len(), 1, "{record}");
        assert_eq!(records[0].owner, "subagent:a");
        assert_eq!(records[0].item, item);
        assert_eq!(records[0].response_id.as_deref(), Some("[#r-0123456789ab]"));
        assert_eq!(records[0].response, "Done.");

        let (untagged, _) = retain_completion_cells_as_handback_records(&current, |_| true);
        let records = handback_records(&untagged);
        assert_eq!(records[0].response_id, None);
        let tagged_record =
            annotate_handback_record_response_id(&untagged, records[0].line, "[#r-0123456789ab]")
                .expect("record tagged");
        assert_eq!(tagged_record, record);
    }

    #[test]
    fn parses_current_and_legacy_attested_completion_evidence() {
        let current = format!("{}\n", cell("subagent:a", "finish a", "k1"));
        assert_eq!(
            completion_evidence(&current),
            vec![CompletionEvidence {
                owner: "subagent:a".into(),
                item: "finish a".into(),
                key: "k1".into(),
            }]
        );
        assert_eq!(completion_evidence(LEGACY_FOUR).len(), 4);
        assert_eq!(
            completion_evidence(LEGACY_FOUR)[0].item,
            "#gh-fix https://github.com/example/project/issues/262"
        );
        assert!(completion_evidence("### Re: Subagent completed: `subagent:a` returned work for queue item “a” — agent-doc\nnot binary\n").is_empty());
    }

    #[test]
    fn matching_strip_retires_only_the_integrated_cell() {
        let first = cell("subagent:a", "finish a", "k1");
        let second = cell("subagent:b", "finish b", "k2");
        let forged = concat!(
            "### Re: Subagent completed: `subagent:c` returned work for queue item “finish c” — agent-doc\n",
            "operator-authored body\n",
            "<!-- agent-doc-subagent-completion key=k3 -->\n",
        );
        let text = format!("prior\n\n{first}\n\n{second}\n\n{forged}tail\n");

        let stripped =
            strip_completion_cells_matching(&text, |evidence| evidence.item == "finish a");

        assert!(!stripped.contains("key=k1"));
        assert!(stripped.contains("key=k2"));
        assert!(stripped.contains("key=k3"));
        assert!(stripped.contains("operator-authored body"));
        assert_eq!(completion_evidence(&stripped).len(), 1);
    }

    /// `#completioncellstripglue`: retiring a cell never joins the text above
    /// it to the text below it, whichever side the cell was glued on.
    #[test]
    fn stripping_a_cell_between_two_paragraphs_keeps_them_separate() {
        let c = cell_with_response("subagent:a", "a", "k", "Done.").unwrap();
        // Separated above (the report-complete shape), response glued below.
        let glued_below = format!("Above.\n\n{c}\nBelow.\n");
        assert_eq!(strip_completion_cells(&glued_below), "Above.\n\nBelow.\n");
        // Glued on both sides by an older binary.
        let glued_both = format!("Above.\n{c}\nBelow.\n");
        assert_eq!(strip_completion_cells(&glued_both), "Above.\n\nBelow.\n");
        // Glued above only: the blank below is already the separator.
        let glued_above = format!("Above.\n{c}\n\nBelow.\n");
        assert_eq!(strip_completion_cells(&glued_above), "Above.\n\nBelow.\n");
        // CRLF documents keep their line ending in the inserted separator.
        let crlf = format!("Above.\r\n{}\r\nBelow.\r\n", c.replace('\n', "\r\n"));
        assert_eq!(strip_completion_cells(&crlf), "Above.\r\n\r\nBelow.\r\n");
        // Two selected cells with only blanks between them retire as one.
        let d = cell_with_response("subagent:b", "b", "k2", "Done b.").unwrap();
        let pair = format!("Above.\n\n{c}\n\n{d}\nBelow.\n");
        assert_eq!(strip_completion_cells(&pair), "Above.\n\nBelow.\n");
        let pair_glued = format!("Above.\n{c}\n{d}\nBelow.\n");
        assert_eq!(strip_completion_cells(&pair_glued), "Above.\n\nBelow.\n");
    }

    /// `#completioncellstripglue`: the live shape from game-sessions.md
    /// (6d1bc843): a cell glued under the closing line of one response and
    /// above the next response's prose. Removal must not make the two read as
    /// one paragraph.
    #[test]
    fn stripping_the_live_glued_cell_keeps_the_response_paragraphs_apart() {
        let c = cell_with_response(
            "subagent:api-797-commitpush",
            "do [#commitpush-m3rd]",
            "k",
            "Verified.",
        )
        .unwrap();
        let text = format!(
            "auto-compaction is not enabled.\n{c}\n❯ PR #797 is repaired.\n\n❯ The broader work remains blocked.\n<!-- agent:boundary:6f886467 -->\n<!-- /agent:exchange -->\n"
        );
        assert_eq!(
            strip_completion_cells(&text),
            "auto-compaction is not enabled.\n\n❯ PR #797 is repaired.\n\n❯ The broader work remains blocked.\n<!-- agent:boundary:6f886467 -->\n<!-- /agent:exchange -->\n"
        );
    }

    /// `#completioncellstripglue`: at the end of the exchange the separator
    /// `report-complete` inserted goes with the cell, restoring the bytes the
    /// append started from.
    #[test]
    fn stripping_a_cell_at_the_end_of_the_exchange_restores_the_pre_append_bytes() {
        let c = cell_with_response("subagent:a", "a", "k", "Done.").unwrap();
        for tail in [
            "<!-- agent:boundary:b -->\n<!-- /agent:exchange -->\n",
            "<!-- /agent:exchange -->\n",
            "",
        ] {
            let before = format!("### Re: r — m\n\nLast line.\n{tail}");
            let appended = format!("### Re: r — m\n\nLast line.\n\n{c}\n{tail}");
            assert_eq!(strip_completion_cells(&appended), before, "tail={tail:?}");
            // Glued by an older binary: nothing to retire but the cell.
            let glued = format!("### Re: r — m\n\nLast line.\n{c}\n{tail}");
            assert_eq!(strip_completion_cells(&glued), before, "tail={tail:?}");
        }
        // No trailing newline after the cell.
        assert_eq!(
            strip_completion_cells(&format!("Last line.\n\n{c}")),
            "Last line.\n"
        );
    }

    /// `#completioncellstripglue`: a heading on either side of the cell stays
    /// its own block, so the prose above a removed cell never becomes the
    /// block directly above a `### Re:` heading (which reads as a prompt).
    #[test]
    fn stripping_a_cell_adjacent_to_a_heading_keeps_the_heading_separate() {
        let c = cell_with_response("subagent:a", "a", "k", "Done.").unwrap();
        let heading_below = format!("Should I push?\n\n{c}\n### Re: next — m\n\nBody.\n");
        assert_eq!(
            strip_completion_cells(&heading_below),
            "Should I push?\n\n### Re: next — m\n\nBody.\n"
        );
        let heading_below_glued = format!("Should I push?\n{c}\n### Re: next — m\n");
        assert_eq!(
            strip_completion_cells(&heading_below_glued),
            "Should I push?\n\n### Re: next — m\n"
        );
        let heading_above = format!("### Re: prior — m\n{c}\nBody.\n");
        assert_eq!(
            strip_completion_cells(&heading_above),
            "### Re: prior — m\n\nBody.\n"
        );
        // Directly under a component marker no separator is needed.
        let under_marker = format!("<!-- agent:exchange -->\n{c}\nBody.\n");
        assert_eq!(
            strip_completion_cells(&under_marker),
            "<!-- agent:exchange -->\nBody.\n"
        );
    }

    /// `#completioncellstripglue`: when blank lines already separate the cell
    /// on both sides, removal leaves exactly one of them and changes nothing
    /// else, and a text without a selected cell is returned unchanged.
    #[test]
    fn stripping_an_already_separated_cell_changes_no_other_byte() {
        let c = cell_with_response("subagent:a", "a", "k", "Done.").unwrap();
        let separated = format!("Above.\n\n{c}\n\nBelow.\n");
        assert_eq!(strip_completion_cells(&separated), "Above.\n\nBelow.\n");
        let wide = format!("Above.\n\n\n{c}\n\n\nBelow.\n");
        assert_eq!(strip_completion_cells(&wide), "Above.\n\n\n\nBelow.\n");
        let unselected = format!("Above.\n{c}\nBelow.\n");
        assert_eq!(
            strip_completion_cells_matching(&unselected, |_| false),
            unselected
        );
        assert_eq!(
            strip_completion_cells("Above.\nBelow.\n"),
            "Above.\nBelow.\n"
        );
    }

    /// `#statussynccommitrefusal`: the baseline is a faithful projection of
    /// current authority when the only difference is pending attested cells.
    #[test]
    fn only_pending_completion_cells_between_recognizes_unattributed_cells() {
        let attested = cell_with_response("subagent:a", "do [#a]", "ka", "Done a.").unwrap();
        let older = cell_with_response("subagent:b", "do [#b]", "kb", "Done b.").unwrap();
        let baseline =
            format!("### Re: x — claude\n\nDid it.\n{older}\n<!-- /agent:exchange -->\n");
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            &format!("\n{attested}\n<!-- /agent:exchange -->"),
        );
        assert!(only_pending_completion_cells_between(&baseline, &current));
        assert_eq!(
            strip_completion_cells_absent_from(&current, &baseline),
            baseline,
            "only the cell the baseline lacks is stripped"
        );

        // Identical text is not "pending cells only".
        assert!(!only_pending_completion_cells_between(&baseline, &baseline));
        // Any other drift beside the cell is real drift.
        let with_prompt = current.replace("Did it.", "Did it.\nnew operator prompt");
        assert!(!only_pending_completion_cells_between(
            &baseline,
            &with_prompt
        ));
        // An edited body loses its attestation, so it is drift, not feedback.
        let edited = current.replace("> Done a.", "> Done a, edited.");
        assert!(!only_pending_completion_cells_between(&baseline, &edited));
        // A cell the baseline already carries is not pending.
        assert!(!only_pending_completion_cells_between(&current, &baseline));
    }

    #[test]
    fn merges_only_missing_attested_cells_across_unrelated_snapshot_drift() {
        let cell = cell_with_response("subagent:a", "do [#a]", "ka", "Done a.").unwrap();
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: status — agent-doc\n\nSnapshot text.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:status -->\nSnapshot status.\n<!-- /agent:status -->\n",
        );
        let source = format!(
            "<!-- agent:exchange -->\n### Re: status — agent-doc\n\nLive text.\n\n{cell}\n<!-- agent:boundary:other -->\n<!-- /agent:exchange -->\n<!-- agent:status -->\nLive status.\n<!-- /agent:status -->\n"
        );

        let merged = merge_missing_attested_completion_cells(base, &source).unwrap();

        assert!(merged.contains("Snapshot text."));
        assert!(merged.contains("Snapshot status."));
        assert!(!merged.contains("Live text."));
        assert!(!merged.contains("Live status."));
        assert!(merged.contains("key=ka"));
        assert!(merged.find("key=ka").unwrap() < merged.find("boundary:keep").unwrap());
        assert!(merge_missing_attested_completion_cells(&merged, &source).is_none());

        let filtered =
            merge_missing_attested_completion_cells_matching(base, &source, |evidence| {
                evidence.item != "do [#a]"
            });
        assert!(filtered.is_none(), "a closed row can veto cell recovery");
    }
}
