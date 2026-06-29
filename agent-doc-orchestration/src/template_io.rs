//! Template I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_core::template`] surface. Lives in the main crate so
//! `agent-doc-core` can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#ckv3` of `#adcr`.
//!
//! Re-exported from [`crate::template`] so existing call sites that use
//! `template_io::apply_patches(file, …)` / `template_io::apply_patches_with_overrides(file, …)`
//! / `template_io::template_info(file)` continue to resolve unchanged.

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use agent_doc_core::project_config::{ComponentConfig, ProjectConfig};
use agent_doc_core::template::{
    ComponentInfo, PatchBlock, TemplateInfo, apply_patches_pure, apply_patches_with_overrides_pure,
};
use agent_doc_element::element;

use crate::graph::RunContext;
use crate::project_config_io;

/// File-based wrapper for the pure
/// [`agent_doc_core::template_io::apply_patches`]. Loads component/max_lines
/// configs from the document's `.agent-doc/config.toml`, derives the
/// boundary summary from the file stem, and delegates.
pub fn apply_patches(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
) -> Result<String> {
    apply_patches_with_context(doc, patches, unmatched, file, None)
}

pub fn apply_patches_with_context(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    rc: Option<&RunContext>,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file, rc);
    let max_lines_configs = load_max_lines_configs(file, rc);
    apply_patches_pure(
        doc,
        patches,
        unmatched,
        summary,
        &component_configs,
        &max_lines_configs,
    )
}

/// File-based wrapper for
/// [`agent_doc_core::template_io::apply_patches_with_overrides`].
pub fn apply_patches_with_overrides(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &HashMap<String, String>,
) -> Result<String> {
    apply_patches_with_overrides_with_context(doc, patches, unmatched, file, mode_overrides, None)
}

pub fn apply_patches_with_overrides_with_context(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &HashMap<String, String>,
    rc: Option<&RunContext>,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file, rc);
    let max_lines_configs = load_max_lines_configs(file, rc);
    apply_patches_with_overrides_pure(
        doc,
        patches,
        unmatched,
        summary,
        &component_configs,
        &max_lines_configs,
        mode_overrides,
    )
}

/// Get template info for a document (for plugin rendering).
pub fn template_info(file: &Path) -> Result<TemplateInfo> {
    template_info_with_context(file, None)
}

pub fn template_info_with_context(file: &Path, rc: Option<&RunContext>) -> Result<TemplateInfo> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, _body) = agent_doc_core::frontmatter::parse(&doc)?;
    let template_mode = fm.resolve_mode().is_template();

    let components = element::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file, rc);

    let component_infos: Vec<ComponentInfo> = components
        .iter()
        .map(|comp| {
            let content = comp.content(&doc).to_string();
            let mode = comp
                .patch_mode()
                .map(|s| s.to_string())
                .or_else(|| configs.get(&comp.name).cloned())
                .unwrap_or_else(|| default_mode(&comp.name).to_string());
            let line = doc[..comp.open_start].matches('\n').count() + 1;
            ComponentInfo {
                name: comp.name.clone(),
                mode,
                content,
                line,
                max_entries: None,
            }
        })
        .collect();

    Ok(TemplateInfo {
        template_mode,
        components: component_infos,
    })
}

/// Load component mode configs from `.agent-doc/config.toml` (under [components] section).
fn load_component_configs(file: &Path, rc: Option<&RunContext>) -> HashMap<String, String> {
    let proj_cfg = load_project_from_doc(file, rc);
    proj_cfg
        .components
        .iter()
        .map(|(name, cfg): (&String, &ComponentConfig)| (name.clone(), cfg.patch.clone()))
        .collect()
}

/// Load max_lines settings from `.agent-doc/config.toml` (under [components] section).
fn load_max_lines_configs(file: &Path, rc: Option<&RunContext>) -> HashMap<String, usize> {
    let proj_cfg = load_project_from_doc(file, rc);
    proj_cfg
        .components
        .iter()
        .filter(|(_, cfg)| cfg.max_lines > 0)
        .map(|(name, cfg): (&String, &ComponentConfig)| (name.clone(), cfg.max_lines))
        .collect()
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
fn load_project_from_doc(file: &Path, rc: Option<&RunContext>) -> Arc<ProjectConfig> {
    if let Some(rc) = rc {
        return rc.project_config();
    }
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            return Arc::new(project_config_io::load_project_from(&candidate));
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    Arc::new(project_config_io::load_project())
}

/// Default mode for a component by name.
fn default_mode(name: &str) -> &'static str {
    match name {
        "exchange" | "findings" => "append",
        _ => "replace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn apply_patches_with_context_uses_project_config_slot() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.exchange]\npatch = \"prepend\"\n",
        )
        .unwrap();
        let doc_path = dir.path().join("doc.md");
        let doc = "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();
        let rc = RunContext::new(doc_path.clone());

        let (patches, unmatched) = agent_doc_core::template::parse_patches(
            "<!-- patch:exchange -->\nnew\n<!-- /patch:exchange -->\n",
        )
        .unwrap();
        assert!(!rc.is_project_config_cached());
        let patched =
            apply_patches_with_context(doc, &patches, &unmatched, &doc_path, Some(&rc)).unwrap();

        assert!(rc.is_project_config_cached());
        assert!(
            patched.contains("new\nold"),
            "component config should make exchange prepend, got:\n{patched}"
        );
    }
}
