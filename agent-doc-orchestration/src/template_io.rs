//! Template I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_core::template`] surface. Lives in the main crate so
//! `agent-doc-core` can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#ckv3` of `#adcr`.
//!
//! Re-exported from [`crate::template`] so existing call sites that use
//! `template::apply_patches(file, …)` / `template::apply_patches_with_overrides(file, …)`
//! / `template::template_info(file)` continue to resolve unchanged.

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use agent_doc_core::component;
use agent_doc_core::template::{
    ComponentInfo, PatchBlock, TemplateInfo, apply_patches_pure, apply_patches_with_overrides_pure,
};

use crate::project_config;

/// File-based wrapper for the pure
/// [`agent_doc_core::template::apply_patches`]. Loads component/max_lines
/// configs from the document's `.agent-doc/config.toml`, derives the
/// boundary summary from the file stem, and delegates.
pub fn apply_patches(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file);
    let max_lines_configs = load_max_lines_configs(file);
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
/// [`agent_doc_core::template::apply_patches_with_overrides`].
pub fn apply_patches_with_overrides(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &HashMap<String, String>,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file);
    let max_lines_configs = load_max_lines_configs(file);
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
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, _body) = agent_doc_core::frontmatter::parse(&doc)?;
    let template_mode = fm.resolve_mode().is_template();

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file);

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
fn load_component_configs(file: &Path) -> HashMap<String, String> {
    let proj_cfg = load_project_from_doc(file);
    proj_cfg
        .components
        .iter()
        .map(|(name, cfg): (&String, &project_config::ComponentConfig)| {
            (name.clone(), cfg.patch.clone())
        })
        .collect()
}

/// Load max_lines settings from `.agent-doc/config.toml` (under [components] section).
fn load_max_lines_configs(file: &Path) -> HashMap<String, usize> {
    let proj_cfg = load_project_from_doc(file);
    proj_cfg
        .components
        .iter()
        .filter(|(_, cfg)| cfg.max_lines > 0)
        .map(|(name, cfg): (&String, &project_config::ComponentConfig)| {
            (name.clone(), cfg.max_lines)
        })
        .collect()
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
fn load_project_from_doc(file: &Path) -> project_config::ProjectConfig {
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            return project_config::load_project_from(&candidate);
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    project_config::load_project()
}

/// Default mode for a component by name.
fn default_mode(name: &str) -> &'static str {
    match name {
        "exchange" | "findings" => "append",
        _ => "replace",
    }
}
