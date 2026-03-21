use anyhow::{Context, Result};
use std::path::Path;
use uuid::Uuid;

use crate::component;
use crate::snapshot;

/// Boundary marker prefix used to identify insertion points in append-mode components.
pub const BOUNDARY_PREFIX: &str = "<!-- agent:boundary:";
pub const BOUNDARY_SUFFIX: &str = " -->";

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
pub fn find_in_component(doc: &str, comp: &component::Component, boundary_id: &str) -> Option<(usize, usize)> {
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
        let line_end = if marker_end < comp.close_start && doc.as_bytes().get(marker_end) == Some(&b'\n') {
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
#[allow(dead_code)]
pub fn find_boundary_id_in_component(doc: &str, comp: &component::Component) -> Option<String> {
    let content_region = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);
    let mut search_from = 0;
    while let Some(start) = content_region[search_from..].find(BOUNDARY_PREFIX) {
        let abs_start = comp.open_end + search_from + start;
        if code_ranges.iter().any(|&(cs, ce)| abs_start >= cs && abs_start < ce) {
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

/// Insert a boundary marker at the end of an append-mode component's content.
///
/// Any existing boundary markers in the component are removed first to prevent
/// orphaned markers from accumulating (e.g., after interrupted sessions).
///
/// Returns the generated boundary UUID and the updated document content.
pub fn insert(doc: &str, component_name: &str) -> Result<(String, String)> {
    // First, remove any stale boundary markers from the document
    let cleaned = remove_all(doc);
    let stale_count = doc.matches(BOUNDARY_PREFIX).count();
    if stale_count > 0 {
        eprintln!(
            "[boundary] removed {} stale boundary marker(s) before inserting new one",
            stale_count
        );
    }

    let components = component::parse(&cleaned)?;
    let comp = components
        .iter()
        .find(|c| c.name == component_name)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found", component_name))?;

    let id = Uuid::new_v4().to_string();
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
pub fn remove_all(doc: &str) -> String {
    let mut result = String::with_capacity(doc.len());
    for line in doc.lines() {
        if line.trim().starts_with(BOUNDARY_PREFIX) && line.trim().ends_with(BOUNDARY_SUFFIX) {
            // Skip boundary marker lines
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    // Preserve trailing content without final newline if original didn't have one
    if !doc.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// CLI entry point: insert a boundary marker and print the UUID.
pub fn run(file: &Path, component: Option<&str>) -> Result<()> {
    let component_name = component.unwrap_or("exchange");

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (id, updated) = insert(&content, component_name)?;

    // Atomic write
    let tmp = file.with_extension("boundary.tmp");
    std::fs::write(&tmp, &updated)
        .with_context(|| format!("failed to write temp file {}", tmp.display()))?;
    std::fs::rename(&tmp, file)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), file.display()))?;

    // Update snapshot so the boundary marker doesn't show up as a diff
    snapshot::save(file, &updated)?;

    println!("{}", id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let components = component::parse(&doc).unwrap();
        let comp = &components[0];
        let result = find_in_component(&doc, comp, id);
        assert!(result.is_some());
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
        assert!(!result.contains("stale-1"), "stale marker 1 should be removed");
        assert!(!result.contains("stale-2"), "stale marker 2 should be removed");

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
