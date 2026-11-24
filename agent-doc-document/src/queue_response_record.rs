//! `#exchangeresponseid`: the binary-owned `agent:exchange` record of a queue
//! response tree that no exchange response carries.
//!
//! Every response tree a queue row shows (`**Response [#r-…]**`) has a
//! corresponding `agent:exchange` entry that carries the same id. A worker
//! hand-back's entry is its completion cell (or the hand-back record it is
//! retired to), tagged with the id. A strike that answers a row from a short
//! summary, a failure, a stop explanation, or an already-done match has no
//! exchange response of its own, so the strike writes this record:
//!
//! ```text
//! ### Re: Queue response: “<row text>” — agent-doc [#r-…]
//! <!-- agent-doc-queue-response sha256=<body hash> -->
//! > - **Summary:** Applied the three title edits.
//! > - **State:** Complete.
//! ```
//!
//! The heading follows the ordinary `### Re: topic — attribution [#r-…]`
//! shape, so the response index and `response-toc` resolve the id to it. The
//! digest makes the record structural evidence of binary authorship: drift
//! classifiers skip a record whose body still hashes to its digest
//! ([`record_heading_lines`]), exactly as they skip completion cells, and an
//! edited record is ordinary text again.

use std::ops::Range;

/// The heading prefix; the topic continues with the quoted row text.
pub const HEADING_PREFIX: &str = "### Re: Queue response: ";
/// The attribution that closes the heading topic, before the response id.
pub const HEADING_ATTRIBUTION_SUFFIX: &str = " — agent-doc";
/// The digest line that follows the heading.
pub const DIGEST_PREFIX: &str = "<!-- agent-doc-queue-response sha256=";

/// The record heading for queue row `item` and public `response_id`.
pub fn heading(item: &str, response_id: &str) -> String {
    let item = crate::subagent_completion::heading_item_text(item);
    format!("{HEADING_PREFIX}“{item}”{HEADING_ATTRIBUTION_SUFFIX} {response_id}")
}

/// The complete record (no trailing newline) for `item`, `response_id`, and
/// the tree's body lines (relative indent kept). `None` when the id is not a
/// public response id or the body is empty.
pub fn record(item: &str, response_id: &str, body_lines: &[String]) -> Option<String> {
    if !agent_doc_markdown_ast::exchange_tree::is_compact_response_id(response_id)
        || body_lines.iter().all(|line| line.trim().is_empty())
    {
        return None;
    }
    let body = body_lines
        .iter()
        .map(|line| {
            let line = line.trim_end();
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let digest = agent_doc_hash::content_hash(&body);
    Some(format!(
        "{}\n{DIGEST_PREFIX}{digest} -->\n{body}",
        heading(item, response_id)
    ))
}

fn is_record_heading(line: &str) -> bool {
    let line = line.trim();
    let line = line.strip_suffix(" (HEAD)").unwrap_or(line);
    let (base, id) = crate::subagent_completion::split_heading_response_id(line);
    id.is_some()
        && base
            .strip_prefix(HEADING_PREFIX)
            .and_then(|rest| rest.strip_suffix(HEADING_ATTRIBUTION_SUFFIX))
            .is_some_and(|topic| !topic.trim().is_empty())
}

fn digest(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(DIGEST_PREFIX)?
        .strip_suffix(" -->")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

/// Line ranges (0-based, over `text.lines()`) of every attested record:
/// heading, digest, and the quoted body that hashes to the digest.
pub fn record_line_ranges(text: &str) -> Vec<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !is_record_heading(lines[index]) {
            index += 1;
            continue;
        }
        let Some(expected) = lines.get(index + 1).copied().and_then(digest) else {
            index += 1;
            continue;
        };
        let body_start = index + 2;
        let body_end = body_start
            + lines[body_start..]
                .iter()
                .take_while(|line| line.trim_start().starts_with('>'))
                .count();
        let body = lines[body_start..body_end]
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join("\n");
        if body_end > body_start && agent_doc_hash::content_hash(&body) == expected {
            ranges.push(index..body_end);
            index = body_end;
        } else {
            index += 1;
        }
    }
    ranges
}

/// 0-based heading lines of every attested record in `text`.
pub fn record_heading_lines(text: &str) -> Vec<usize> {
    record_line_ranges(text)
        .into_iter()
        .map(|range| range.start)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "[#r-0123456789ab]";

    fn body() -> Vec<String> {
        vec![
            "- **Summary:** Applied the edits.".to_string(),
            "- **State:** Complete.".to_string(),
        ]
    }

    #[test]
    fn record_carries_the_id_on_its_heading_and_is_attested() {
        let record = record("#justdoit", ID, &body()).unwrap();
        let first = record.lines().next().unwrap();
        assert_eq!(
            first,
            "### Re: Queue response: “#justdoit” — agent-doc [#r-0123456789ab]"
        );
        assert_eq!(
            agent_doc_markdown_ast::exchange_tree::response_id_from_heading(first),
            Some(ID)
        );
        let text = format!("intro\n\n{record}\n\nafter\n");
        assert_eq!(record_heading_lines(&text), vec![2]);
        assert_eq!(record_line_ranges(&text), vec![2..6]);
    }

    #[test]
    fn an_edited_record_is_ordinary_text() {
        let record = record("#justdoit", ID, &body()).unwrap();
        let edited = record.replace("Applied", "Reverted");
        assert!(record_line_ranges(&edited).is_empty());
        let forged =
            "### Re: Queue response: “x” — agent-doc [#r-0123456789ab]\n> - **Summary:** hi\n";
        assert!(record_line_ranges(forged).is_empty());
    }

    #[test]
    fn a_record_needs_a_public_id_and_a_body() {
        assert!(record("x", "[#q-0123456789ab]", &body()).is_none());
        assert!(record("x", ID, &[String::new()]).is_none());
    }
}
