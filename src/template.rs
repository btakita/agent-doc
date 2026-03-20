//! Template-mode support for in-place response documents.
//!
//! Parses `<!-- patch:name -->...<!-- /patch:name -->` blocks from agent responses
//! and applies them to the corresponding `<!-- agent:name -->` components in the document.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::component::{self, Component};

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
    let mut result = doc.to_string();

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
        if mode == "append" {
            if let Some(bid) = find_boundary_in_component(&result, comp) {
                result = comp.append_with_boundary(&result, &patch.content, &bid);
                continue;
            }
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

    Ok(result)
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

fn find_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
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
        let (patches, unmatched) = parse_patches(response).unwrap();
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
}
