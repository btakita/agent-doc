//! Template-mode support for in-place response documents.
//!
//! Parses `<!-- patch:name -->...<!-- /patch:name -->` blocks from agent responses
//! and applies them to the corresponding `<!-- agent:name -->` components in the document.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::component;

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

/// Check if a document is in template mode.
pub fn is_template_mode(mode: Option<&str>) -> bool {
    matches!(mode, Some("template"))
}

/// Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks from an agent response.
///
/// Content outside patch blocks is collected as "unmatched" and returned separately.
pub fn parse_patches(response: &str) -> Result<(Vec<PatchBlock>, String)> {
    let bytes = response.as_bytes();
    let len = bytes.len();
    let mut patches = Vec::new();
    let mut unmatched = String::new();
    let mut pos = 0;
    let mut last_end = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            pos += 1;
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

            // Find the matching close: <!-- /patch:name -->
            let close_marker = format!("<!-- /patch:{} -->", name);
            if let Some(close_pos) = response[content_start..].find(&close_marker) {
                let content = &response[content_start..content_start + close_pos];
                patches.push(PatchBlock {
                    name: name.to_string(),
                    content: content.to_string(),
                });

                let mut end = content_start + close_pos + close_marker.len();
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
        let mode = configs.get(&patch.name).map(|s| s.as_str()).unwrap_or_else(|| default_mode(&patch.name));
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
            // Append to existing exchange/output component
            let existing = output_comp.content(&result);
            let new_content = if existing.trim().is_empty() {
                format!("{}\n", unmatched)
            } else {
                format!("{}{}\n", existing, unmatched)
            };
            result = output_comp.replace_content(&result, &new_content);
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

/// Get template info for a document (for plugin rendering).
pub fn template_info(file: &Path) -> Result<TemplateInfo> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, _body) = crate::frontmatter::parse(&doc)?;
    let template_mode = is_template_mode(fm.mode.as_deref());

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file);

    let component_infos: Vec<ComponentInfo> = components
        .iter()
        .map(|comp| {
            let content = comp.content(&doc).to_string();
            let mode = configs.get(&comp.name).cloned().unwrap_or_else(|| default_mode(&comp.name).to_string());
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
                if let Some(mode) = value.get("mode").and_then(|v| v.as_str()) {
                    result.insert(name.clone(), mode.to_string());
                }
            }
        }
    }
    result
}

/// Default mode for a component by name.
/// `exchange` defaults to `append`; all others default to `replace`.
fn default_mode(name: &str) -> &'static str {
    if name == "exchange" { "append" } else { "replace" }
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
        let doc = "---\nresponse_mode: template\n---\n\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(info.template_mode);
        assert_eq!(info.components.len(), 1);
        assert_eq!(info.components[0].name, "status");
        assert_eq!(info.components[0].content, "content\n");
    }

    #[test]
    fn template_info_append_mode() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nresponse_mode: append\n---\n\n# Doc\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(!info.template_mode);
        assert!(info.components.is_empty());
    }
}
