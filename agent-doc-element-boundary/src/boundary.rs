//! # Module: boundary
//!
//! ## Spec
//! - Manages `<!-- agent:boundary:UUID -->` markers that anchor IPC patch insertion points
//!   inside append-mode components (especially `exchange`).
//! - `insert` removes all stale boundary markers first, then appends a fresh UUID marker
//!   just before the component close tag.
//! - `remove` / `remove_all` strip markers from a document string without touching other content.
//! - `find_in_component` locates a specific boundary marker's byte range within a component,
//!   returning `(line_start, line_end)` for surgical replacement.
//! - `find_boundary_id_in_component` scans a component for any boundary marker, skipping matches
//!   inside fenced code blocks.
//! - CLI/file-IO adapters live outside this pure element crate.
//!
//! ## Agentic Contracts
//! - `new_id() -> String` — delegates to `new_boundary_id()`; guaranteed unique UUID.
//! - `format_marker(id) -> String` — produces `<!-- agent:boundary:ID -->`.
//! - `extract_id(marker) -> Option<&str>` — inverse of `format_marker`; returns trimmed ID.
//! - `insert(doc, component_name) -> Result<(String, String)>` — pure transform returning
//!   `(uuid, updated_doc)`; errors if the named component is not found.
//! - `remove_all(doc) -> String` — pure function; original trailing-newline behaviour preserved.
//!
//! ## Evals
//! - format_and_extract: `format_marker("abc-123")` → `"<!-- agent:boundary:abc-123 -->"`;
//!   `extract_id` round-trips back to `"abc-123"`
//! - insert_at_end: insert into `<!-- agent:exchange -->` component → marker appears between
//!   content and close tag
//! - stale_cleanup: document with two orphaned markers → `insert` removes both, leaves exactly one
//! - remove_all: two boundary lines in doc → both stripped, other lines preserved verbatim
//! - find_in_component: marker present → `Some((line_start, line_end))` within component bounds
//! - find_boundary_id: named component contains marker → `Some(id)`; ignores marker-looking text
//!   inside code ranges
//! - no_component: `insert` with unknown component name → `Err` containing component name

use anyhow::Result;

use agent_doc_element::element;

/// Boundary marker prefix used to identify insertion points in append-mode components.
pub const BOUNDARY_PREFIX: &str = "<!-- agent:boundary:";
pub const BOUNDARY_SUFFIX: &str = " -->";

/// Generate a new boundary ID (delegates to lib).
pub fn new_id() -> String {
    agent_doc_element::id::new_boundary_id()
}

/// Format a boundary marker comment.
pub fn format_marker(id: &str) -> String {
    format!("{}{}{}", BOUNDARY_PREFIX, id, BOUNDARY_SUFFIX)
}

/// Extract a boundary ID from a marker string, if present.
#[allow(dead_code)]
pub fn extract_id(marker: &str) -> Option<&str> {
    let trimmed = marker.trim();
    trimmed
        .strip_prefix(BOUNDARY_PREFIX)
        .and_then(|rest| rest.strip_suffix(BOUNDARY_SUFFIX))
        .map(|id| id.trim())
}

/// Find the byte offset of a boundary marker within a component's content.
///
/// Returns `Some((marker_line_start, marker_line_end))` — the byte range of
/// the entire line containing the marker (including trailing newline).
#[allow(dead_code)]
pub fn find_in_component(
    doc: &str,
    comp: &element::Component,
    boundary_id: &str,
) -> Option<(usize, usize)> {
    let content_region = &doc[comp.open_end..comp.close_start];
    let marker = format_marker(boundary_id);

    if let Some(rel_pos) = content_region.find(&marker) {
        let abs_pos = comp.open_end + rel_pos;

        // Find start of the line containing the marker
        let line_start = doc[..abs_pos]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(comp.open_end)
            .max(comp.open_end);

        // Find end of the line (including trailing newline)
        let marker_end = abs_pos + marker.len();
        let line_end =
            if marker_end < comp.close_start && doc.as_bytes().get(marker_end) == Some(&b'\n') {
                marker_end + 1
            } else {
                marker_end
            };

        Some((line_start, line_end.min(comp.close_start)))
    } else {
        None
    }
}

/// Find any boundary ID within a pre-parsed component.
///
/// Scans the component's content for any `<!-- agent:boundary:UUID -->` marker,
/// skipping matches inside code blocks. Returns the UUID if found.
pub fn find_boundary_id_in_component(doc: &str, comp: &element::Component) -> Option<String> {
    let content_region = &doc[comp.open_end..comp.close_start];
    let code_ranges = element::find_code_ranges(doc);
    let mut search_from = 0;
    while let Some(start) = content_region[search_from..].find(BOUNDARY_PREFIX) {
        let abs_start = comp.open_end + search_from + start;
        if code_ranges
            .iter()
            .any(|&(cs, ce)| abs_start >= cs && abs_start < ce)
        {
            search_from += start + BOUNDARY_PREFIX.len();
            continue;
        }
        let after_prefix = &content_region[search_from + start + BOUNDARY_PREFIX.len()..];
        if let Some(end) = after_prefix.find(BOUNDARY_SUFFIX) {
            let id = &after_prefix[..end];
            return Some(id.trim().to_string());
        }
        break;
    }
    None
}

/// Find any boundary ID within a named component.
///
/// Scans the named component's content for any `<!-- agent:boundary:UUID -->`
/// marker, skipping matches inside code ranges. Returns the UUID if found.
pub fn find_boundary_id(doc: &str, component_name: &str) -> Option<String> {
    let components = element::parse(doc).ok()?;
    let comp = components
        .iter()
        .find(|component| component.name == component_name)?;
    find_boundary_id_in_component(doc, comp)
}

/// Insert a boundary marker at the end of an append-mode component's content.
///
/// Any existing boundary markers in the component are removed first to prevent
/// orphaned markers from accumulating (e.g., after interrupted sessions).
///
/// Returns the generated boundary UUID and the updated document content.
pub fn insert(doc: &str, component_name: &str) -> Result<(String, String)> {
    // First, remove any stale boundary markers from the document
    let cleaned = remove_all(doc);

    let components = element::parse(&cleaned)?;
    let comp = components
        .iter()
        .find(|c| c.name == component_name)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found", component_name))?;

    let id = new_id();
    let marker = format_marker(&id);

    // Insert marker at the end of component content, just before the close tag
    let mut result = String::with_capacity(cleaned.len() + marker.len() + 2);
    let content = &cleaned[comp.open_end..comp.close_start];

    result.push_str(&cleaned[..comp.open_end]);
    result.push_str(content.trim_end());
    result.push('\n');
    result.push_str(&marker);
    result.push('\n');
    result.push_str(&cleaned[comp.close_start..]);

    Ok((id, result))
}

/// Remove a specific boundary marker from the document.
#[allow(dead_code)]
pub fn remove(doc: &str, boundary_id: &str) -> String {
    let marker_line = format!("{}\n", format_marker(boundary_id));
    doc.replace(&marker_line, "")
}

/// Remove all boundary markers from a document.
///
/// Markers are removed wherever they occur on a line, not only when the marker
/// owns the whole line. Interrupted editor patchback used to be able to append
/// duplicate markers directly to prompt text; leaving those inline comments in
/// place makes every later exchange split ambiguous. Marker-looking text inside
/// fenced code blocks is preserved.
pub fn remove_all(doc: &str) -> String {
    let mut result = String::with_capacity(doc.len());
    let code_ranges = element::find_code_ranges(doc);
    let mut absolute_line_start = 0;

    for line in doc.split_inclusive('\n') {
        let mut cleaned_line = String::with_capacity(line.len());
        let mut cursor = 0;
        let mut removed_marker = false;

        while let Some(relative_start) = line[cursor..].find(BOUNDARY_PREFIX) {
            let marker_start = cursor + relative_start;
            let absolute_marker_start = absolute_line_start + marker_start;
            if code_ranges
                .iter()
                .any(|&(start, end)| absolute_marker_start >= start && absolute_marker_start < end)
            {
                let prefix_end = marker_start + BOUNDARY_PREFIX.len();
                cleaned_line.push_str(&line[cursor..prefix_end]);
                cursor = prefix_end;
                continue;
            }

            let suffix_search_start = marker_start + BOUNDARY_PREFIX.len();
            let Some(relative_end) = line[suffix_search_start..].find(BOUNDARY_SUFFIX) else {
                break;
            };
            let marker_end = suffix_search_start + relative_end + BOUNDARY_SUFFIX.len();
            cleaned_line.push_str(&line[cursor..marker_start]);
            cursor = marker_end;
            removed_marker = true;
        }

        cleaned_line.push_str(&line[cursor..]);
        if !(removed_marker && cleaned_line.trim().is_empty()) {
            result.push_str(&cleaned_line);
        }
        absolute_line_start += line.len();
    }

    result
}

/// Count the real boundary markers in a document (`#boundaryprosecount`).
///
/// A raw `doc.matches(BOUNDARY_PREFIX).count()` cannot tell a marker from a
/// document that *describes* one. agent-doc's own bug-tracking sessions quote
/// the marker in inline code — `` `<!-- agent:boundary: -->` `` — so the
/// single-boundary invariant read a documented marker as an accreted one and
/// failed a healthy closeout that no collapse could ever repair: the collapse
/// masks code ranges (see [`remove_all`]) and correctly preserves that prose,
/// so the count never dropped and the self-heal bailed every time.
///
/// This masks the same code ranges [`remove_all`] preserves, so counting and
/// collapsing agree by construction. Prefer it over a substring count anywhere
/// the number of markers gates behavior.
pub fn count_markers(doc: &str) -> usize {
    let code_ranges = element::find_code_ranges(doc);
    let mut count = 0usize;
    let mut absolute_line_start = 0;

    for line in doc.split_inclusive('\n') {
        let mut cursor = 0;

        while let Some(relative_start) = line[cursor..].find(BOUNDARY_PREFIX) {
            let marker_start = cursor + relative_start;
            let prefix_end = marker_start + BOUNDARY_PREFIX.len();
            let absolute_marker_start = absolute_line_start + marker_start;
            if code_ranges
                .iter()
                .any(|&(start, end)| absolute_marker_start >= start && absolute_marker_start < end)
            {
                cursor = prefix_end;
                continue;
            }

            let Some(relative_end) = line[prefix_end..].find(BOUNDARY_SUFFIX) else {
                break;
            };
            count += 1;
            cursor = prefix_end + relative_end + BOUNDARY_SUFFIX.len();
        }

        absolute_line_start += line.len();
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_markers_ignores_prose_that_quotes_the_marker() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: one\n\n",
            "Response-block dedup normalizes `<!-- agent:boundary: -->`, the `(HEAD)` suffix.\n",
            "<!-- agent:boundary:real-one -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            doc.matches(BOUNDARY_PREFIX).count(),
            2,
            "the raw substring count is what regressed the invariant"
        );
        assert_eq!(count_markers(doc), 1);
    }

    #[test]
    fn count_markers_agrees_with_remove_all_on_accreted_markers() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "Prose about `<!-- agent:boundary: -->` markers.\n",
            "<!-- agent:boundary:stale-one -->\n",
            "### Re: two\n\n",
            "<!-- agent:boundary:stale-two -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(count_markers(doc), 2);
        assert_eq!(
            count_markers(&remove_all(doc)),
            0,
            "collapse and count must see the same marker set"
        );
        assert!(
            remove_all(doc).contains("`<!-- agent:boundary: -->`"),
            "collapse preserves the quoted prose the count now ignores"
        );
    }

    #[test]
    fn count_markers_ignores_fenced_marker_text() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "```markdown\n",
            "<!-- agent:boundary:example -->\n",
            "```\n",
            "<!-- agent:boundary:real-one -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(count_markers(doc), 1);
    }

    #[test]
    fn format_and_extract() {
        let id = "abc-123";
        let marker = format_marker(id);
        assert_eq!(marker, "<!-- agent:boundary:abc-123 -->");
        assert_eq!(extract_id(&marker), Some("abc-123"));
    }

    #[test]
    fn insert_at_end_of_component() {
        let doc = "before\n<!-- agent:exchange -->\nsome content\n<!-- /agent:exchange -->\nafter";
        let (id, result) = insert(doc, "exchange").unwrap();
        let marker = format_marker(&id);
        assert!(result.contains(&marker));
        assert!(result.contains("some content"));
        // Marker should be between content and close tag
        let marker_pos = result.find(&marker).unwrap();
        let close_pos = result.find("<!-- /agent:exchange -->").unwrap();
        assert!(marker_pos < close_pos);
    }

    #[test]
    fn find_boundary_in_component() {
        let id = "test-uuid";
        let doc = format!(
            "<!-- agent:exchange -->\ncontent\n{}\n<!-- /agent:exchange -->",
            format_marker(id)
        );
        let components = element::parse(&doc).unwrap();
        let comp = &components[0];
        let result = find_in_component(&doc, comp, id);
        assert!(result.is_some());
    }

    #[test]
    fn find_boundary_id_skips_code_blocks() {
        let content = "<!-- agent:exchange -->\n```\n<!-- agent:boundary:fake-id -->\n```\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert!(
            result.is_none(),
            "boundary inside code block must not be found, got: {:?}",
            result
        );
    }

    #[test]
    fn find_boundary_id_finds_real_marker() {
        let content = "<!-- agent:exchange -->\nSome text.\n<!-- agent:boundary:real-uuid-5678 -->\nMore text.\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert_eq!(result, Some("real-uuid-5678".to_string()));
    }

    #[test]
    fn remove_boundary() {
        let id = "test-uuid";
        let doc = format!(
            "<!-- agent:exchange -->\ncontent\n{}\n<!-- /agent:exchange -->",
            format_marker(id)
        );
        let cleaned = remove(&doc, id);
        assert!(!cleaned.contains("agent:boundary"));
        assert!(cleaned.contains("content"));
    }

    #[test]
    fn remove_all_boundaries() {
        let doc = "line1\n<!-- agent:boundary:aaa -->\nline2\n<!-- agent:boundary:bbb -->\nline3\n";
        let cleaned = remove_all(doc);
        assert_eq!(cleaned, "line1\nline2\nline3\n");
    }

    #[test]
    fn remove_all_boundaries_removes_inline_duplicates_but_preserves_code() {
        let doc = concat!(
            "prompt<!-- agent:boundary:first --><!-- agent:boundary:second -->\n",
            "  <!-- agent:boundary:standalone -->  \n",
            "```md\n",
            "example<!-- agent:boundary:literal -->\n",
            "```\n",
            "tail"
        );
        let cleaned = remove_all(doc);
        assert_eq!(
            cleaned,
            "prompt\n```md\nexample<!-- agent:boundary:literal -->\n```\ntail"
        );
    }

    #[test]
    fn insert_cleans_stale_markers() {
        // Simulate two orphaned boundary markers (from interrupted sessions)
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "some content\n",
            "<!-- agent:boundary:stale-1 -->\n",
            "more content\n",
            "<!-- agent:boundary:stale-2 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (new_id, result) = insert(doc, "exchange").unwrap();

        // Stale markers should be gone
        assert!(
            !result.contains("stale-1"),
            "stale marker 1 should be removed"
        );
        assert!(
            !result.contains("stale-2"),
            "stale marker 2 should be removed"
        );

        // New marker should be present
        let new_marker = format_marker(&new_id);
        assert!(result.contains(&new_marker), "new marker should be present");

        // Content should be preserved
        assert!(result.contains("some content"));
        assert!(result.contains("more content"));

        // Only one boundary marker should remain
        let marker_count = result.matches(BOUNDARY_PREFIX).count();
        assert_eq!(marker_count, 1, "should have exactly one boundary marker");
    }
}
