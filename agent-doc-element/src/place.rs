//! Inline `agent:place` regions.
//!
//! The prompt is the body between the opening marker and one nested
//! `place:out` block. Writers replace only the bytes inside that block.
//! `place:out` is not an `agent:*` component, so a missing inner close stays a
//! local repair instead of an unclosed document component.

use crate::element::{self, Component};

pub const OUTPUT_OPEN_LABEL: &str = "place:out";
pub const OUTPUT_CLOSE_LABEL: &str = "/place:out";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    Open,
    Close,
}

#[derive(Debug, Clone, Copy)]
struct Marker {
    kind: MarkerKind,
    /// Start of the marker line, inclusive.
    line_start: usize,
    /// End of the marker line, exclusive, including its trailing newline.
    line_end: usize,
}

/// How the `place:out` delimiters inside one `agent:place` are shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceStructure {
    WellFormed,
    Missing,
    Unclosed,
    Duplicate,
    OrphanClose,
}

/// One `agent:place` region. Attribute legality is decided by the document
/// contract; this type only describes bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceRegion {
    pub open_start: usize,
    pub open_end: usize,
    pub close_start: usize,
    pub close_end: usize,
    pub open_line: usize,
    pub id: Option<String>,
    pub mode_attr: Option<String>,
    pub interval_attr: Option<String>,
    pub prompt_attr: Option<String>,
    /// Trimmed prose before the first `place:out` marker.
    pub body_prompt: String,
    /// Trimmed payload when the block is well-formed.
    pub output: Option<String>,
    pub structure: PlaceStructure,
    /// Absolute byte range of the output interior, exclusive of the marker lines.
    pub interior: Option<(usize, usize)>,
}

impl PlaceStructure {
    pub fn message(self, id: &str, line: usize) -> String {
        let which = match self {
            Self::WellFormed => return String::new(),
            Self::Missing => "has no `place:out` block",
            Self::Unclosed => "has an unclosed `place:out`",
            Self::Duplicate => "has more than one `place:out` block",
            Self::OrphanClose => "has an orphaned `/place:out`",
        };
        format!("`agent:place` id `{id}` at line {line} {which}")
    }
}

/// Every `agent:place` region in document order.
///
/// A document that does not parse is returned as that parse error. A missing
/// or duplicated `place:out` is reported on the region, not as a parse error.
pub fn regions(doc: &str) -> anyhow::Result<Vec<PlaceRegion>> {
    let components = element::parse(doc)?;
    let code = element::find_code_ranges(doc);
    Ok(components
        .into_iter()
        .filter(|component| component.name == "place")
        .map(|component| inspect(doc, &component, &code))
        .collect())
}

/// Replace the interior of `place:out` for `id`.
///
/// Returns the document unchanged when the trimmed payload already matches.
/// Refuses a missing id, a malformed block, or an answer that contains a
/// comment delimiter.
pub fn replace_output(doc: &str, id: &str, text: &str) -> anyhow::Result<String> {
    if text.contains("<!--") || text.contains("-->") {
        anyhow::bail!("place output must not contain comment delimiters; pass only the answer");
    }
    let regions = regions(doc)?;
    let matches: Vec<_> = regions
        .iter()
        .filter(|region| region.id.as_deref() == Some(id))
        .collect();
    if matches.len() > 1 {
        anyhow::bail!("`agent:place` id `{id}` is duplicated");
    }
    let Some(region) = matches.first() else {
        anyhow::bail!("no `agent:place` with id `{id}`");
    };
    if region.structure != PlaceStructure::WellFormed {
        anyhow::bail!(
            "{}",
            region.structure.message(
                region.id.as_deref().unwrap_or("(missing id)"),
                region.open_line
            )
        );
    }
    let (start, end) = region.interior.expect("well-formed place has an interior");
    let trimmed = text.trim();
    let replacement = if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    };
    if doc[start..end] == replacement {
        return Ok(doc.to_string());
    }
    let mut updated = String::with_capacity(doc.len() + replacement.len());
    updated.push_str(&doc[..start]);
    updated.push_str(&replacement);
    updated.push_str(&doc[end..]);
    Ok(updated)
}

/// Fold malformed `place:out` delimiters into one block per `agent:place`.
///
/// A well-formed document is `Ok(None)`. Bytes outside each place body are
/// unchanged. Nested `agent:place` regions are not rewritten.
pub fn repair(doc: &str) -> anyhow::Result<Option<String>> {
    let parsed = regions(doc)?;
    if parsed.iter().any(|region| {
        parsed.iter().any(|other| {
            other.open_start > region.open_start && other.close_end <= region.close_start
        })
    }) {
        anyhow::bail!("`agent:place` regions must not nest; place repair does not unnest them");
    }
    let mut next = doc.to_string();
    let mut changed = false;
    let mut ordered = parsed;
    ordered.sort_by_key(|region| std::cmp::Reverse(region.open_start));
    for region in ordered {
        if region.structure == PlaceStructure::WellFormed {
            continue;
        }
        let body = next[region.open_end..region.close_start].to_string();
        let code = element::find_code_ranges(&next);
        let markers = line_markers(&next, region.open_end, region.close_start, &code);
        let Some(repaired) = repair_body(&body, &markers, region.open_end) else {
            continue;
        };
        next.replace_range(region.open_end..region.close_start, &repaired);
        changed = true;
    }
    Ok(changed.then_some(next))
}

/// Drop well-formed `place:out` interiors so an output write is not a new
/// prompt. Malformed blocks are left visible.
pub fn blank_outputs(doc: &str) -> String {
    let Ok(parsed) = regions(doc) else {
        return doc.to_string();
    };
    let mut spans: Vec<(usize, usize)> = parsed
        .into_iter()
        .filter_map(|region| region.interior)
        .filter(|(start, end)| start < end)
        .collect();
    if spans.is_empty() {
        return doc.to_string();
    }
    spans.sort_unstable();
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    out
}

fn inspect(doc: &str, component: &Component, code: &[(usize, usize)]) -> PlaceRegion {
    let markers = line_markers(doc, component.open_end, component.close_start, code);
    let (structure, pair) = classify_markers(&markers);
    let prompt_end = markers
        .first()
        .map(|marker| marker.line_start)
        .unwrap_or(component.close_start);
    let body_prompt = doc[component.open_end..prompt_end].trim().to_string();
    let (output, interior) = if let Some((open, close)) = pair {
        let start = open.line_end;
        let end = close.line_start;
        (Some(doc[start..end].trim().to_string()), Some((start, end)))
    } else {
        (None, None)
    };
    PlaceRegion {
        open_start: component.open_start,
        open_end: component.open_end,
        close_start: component.close_start,
        close_end: component.close_end,
        open_line: doc[..component.open_start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        id: attr(&component.attrs, "id"),
        mode_attr: attr(&component.attrs, "mode"),
        interval_attr: attr(&component.attrs, "interval"),
        prompt_attr: attr(&component.attrs, "prompt"),
        body_prompt,
        output,
        structure,
        interior,
    }
}

fn attr(attrs: &std::collections::HashMap<String, String>, key: &str) -> Option<String> {
    attrs.get(key).cloned()
}

fn line_markers(doc: &str, start: usize, end: usize, code: &[(usize, usize)]) -> Vec<Marker> {
    let mut markers = Vec::new();
    let mut index = start;
    while index < end {
        let rest = &doc[index..end];
        let (line_end, content) = match rest.find('\n') {
            Some(nl) => (index + nl + 1, &rest[..nl]),
            None => (end, rest),
        };
        let in_code = code
            .iter()
            .any(|&(code_start, code_end)| index >= code_start && index < code_end);
        if !in_code && let Some(kind) = marker_kind(content.trim().trim_end_matches('\r')) {
            markers.push(Marker {
                kind,
                line_start: index,
                line_end,
            });
        }
        index = line_end;
    }
    markers
}

fn marker_kind(trimmed: &str) -> Option<MarkerKind> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    match inner {
        OUTPUT_OPEN_LABEL => Some(MarkerKind::Open),
        OUTPUT_CLOSE_LABEL => Some(MarkerKind::Close),
        _ => None,
    }
}

/// Last balanced pair, when the markers are exactly one pair.
fn classify_markers(markers: &[Marker]) -> (PlaceStructure, Option<(Marker, Marker)>) {
    let mut opens: Vec<Marker> = Vec::new();
    let mut pairs: Vec<(Marker, Marker)> = Vec::new();
    let mut orphans = 0usize;
    for marker in markers {
        match marker.kind {
            MarkerKind::Open => opens.push(*marker),
            MarkerKind::Close => {
                if let Some(open) = opens.pop() {
                    pairs.push((open, *marker));
                } else {
                    orphans += 1;
                }
            }
        }
    }
    let unclosed = opens.len();
    let structure = match (pairs.len(), unclosed, orphans) {
        (1, 0, 0) => PlaceStructure::WellFormed,
        (0, 0, 0) => PlaceStructure::Missing,
        (0, _, 0) => PlaceStructure::Unclosed,
        (0, 0, _) => PlaceStructure::OrphanClose,
        _ => PlaceStructure::Duplicate,
    };
    let pair = (structure == PlaceStructure::WellFormed).then(|| pairs[0]);
    (structure, pair)
}

fn repair_body(body: &str, markers: &[Marker], body_start: usize) -> Option<String> {
    let (structure, _) = classify_markers(markers);
    if structure == PlaceStructure::WellFormed {
        return None;
    }
    let relative: Vec<(usize, usize, MarkerKind)> = markers
        .iter()
        .map(|marker| {
            (
                marker.line_start - body_start,
                marker.line_end - body_start,
                marker.kind,
            )
        })
        .collect();
    let repaired = match structure {
        PlaceStructure::WellFormed => return None,
        PlaceStructure::Missing => append_block(body, ""),
        PlaceStructure::OrphanClose => {
            let stripped = delete_ranges(body, &relative);
            append_block(&stripped, "")
        }
        PlaceStructure::Unclosed
            if relative
                .iter()
                .filter(|marker| marker.2 == MarkerKind::Open)
                .count()
                == 1
                && relative.iter().all(|marker| marker.2 == MarkerKind::Open) =>
        {
            append_close(body)
        }
        PlaceStructure::Duplicate | PlaceStructure::Unclosed => {
            let payload = keeper_payload(body, &relative);
            let prompt = prompt_before_markers(body, &relative);
            append_block(&prompt, payload.trim())
        }
    };
    (repaired != body).then_some(repaired)
}

fn prompt_before_markers(body: &str, markers: &[(usize, usize, MarkerKind)]) -> String {
    let end = markers.first().map(|marker| marker.0).unwrap_or(body.len());
    body[..end].trim_end().to_string()
}

fn keeper_payload(body: &str, markers: &[(usize, usize, MarkerKind)]) -> String {
    let mut opens = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut last_unclosed: Option<usize> = None;
    for (start, end, kind) in markers {
        match kind {
            MarkerKind::Open => {
                opens.push((*start, *end));
                last_unclosed = Some(*end);
            }
            MarkerKind::Close => {
                if let Some((_, open_end)) = opens.pop() {
                    pairs.push((open_end, *start));
                    last_unclosed = opens.last().map(|open| open.1);
                }
            }
        }
    }
    let (start, end) = if let Some(&(start, end)) = pairs.last() {
        (start, end)
    } else if let Some(start) = last_unclosed {
        (start, body.len())
    } else {
        return String::new();
    };
    delete_marker_lines(&body[start..end]).trim().to_string()
}

fn delete_marker_lines(text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if marker_kind(line.trim().trim_end_matches('\r')).is_none() {
            out.push_str(line);
        }
    }
    out
}

fn delete_ranges(body: &str, ranges: &[(usize, usize, MarkerKind)]) -> String {
    let mut out = body.to_string();
    for (start, end, _) in ranges.iter().rev() {
        out.replace_range(*start..*end, "");
    }
    out.trim_end().to_string()
}

fn append_close(body: &str) -> String {
    let mut out = body.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("<!-- /place:out -->\n");
    out
}

fn append_block(prompt: &str, payload: &str) -> String {
    let mut out = prompt.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("<!-- place:out -->\n");
    if !payload.is_empty() {
        out.push_str(payload.trim_end());
        out.push('\n');
    }
    out.push_str("<!-- /place:out -->\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONCE: &str = "\
<!-- agent:place id=\"step-1\" -->
Rewrite the paragraph above as a numbered runbook. Keep the service names.
<!-- place:out -->
<!-- /place:out -->
<!-- /agent:place -->
";

    #[test]
    fn body_prompt_wins_over_the_quoted_attribute() {
        let doc = "\
<!-- agent:place id=\"step-1\" prompt=\"attribute\" -->
body prompt
<!-- place:out -->
<!-- /place:out -->
<!-- /agent:place -->
";
        let region = &regions(doc).unwrap()[0];
        assert_eq!(region.structure, PlaceStructure::WellFormed);
        assert_eq!(region.body_prompt, "body prompt");
        assert_eq!(region.prompt_attr.as_deref(), Some("attribute"));
        assert_eq!(region.output.as_deref(), Some(""));
    }

    #[test]
    fn replace_output_touches_only_the_interior() {
        let updated = replace_output(ONCE, "step-1", "1. Read the log.\n").unwrap();
        assert!(updated.contains("Rewrite the paragraph above"));
        assert!(updated.contains("1. Read the log.\n"));
        assert!(updated.contains("<!-- place:out -->\n1. Read the log.\n<!-- /place:out -->"));
        assert_eq!(
            replace_output(&updated, "step-1", "1. Read the log.").unwrap(),
            updated
        );
        let blanked_before = blank_outputs(ONCE);
        let blanked_after = blank_outputs(&updated);
        assert_eq!(blanked_before, blanked_after);
    }

    #[test]
    fn a_prompt_edit_survives_blanking() {
        let edited = ONCE.replace("Rewrite the paragraph", "Summarize the paragraph");
        assert_ne!(blank_outputs(ONCE), blank_outputs(&edited));
    }

    #[test]
    fn fenced_examples_are_not_output_blocks() {
        let doc = "\
<!-- agent:place id=\"step-1\" -->
```
<!-- place:out -->
example
<!-- /place:out -->
```
Rewrite it.
<!-- place:out -->
<!-- /place:out -->
<!-- /agent:place -->
";
        let region = &regions(doc).unwrap()[0];
        assert_eq!(region.structure, PlaceStructure::WellFormed);
        assert!(region.body_prompt.contains("Rewrite it."));
        assert_eq!(region.output.as_deref(), Some(""));
    }

    #[test]
    fn repair_inserts_a_missing_block_and_closes_an_open_one() {
        let missing = "\
<!-- agent:place id=\"step-1\" -->
Rewrite it.
<!-- /agent:place -->
";
        let repaired = repair(missing).unwrap().unwrap();
        assert_eq!(
            regions(&repaired).unwrap()[0].structure,
            PlaceStructure::WellFormed
        );
        assert_eq!(regions(&repaired).unwrap()[0].body_prompt, "Rewrite it.");
        assert!(repair(&repaired).unwrap().is_none());

        let unclosed = "\
<!-- agent:place id=\"step-1\" -->
Rewrite it.
<!-- place:out -->
draft
<!-- /agent:place -->
";
        let closed = repair(unclosed).unwrap().unwrap();
        let region = &regions(&closed).unwrap()[0];
        assert_eq!(region.structure, PlaceStructure::WellFormed);
        assert_eq!(region.output.as_deref(), Some("draft"));
    }

    #[test]
    fn repair_keeps_the_last_duplicate_payload() {
        let doc = "\
<!-- agent:place id=\"step-1\" -->
Rewrite it.
<!-- place:out -->
old
<!-- /place:out -->
<!-- place:out -->
new
<!-- /place:out -->
<!-- /agent:place -->
";
        let repaired = repair(doc).unwrap().unwrap();
        let region = &regions(&repaired).unwrap()[0];
        assert_eq!(region.structure, PlaceStructure::WellFormed);
        assert_eq!(region.output.as_deref(), Some("new"));
        assert_eq!(region.body_prompt, "Rewrite it.");
    }
}
