//! # Module: project_config
//!
//! Project-level configuration loaded from `.agent-doc/config.toml`.
//! Shared between binary and library for consistent project config handling.
//!
//! ## Spec
//! - Defines `ProjectConfig`: per-project settings (tmux_session, components).
//! - Defines `ComponentConfig`: per-component patch configuration (mode, timestamps, hooks).
//! - `load_project()` reads and parses the project config file. On absence, I/O error, or parse
//!   error, returns `ProjectConfig::default()` and emits a warning to stderr (never panics).
//! - `project_tmux_session()` is a convenience wrapper returning the configured tmux session name.
//! - `save_project()` serialises `ProjectConfig` to TOML and writes it to
//!   `.agent-doc/config.toml`, creating the directory if needed.
//!
//! ## Agentic Contracts
//! - Never panics on missing config: `load_project()` returns defaults when the file is absent.
//! - Project config errors are non-fatal: errors are surfaced as stderr warnings, not propagated.
//! - Atomic-safe directory creation: `save_project()` calls `create_dir_all` before writing.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_capture: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_done: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_done: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_done: Option<bool>,
}

/// Workspace-level lint configuration (`[lint]` section in
/// `.agent-doc/config.toml`).
///
/// Currently exposes a single key, `dialect`, controlling the
/// `tagpath lint --dialect agent-doc` finalize gate. See
/// `crate::frontmatter::LintDialectMode` for the resolved semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LintConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<crate::frontmatter::LintDialectMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshProfileConfig {
    /// Resolved SSH targets for a named profile.
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshDocConfig {
    /// Optional profile name to resolve for this document path.
    #[serde(default)]
    pub profile: Option<String>,
    /// Optional direct targets for this document path.
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshConfig {
    /// Named SSH profiles that expand to concrete targets.
    #[serde(default)]
    pub profiles: BTreeMap<String, SshProfileConfig>,
    /// Relative document paths that require SSH metadata resolution.
    #[serde(default)]
    pub docs: BTreeMap<String, SshDocConfig>,
}

/// Component patch configuration (mode, timestamps, max entries, hooks).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentConfig {
    /// Patch mode: "replace" (default), "append", "prepend".
    /// `patch` is the primary key; `mode` is a backward-compatible alias.
    #[serde(default = "default_patch_mode", alias = "mode")]
    pub patch: String,
    /// Merge strategy: "append-friendly" (default) or "strict".
    /// "append-friendly" auto-resolves conflicts where both sides only appended.
    /// "strict" preserves all conflict markers for manual resolution.
    /// Currently parsed for config validation; merge runs at document level.
    #[serde(default = "default_merge_strategy")]
    #[allow(dead_code)]
    pub merge_strategy: String,
    /// Auto-prefix entries with ISO timestamp (for append/prepend modes)
    #[serde(default)]
    pub timestamp: bool,
    /// Auto-trim old entries in append/prepend modes (0 = unlimited)
    #[serde(default)]
    pub max_entries: usize,
    /// Trim component content to the last N lines after patching (0 = unlimited).
    /// Currently used by template.rs post-patch processing.
    #[serde(default)]
    #[allow(dead_code)]
    pub max_lines: usize,
    /// Shell command to run before patching (stdin: content, stdout: transformed)
    #[serde(default)]
    pub pre_patch: Option<String>,
    /// Shell command to run after patching (fire-and-forget)
    #[serde(default)]
    pub post_patch: Option<String>,
}

fn default_patch_mode() -> String {
    "replace".to_string()
}

fn default_merge_strategy() -> String {
    "append-friendly".to_string()
}

/// Project-level configuration, read from `.agent-doc/config.toml` relative to CWD.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Target tmux session name for this project.
    #[serde(default)]
    pub tmux_session: Option<String>,
    /// Explicit opt-in for automatic compaction/reload policies.
    /// Session-accretion heuristics never compact by themselves; omit to disable.
    #[serde(default, alias = "auto_compact")]
    pub agent_doc_auto_compact: Option<usize>,
    /// Guard behavior overrides (for example pending-capture enforcement).
    #[serde(default)]
    pub guards: GuardConfig,
    /// Lint behavior overrides for the agent-doc finalize lint gate.
    #[serde(default)]
    pub lint: LintConfig,
    /// Project-local SSH requirement mappings for known ops documents.
    #[serde(default)]
    pub ssh: SshConfig,
    /// Component-specific configuration (patch modes, timestamps, max_entries, hooks).
    #[serde(default)]
    pub components: BTreeMap<String, ComponentConfig>,
}

/// Parse a TOML string into a [`ProjectConfig`]. Pure — no fs I/O.
///
/// File-based loading (with legacy `components.toml` migration) lives in
/// `crate::project_config_io` in the main `agent-doc` crate.
pub fn parse_project_toml(content: &str) -> Result<ProjectConfig> {
    toml::from_str(content).map_err(anyhow::Error::from)
}

/// Parse a legacy `components.toml` body (flat `[name]` tables of
/// [`ComponentConfig`] fields) into a map. Used by the file-based migrator
/// in `crate::project_config_io`.
pub fn parse_legacy_components_toml(
    content: &str,
) -> Result<BTreeMap<String, ComponentConfig>> {
    toml::from_str(content).map_err(anyhow::Error::from)
}
