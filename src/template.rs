//! # Module: template
//!
//! Template-mode support for in-place response documents. Parses structured patch
//! blocks from agent responses and applies them to named component slots in the
//! document, with boundary marker lifecycle management for CRDT-safe stream writes.
//!
//! ## Spec
//! - `parse_patches`: Scans agent response text for `<!-- patch:name -->...<!-- /patch:name -->`
//!   blocks and returns a list of `PatchBlock` values plus any unmatched text (content outside
//!   patch blocks). Markers inside fenced code blocks (``` or ~~~) and inline code spans are
//!   ignored so examples in responses are never mis-parsed as real patches.
//! - `apply_patches`: Delegates to `apply_patches_with_overrides` with an empty override map.
//!   Applies parsed patches to matching `<!-- agent:name -->` components in the document.
//! - `apply_patches_with_overrides`: Core patch application pipeline:
//!   1. Pre-patch: strips all existing boundary markers, inserts a fresh boundary at the end
//!      of the `exchange` component (keyed to the file stem).
//!   2. Applies each patch using mode resolution: stream overrides > inline attr (`patch=` or
//!      `mode=`) > `.agent-doc/components.toml` > built-in defaults (`exchange`/`findings`
//!      default to `append`, all others to `replace`).
//!   3. For `append` mode, uses boundary-aware insertion when a boundary marker exists.
//!   4. Patches targeting missing component names are routed as overflow to `exchange`/`output`.
//!   5. Unmatched text and overflow are merged and appended to `exchange`/`output` (or an
//!      auto-created `exchange` component if none exists).
//!   6. Post-patch: if the boundary was consumed, re-inserts a fresh one at the end of exchange.
//! - `reposition_boundary_to_end`: Removes all boundaries and inserts a new one at the end of
//!   `exchange`. Used by the IPC write path to keep boundary position current.
//! - `reposition_boundary_to_end_with_summary`: Same, with optional human-readable suffix on
//!   the boundary ID (e.g. `a0cfeb34:agent-doc`).
//! - `template_info`: Reads a document file, resolves its template mode flag, and returns a
//!   serializable `TemplateInfo` with per-component name, resolved mode, content, and line
//!   number. Used by editor plugins for rendering.
//! - `is_template_mode` (test-only): Legacy helper to detect `mode = "template"` string.
//!
//! ## Agentic Contracts
//! - `parse_patches` is pure and infallible for valid UTF-8; it returns `Ok` even for empty
//!   or patch-free responses.
//! - Patch markers inside fenced code blocks are never extracted as real patches. Agents may
//!   include example markers in code blocks without triggering unintended writes.
//! - Component patches are applied in reverse document order so earlier byte offsets remain
//!   valid throughout the operation.
//! - A boundary marker always exists at the end of `exchange` after `apply_patches_with_overrides`
//!   returns. Callers that perform incremental (CRDT/stream) writes may rely on this invariant.
//! - Missing-component patches never cause errors; content is silently routed to `exchange`/
//!   `output` with a diagnostic written to stderr.
//! - Mode precedence is deterministic: stream override > inline attr (`patch=` > `mode=`) >
//!   `components.toml` (`patch` key > `mode` key) > built-in default. Callers can rely on this
//!   ordering when constructing overrides for stream mode.
//! - `template_info` reads the document from disk; callers must ensure the file is flushed
//!   before calling (especially in the IPC write path).
//!
//! ## Evals
//! - `parse_single_patch`: single patch block → one `PatchBlock`, empty unmatched
//! - `parse_multiple_patches`: two sequential patch blocks → two `PatchBlock`s in order, empty unmatched
//! - `parse_with_unmatched_content`: text before and after patch block → unmatched contains both text segments
//! - `parse_empty_response`: empty string → zero patches, empty unmatched
//! - `parse_no_patches`: plain text with no markers → zero patches, full text in unmatched
//! - `parse_patches_ignores_markers_in_fenced_code_block`: agent:component markers inside ``` are preserved as content
//! - `parse_patches_ignores_patch_markers_in_fenced_code_block`: nested patch markers inside ``` are not parsed as patches
//! - `parse_patches_ignores_markers_in_tilde_fence`: patch markers inside ~~~ are ignored
//! - `parse_patches_ignores_closing_marker_in_code_block`: closing marker inside code block is skipped; real close is found
//! - `parse_patches_normal_markers_still_work`: sanity — two back-to-back patches parse correctly
//! - `apply_patches_replace`: patch to non-exchange component replaces existing content
//! - `apply_patches_unmatched_creates_exchange`: unmatched text auto-creates `<!-- agent:exchange -->` when absent
//! - `apply_patches_unmatched_appends_to_existing_exchange`: unmatched text appends to existing exchange; no duplicate component
//! - `apply_patches_missing_component_routes_to_exchange`: patch targeting unknown component name appears in exchange
//! - `apply_patches_missing_component_creates_exchange`: missing component + no exchange → auto-creates exchange with overflow
//! - `inline_attr_mode_overrides_config`: `mode=replace` on tag wins over `components.toml` append config
//! - `inline_attr_mode_overrides_default`: `mode=replace` on exchange wins over built-in append default
//! - `no_inline_attr_falls_back_to_config`: no inline attr → `components.toml` append config applies
//! - `no_inline_attr_no_config_falls_back_to_default`: no attr, no config → exchange defaults to append
//! - `inline_patch_attr_overrides_config`: `patch=replace` on tag wins over `components.toml` append config
//! - `template_info_works`: template-mode doc → `TemplateInfo.template_mode = true`, component list populated
//! - `template_info_legacy_mode_works`: `response_mode: template` frontmatter key recognized
//! - `template_info_append_mode`: non-template doc → `template_mode = false`, empty component list
//! - `is_template_mode_detection`: `Some("template")` → true; other strings and `None` → false
//! - (aspirational) `apply_patches_boundary_invariant`: after any apply_patches call with an exchange component, a boundary marker exists at end of exchange
//! - (aspirational) `reposition_boundary_removes_stale`: multiple stale boundaries are reduced to exactly one at end of exchange

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::component::{self, find_comment_end, Component};

/// A parsed patch directive from an agent response.
#[derive(Debug, Clone)]
pub struct PatchBlock {
    pub name: String,
    pub content: String,
}

/// Template info output for plugins.
#[derive(Debug, Serialize)]
pub struct TemplateInfo {
    pub template_mode: bool,
    pub components: Vec<ComponentInfo>,
}

/// Per-component info for plugin rendering.
#[derive(Debug, Serialize)]
pub struct ComponentInfo {
    pub name: String,
    pub mode: String,
    pub content: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<usize>,
}

/// Check if a document is in template mode (deprecated — use `fm.resolve_mode().is_template()`).
#[cfg(test)]
pub fn is_template_mode(mode: Option<&str>) -> bool {
    matches!(mode, Some("template"))
}

/// Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks from an agent response.
///
/// Content outside patch blocks is collected as "unmatched" and returned separately.
/// Markers inside fenced code blocks (``` or ~~~) and inline code spans are ignored.
pub fn parse_patches(response: &str) -> Result<(Vec<PatchBlock>, String)> {
    let bytes = response.as_bytes();
    let len = bytes.len();
    let code_ranges = component::find_code_ranges(response);
    let mut patches = Vec::new();
    let mut unmatched = String::new();
    let mut pos = 0;
    let mut last_end = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            pos += 1;
            continue;
        }

        // Skip markers inside code regions
        if code_ranges.iter().any(|&(start, end)| pos >= start && pos < end) {
            pos += 4;
            continue;
        }

        let marker_start = pos;

        // Find closing -->
        let close = match find_comment_end(bytes, pos + 4) {
            Some(c) => c,
            None => {
                pos += 4;
                continue;
            }
        };

        let inner = &response[marker_start + 4..close - 3];
        let trimmed = inner.trim();

        if let Some(name) = trimmed.strip_prefix("patch:") {
            let name = name.trim();
            if name.is_empty() || name.starts_with('/') {
                pos = close;
                continue;
            }

            // Consume trailing newline after opening marker
            let mut content_start = close;
            if content_start < len && bytes[content_start] == b'\n' {
                content_start += 1;
            }

            // Collect unmatched text before this patch block
            let before = &response[last_end..marker_start];
            let trimmed_before = before.trim();
            if !trimmed_before.is_empty() {
                if !unmatched.is_empty() {
                    unmatched.push('\n');
                }
                unmatched.push_str(trimmed_before);
            }

            // Find the matching close: <!-- /patch:name --> (skipping code blocks)
            let close_marker = format!("<!-- /patch:{} -->", name);
            if let Some(close_pos) = find_outside_code(&close_marker, response, content_start, &code_ranges) {
                let content = &response[content_start..close_pos];
                patches.push(PatchBlock {
                    name: name.to_string(),
                    content: content.to_string(),
                });

                let mut end = close_pos + close_marker.len();
                if end < len && bytes[end] == b'\n' {
                    end += 1;
                }
                last_end = end;
                pos = end;
                continue;
            }
        }

        pos = close;
    }

    // Collect any trailing unmatched text
    if last_end < len {
        let trailing = response[last_end..].trim();
        if !trailing.is_empty() {
            if !unmatched.is_empty() {
                unmatched.push('\n');
            }
            unmatched.push_str(trailing);
        }
    }

    Ok((patches, unmatched))
}

/// Apply patch blocks to a document's components.
///
/// For each patch block, finds the matching `<!-- agent:name -->` component
/// and replaces its content. Uses patch.rs mode logic (replace/append/prepend)
/// based on `.agent-doc/components.toml` config.
///
/// Returns the modified document. Unmatched content (outside patch blocks)
/// is appended to `<!-- agent:output -->` if it exists, or creates one at the end.
pub fn apply_patches(doc: &str, patches: &[PatchBlock], unmatched: &str, file: &Path) -> Result<String> {
    apply_patches_with_overrides(doc, patches, unmatched, file, &std::collections::HashMap::new())
}

/// Apply patches with per-component mode overrides (e.g., stream mode forces "replace"
/// for cumulative buffers even on append-mode components like exchange).
pub fn apply_patches_with_overrides(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &std::collections::HashMap<String, String>,
) -> Result<String> {
    // Pre-patch: ensure a fresh boundary exists in the exchange component.
    // Remove any stale boundaries from previous cycles, then insert a new one
    // at the end of the exchange. This is deterministic — belongs in the binary,
    // not the SKILL workflow.
    let summary = file.file_stem().and_then(|s| s.to_str());
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = crate::new_boundary_id_with_summary(summary);
        let marker = crate::format_boundary_marker(&id);
        let content = exchange.content(&result);
        let new_content = format!("{}\n{}\n", content.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
        eprintln!("[template] pre-patch boundary {} inserted at end of exchange", id);
    }

    // Apply patches in reverse order (by position) to preserve byte offsets
    let components = component::parse(&result)
        .context("failed to parse components")?;

    // Load component configs
    let configs = load_component_configs(file);

    // Build a list of (component_index, patch) pairs, sorted by component position descending.
    // Patches targeting missing components are collected as overflow and routed to
    // exchange/output (same as unmatched content) — this avoids silent failures when
    // the agent uses a wrong component name.
    let mut ops: Vec<(usize, &PatchBlock)> = Vec::new();
    let mut overflow = String::new();
    for patch in patches {
        if let Some(idx) = components.iter().position(|c| c.name == patch.name) {
            ops.push((idx, patch));
        } else {
            let available: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
            eprintln!(
                "[template] patch target '{}' not found, routing to exchange/output. Available: {}",
                patch.name,
                available.join(", ")
            );
            if !overflow.is_empty() {
                overflow.push('\n');
            }
            overflow.push_str(&patch.content);
        }
    }

    // Sort by position descending so replacements don't shift earlier offsets
    ops.sort_by(|a, b| b.0.cmp(&a.0));

    for (idx, patch) in &ops {
        let comp = &components[*idx];
        // Mode precedence: stream overrides > inline attr > components.toml > built-in default
        let mode = mode_overrides.get(&patch.name)
            .map(|s| s.as_str())
            .or_else(|| comp.patch_mode())
            .or_else(|| configs.get(&patch.name).map(|s| s.as_str()))
            .unwrap_or_else(|| default_mode(&patch.name));
        // For append mode, use boundary-aware insertion when a marker exists
        if mode == "append"
            && let Some(bid) = find_boundary_in_component(&result, comp)
        {
            result = comp.append_with_boundary(&result, &patch.content, &bid);
            continue;
        }
        let new_content = apply_mode(mode, comp.content(&result), &patch.content);
        result = comp.replace_content(&result, &new_content);
    }

    // Merge overflow (from missing-component patches) with unmatched content
    let mut all_unmatched = String::new();
    if !overflow.is_empty() {
        all_unmatched.push_str(&overflow);
    }
    if !unmatched.is_empty() {
        if !all_unmatched.is_empty() {
            all_unmatched.push('\n');
        }
        all_unmatched.push_str(unmatched);
    }

    // Handle unmatched content
    if !all_unmatched.is_empty() {
        let unmatched = &all_unmatched;
        // Re-parse after patches applied
        let components = component::parse(&result)
            .context("failed to re-parse components after patching")?;

        if let Some(output_comp) = components.iter().find(|c| c.name == "exchange" || c.name == "output") {
            // Try boundary-aware append first (preserves prompt ordering)
            if let Some(bid) = find_boundary_in_component(&result, output_comp) {
                eprintln!("[template] unmatched content: using boundary {} for insertion", &bid[..bid.len().min(8)]);
                result = output_comp.append_with_boundary(&result, unmatched, &bid);
            } else {
                // No boundary — plain append to exchange/output component
                let existing = output_comp.content(&result);
                let new_content = if existing.trim().is_empty() {
                    format!("{}\n", unmatched)
                } else {
                    format!("{}{}\n", existing, unmatched)
                };
                result = output_comp.replace_content(&result, &new_content);
            }
        } else {
            // Auto-create exchange component at the end
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("\n<!-- agent:exchange -->\n");
            result.push_str(unmatched);
            result.push_str("\n<!-- /agent:exchange -->\n");
        }
    }

    // Post-patch: ensure a boundary exists at the end of the exchange component.
    // This is unconditional for template docs with an exchange — the boundary must
    // always exist for checkpoint writes to work. Checking the original doc's content
    // causes a snowball: once one cycle loses the boundary, every subsequent cycle
    // also loses it because the check always finds nothing.
    {
        if let Ok(components) = component::parse(&result)
            && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
            && find_boundary_in_component(&result, exchange).is_none()
        {
            // Boundary was consumed — re-insert at end of exchange
            let id = uuid::Uuid::new_v4().to_string();
            let marker = format!("<!-- agent:boundary:{} -->", id);
            let content = exchange.content(&result);
            let new_content = format!("{}\n{}\n", content.trim_end(), marker);
            result = exchange.replace_content(&result, &new_content);
            eprintln!("[template] re-inserted boundary {} at end of exchange", &id[..id.len().min(8)]);
        }
    }

    Ok(result)
}

/// Reposition the boundary marker to the end of the exchange component.
///
/// Removes all existing boundaries and inserts a fresh one at the end of
/// the exchange. This is the same pre-patch logic used in
/// `apply_patches_with_overrides()`, extracted for use by the IPC write path.
///
/// Returns the document unchanged if no exchange component exists.
pub fn reposition_boundary_to_end(doc: &str) -> String {
    reposition_boundary_to_end_with_summary(doc, None)
}

/// Reposition boundary with an optional human-readable summary suffix.
///
/// The summary is slugified and appended to the boundary ID:
/// `a0cfeb34:agent-doc` instead of just `a0cfeb34`.
pub fn reposition_boundary_to_end_with_summary(doc: &str, summary: Option<&str>) -> String {
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = crate::new_boundary_id_with_summary(summary);
        let marker = crate::format_boundary_marker(&id);
        let content = exchange.content(&result);
        let new_content = format!("{}\n{}\n", content.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
    }
    result
}

/// Remove all boundary markers from a document (line-level removal).
/// Skips boundaries inside fenced code blocks (lesson #13).
fn remove_all_boundaries(doc: &str) -> String {
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let code_ranges = component::find_code_ranges(doc);
    let in_code = |pos: usize| code_ranges.iter().any(|&(start, end)| pos >= start && pos < end);
    let mut result = String::with_capacity(doc.len());
    let mut offset = 0;
    for line in doc.lines() {
        let trimmed = line.trim();
        let is_boundary = trimmed.starts_with(prefix) && trimmed.ends_with(suffix);
        if is_boundary && !in_code(offset) {
            // Skip boundary marker lines outside code blocks
            offset += line.len() + 1; // +1 for newline
            continue;
        }
        result.push_str(line);
        result.push('\n');
        offset += line.len() + 1;
    }
    if !doc.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Find a boundary marker ID inside a component's content, skipping code blocks.
fn find_boundary_in_component(doc: &str, comp: &Component) -> Option<String> {
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let content_region = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);
    let mut search_from = 0;
    while let Some(start) = content_region[search_from..].find(prefix) {
        let abs_start = comp.open_end + search_from + start;
        if code_ranges.iter().any(|&(cs, ce)| abs_start >= cs && abs_start < ce) {
            search_from += start + prefix.len();
            continue;
        }
        let after_prefix = &content_region[search_from + start + prefix.len()..];
        if let Some(end) = after_prefix.find(suffix) {
            return Some(after_prefix[..end].trim().to_string());
        }
        break;
    }
    None
}

/// Get template info for a document (for plugin rendering).
pub fn template_info(file: &Path) -> Result<TemplateInfo> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, _body) = crate::frontmatter::parse(&doc)?;
    let template_mode = fm.resolve_mode().is_template();

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file);

    let component_infos: Vec<ComponentInfo> = components
        .iter()
        .map(|comp| {
            let content = comp.content(&doc).to_string();
            // Inline attr > components.toml > built-in default
            let mode = comp.patch_mode().map(|s| s.to_string())
                .or_else(|| configs.get(&comp.name).cloned())
                .unwrap_or_else(|| default_mode(&comp.name).to_string());
            // Compute line number from byte offset
            let line = doc[..comp.open_start].matches('\n').count() + 1;
            ComponentInfo {
                name: comp.name.clone(),
                mode,
                content,
                line,
                max_entries: None, // TODO: read from components.toml
            }
        })
        .collect();

    Ok(TemplateInfo {
        template_mode,
        components: component_infos,
    })
}

/// Load component mode configs from `.agent-doc/components.toml`.
/// Returns a map of component_name → mode string.
fn load_component_configs(file: &Path) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    let root = find_project_root(file);
    if let Some(root) = root {
        let config_path = root.join(".agent-doc/components.toml");
        if config_path.exists()
            && let Ok(content) = std::fs::read_to_string(&config_path)
            && let Ok(table) = content.parse::<toml::Table>()
        {
            for (name, value) in &table {
                // "patch" is the primary key; "mode" is a backward-compatible alias
                if let Some(mode) = value.get("patch").and_then(|v| v.as_str())
                    .or_else(|| value.get("mode").and_then(|v| v.as_str()))
                {
                    result.insert(name.clone(), mode.to_string());
                }
            }
        }
    }
    result
}

/// Default mode for a component by name.
/// `exchange` and `findings` default to `append`; all others default to `replace`.
fn default_mode(name: &str) -> &'static str {
    match name {
        "exchange" | "findings" => "append",
        _ => "replace",
    }
}

/// Apply mode logic (replace/append/prepend).
fn apply_mode(mode: &str, existing: &str, new_content: &str) -> String {
    match mode {
        "append" => format!("{}{}", existing, new_content),
        "prepend" => format!("{}{}", new_content, existing),
        _ => new_content.to_string(), // "replace" default
    }
}

fn find_project_root(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let mut dir = canonical.parent()?;
    loop {
        if dir.join(".agent-doc").is_dir() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Find `needle` in `haystack` starting at `from`, skipping occurrences inside code ranges.
/// Returns the byte offset of the match within `haystack` (absolute, not relative to `from`).
fn find_outside_code(needle: &str, haystack: &str, from: usize, code_ranges: &[(usize, usize)]) -> Option<usize> {
    let mut search_start = from;
    loop {
        let rel = haystack[search_start..].find(needle)?;
        let abs = search_start + rel;
        if code_ranges.iter().any(|&(start, end)| abs >= start && abs < end) {
            // Inside a code block — skip past this occurrence
            search_start = abs + needle.len();
            continue;
        }
        return Some(abs);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn parse_single_patch() {
        let response = "<!-- patch:status -->\nBuild passing.\n<!-- /patch:status -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "Build passing.\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_multiple_patches() {
        let response = "\
<!-- patch:status -->
All green.
<!-- /patch:status -->

<!-- patch:log -->
- New entry
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All green.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- New entry\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_with_unmatched_content() {
        let response = "Some free text.\n\n<!-- patch:status -->\nOK\n<!-- /patch:status -->\n\nTrailing text.\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert!(unmatched.contains("Some free text."));
        assert!(unmatched.contains("Trailing text."));
    }

    #[test]
    fn parse_empty_response() {
        let (patches, unmatched) = parse_patches("").unwrap();
        assert!(patches.is_empty());
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_no_patches() {
        let response = "Just a plain response with no patch blocks.";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert!(patches.is_empty());
        assert_eq!(unmatched, "Just a plain response with no patch blocks.");
    }

    #[test]
    fn apply_patches_replace() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("\nold\n"));
        assert!(result.contains("<!-- agent:status -->"));
    }

    #[test]
    fn apply_patches_unmatched_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches(doc, &[], "Extra info here", &doc_path).unwrap();
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Extra info here"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn apply_patches_unmatched_appends_to_existing_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches(doc, &[], "new stuff", &doc_path).unwrap();
        assert!(result.contains("previous"));
        assert!(result.contains("new stuff"));
        // Should not create a second exchange component
        assert_eq!(result.matches("<!-- agent:exchange -->").count(), 1);
    }

    #[test]
    fn apply_patches_missing_component_routes_to_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Missing component content should be routed to exchange
        assert!(result.contains("overflow data"), "missing patch content should appear in exchange");
        assert!(result.contains("previous"), "existing exchange content should be preserved");
    }

    #[test]
    fn apply_patches_missing_component_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Should auto-create exchange component
        assert!(result.contains("<!-- agent:exchange -->"), "should create exchange component");
        assert!(result.contains("overflow data"), "overflow content should be in exchange");
    }

    #[test]
    fn is_template_mode_detection() {
        assert!(is_template_mode(Some("template")));
        assert!(!is_template_mode(Some("append")));
        assert!(!is_template_mode(None));
    }

    #[test]
    fn template_info_works() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(info.template_mode);
        assert_eq!(info.components.len(), 1);
        assert_eq!(info.components[0].name, "status");
        assert_eq!(info.components[0].content, "content\n");
    }

    #[test]
    fn template_info_legacy_mode_works() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nresponse_mode: template\n---\n\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(info.template_mode);
    }

    #[test]
    fn template_info_append_mode() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nagent_doc_format: append\n---\n\n# Doc\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(!info.template_mode);
        assert!(info.components.is_empty());
    }

    #[test]
    fn parse_patches_ignores_markers_in_fenced_code_block() {
        let response = "\
<!-- patch:exchange -->
Here is how you use component markers:

```markdown
<!-- agent:exchange -->
example content
<!-- /agent:exchange -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("```markdown"));
        assert!(patches[0].content.contains("<!-- agent:exchange -->"));
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_patch_markers_in_fenced_code_block() {
        // Patch markers inside a code block should not be treated as real patches
        let response = "\
<!-- patch:exchange -->
Real content here.

```markdown
<!-- patch:fake -->
This is just an example.
<!-- /patch:fake -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1, "should only find the outer real patch");
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("<!-- patch:fake -->"), "code block content should be preserved");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_markers_in_tilde_fence() {
        let response = "\
<!-- patch:status -->
OK
<!-- /patch:status -->

~~~
<!-- patch:fake -->
example
<!-- /patch:fake -->
~~~
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        // Only the real patch should be found; the fake one inside ~~~ is ignored
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
    }

    #[test]
    fn parse_patches_ignores_closing_marker_in_code_block() {
        // The closing marker for a real patch is inside a code block,
        // so the parser should skip it and find the real closing marker outside
        let response = "\
<!-- patch:exchange -->
Example:

```
<!-- /patch:exchange -->
```

Real content continues.
<!-- /patch:exchange -->
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("Real content continues."));
    }

    #[test]
    fn parse_patches_normal_markers_still_work() {
        // Sanity check: normal patch parsing without code blocks still works
        let response = "\
<!-- patch:status -->
All systems go.
<!-- /patch:status -->
<!-- patch:log -->
- Entry 1
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All systems go.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- Entry 1\n");
        assert!(unmatched.is_empty());
    }

    // --- Inline attribute mode resolution tests ---

    #[test]
    fn inline_attr_mode_overrides_config() {
        // Component has mode=replace inline, but components.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        // Write config with append mode for status
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[status]\nmode = \"append\"\n",
        ).unwrap();
        // But the inline attr says replace
        let doc = "<!-- agent:status mode=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Inline replace should win over config append
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_attr_mode_overrides_default() {
        // exchange defaults to append, but inline says replace
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=replace -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn no_inline_attr_falls_back_to_config() {
        // No inline attr → falls back to components.toml
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[status]\nmode = \"append\"\n",
        ).unwrap();
        let doc = "<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Config says append, so both old and new should be present
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn no_inline_attr_no_config_falls_back_to_default() {
        // No inline attr, no config → built-in defaults
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // exchange defaults to append
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_config() {
        // Component has patch=replace inline, but components.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[status]\nmode = \"append\"\n",
        ).unwrap();
        let doc = "<!-- agent:status patch=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_mode_attr() {
        // Both patch= and mode= present; patch= wins
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=replace mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn toml_patch_key_works() {
        // components.toml uses `patch = "append"` instead of `mode = "append"`
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[status]\npatch = \"append\"\n",
        ).unwrap();
        let doc = "<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn stream_override_beats_inline_attr() {
        // Stream mode overrides should still beat inline attrs
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
        }];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result = apply_patches_with_overrides(doc, &patches, "", &doc_path, &overrides).unwrap();
        // Stream override (replace) should win over inline attr (append)
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn apply_patches_ignores_component_tags_in_code_blocks() {
        // Component tags inside a fenced code block should not be patch targets.
        // Only the real top-level component should receive the patch content.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
# Scaffold Guide

Here is an example of a component:

```markdown
<!-- agent:status -->
example scaffold content
<!-- /agent:status -->
```

<!-- agent:status -->
real status content
<!-- /agent:status -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "patched status\n".to_string(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();

        // The real component should be patched
        assert!(result.contains("patched status\n"), "real component should receive the patch");
        // The code block example should be untouched
        assert!(result.contains("example scaffold content"), "code block content should be preserved");
        // The code block's markers should still be there
        assert!(result.contains("```markdown\n<!-- agent:status -->"), "code block markers should be preserved");
    }

    #[test]
    fn unmatched_content_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:test-uuid-123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // No patch blocks — only unmatched content (simulates skill not wrapping in patch blocks)
        let patches = vec![];
        let unmatched = "### Re: Response\n\nResponse content here.\n";

        let result = apply_patches(doc, &patches, unmatched, &file).unwrap();

        // Response should be inserted at the boundary marker position (after prompt)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after the user prompt (boundary insertion)"
        );

        // Boundary marker should be consumed (replaced by response)
        assert!(
            !result.contains("test-uuid-123"),
            "boundary marker should be consumed after insertion"
        );
    }

    #[test]
    fn explicit_patch_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:patch-uuid-456 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // Explicit patch block targeting exchange
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: Response\n\nResponse content.\n".to_string(),
        }];

        let result = apply_patches(doc, &patches, "", &file).unwrap();

        // Response should be after prompt (boundary consumed)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after user prompt"
        );

        // Boundary marker should be consumed
        assert!(
            !result.contains("patch-uuid-456"),
            "boundary marker should be consumed by explicit patch"
        );
    }

    #[test]
    fn boundary_reinserted_even_when_original_doc_has_no_boundary() {
        // Regression: the snowball bug — once one cycle loses the boundary,
        // every subsequent cycle also loses it because orig_had_boundary finds nothing.
        let dir = setup_project();
        let file = dir.path().join("test.md");
        // Document with exchange but NO boundary marker
        let doc = "<!-- agent:exchange patch=append -->\nUser prompt here.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        let response = "<!-- patch:exchange -->\nAgent response.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        let result = apply_patches(doc, &patches, &unmatched, &file).unwrap();

        // Must have a boundary at end of exchange, even though original had none
        assert!(
            result.contains("<!-- agent:boundary:"),
            "boundary must be re-inserted even when original doc had no boundary: {result}"
        );
    }

    #[test]
    fn boundary_survives_multiple_cycles() {
        // Simulate two consecutive write cycles — boundary must persist
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nPrompt 1.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        // Cycle 1
        let response1 = "<!-- patch:exchange -->\nResponse 1.\n<!-- /patch:exchange -->\n";
        let (patches1, unmatched1) = parse_patches(response1).unwrap();
        let result1 = apply_patches(doc, &patches1, &unmatched1, &file).unwrap();
        assert!(result1.contains("<!-- agent:boundary:"), "cycle 1 must have boundary");

        // Cycle 2 — use cycle 1's output as the new doc (simulates next write)
        let response2 = "<!-- patch:exchange -->\nResponse 2.\n<!-- /patch:exchange -->\n";
        let (patches2, unmatched2) = parse_patches(response2).unwrap();
        let result2 = apply_patches(&result1, &patches2, &unmatched2, &file).unwrap();
        assert!(result2.contains("<!-- agent:boundary:"), "cycle 2 must have boundary");
    }

    #[test]
    fn remove_all_boundaries_skips_code_blocks() {
        let doc = "before\n```\n<!-- agent:boundary:fake-id -->\n```\nafter\n<!-- agent:boundary:real-id -->\nend\n";
        let result = remove_all_boundaries(doc);
        // The one inside the code block should survive
        assert!(
            result.contains("<!-- agent:boundary:fake-id -->"),
            "boundary inside code block must be preserved: {result}"
        );
        // The one outside should be removed
        assert!(
            !result.contains("<!-- agent:boundary:real-id -->"),
            "boundary outside code block must be removed: {result}"
        );
    }

    #[test]
    fn reposition_boundary_moves_to_end() {
        let doc = "\
<!-- agent:exchange -->
Previous response.
<!-- agent:boundary:old-id -->
User prompt here.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        // Old boundary should be gone
        assert!(!result.contains("old-id"), "old boundary should be removed");
        // New boundary should exist
        assert!(result.contains("<!-- agent:boundary:"), "new boundary should be inserted");
        // New boundary should be after the user prompt, before close tag
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let prompt_pos = result.find("User prompt here.").unwrap();
        let close_pos = result.find("<!-- /agent:exchange -->").unwrap();
        assert!(boundary_pos > prompt_pos, "boundary should be after user prompt");
        assert!(boundary_pos < close_pos, "boundary should be before close tag");
    }

    #[test]
    fn reposition_boundary_no_exchange_unchanged() {
        let doc = "\
<!-- agent:output -->
Some content.
<!-- /agent:output -->";
        let result = reposition_boundary_to_end(doc);
        assert!(!result.contains("<!-- agent:boundary:"), "no boundary should be added to non-exchange");
    }
}
