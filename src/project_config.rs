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
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_capture: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_done: Option<crate::frontmatter::PendingCaptureGuardMode>,
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
    /// Guard behavior overrides (for example pending-capture enforcement).
    #[serde(default)]
    pub guards: GuardConfig,
    /// Component-specific configuration (patch modes, timestamps, max_entries, hooks).
    #[serde(default)]
    pub components: BTreeMap<String, ComponentConfig>,
}

/// Load project config from `.agent-doc/config.toml` in CWD, or return defaults.
/// Also performs one-time migration from legacy `components.toml` if present.
pub fn load_project() -> ProjectConfig {
    load_project_from(&project_config_path())
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
pub fn load_project_for_doc(file: &Path) -> ProjectConfig {
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            return load_project_from(&candidate);
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    load_project()
}

/// Load project config from an explicit path. Used by `load_project()` and tests.
pub(crate) fn load_project_from(path: &Path) -> ProjectConfig {
    let mut config = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("warning: failed to parse {}: {}", path.display(), e);
                    ProjectConfig::default()
                }
            },
            Err(e) => {
                eprintln!("warning: failed to read {}: {}", path.display(), e);
                ProjectConfig::default()
            }
        }
    } else {
        ProjectConfig::default()
    };

    // One-time migration: merge legacy components.toml into config.toml
    if let Some(parent) = path.parent() {
        let legacy_path = parent.join("components.toml");
        if legacy_path.exists()
            && let Ok(legacy_content) = std::fs::read_to_string(&legacy_path)
        {
            // Legacy format: flat [name] sections with ComponentConfig fields
            match toml::from_str::<BTreeMap<String, ComponentConfig>>(&legacy_content) {
                Ok(legacy_components) => {
                    let mut migrated = 0usize;
                    for (name, comp) in legacy_components {
                        // config.toml entries take precedence — only insert missing
                        config.components.entry(name).or_insert_with(|| {
                            migrated += 1;
                            comp
                        });
                    }
                    // Save merged config and remove legacy file
                    if let Err(e) = save_project_to(&config, path) {
                        eprintln!("warning: failed to save migrated config: {}", e);
                    } else {
                        if let Err(e) = std::fs::remove_file(&legacy_path) {
                            eprintln!(
                                "warning: failed to remove legacy {}: {}",
                                legacy_path.display(),
                                e
                            );
                        } else {
                            eprintln!(
                                "[config] migrated {} component(s) from components.toml → config.toml",
                                migrated
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "warning: failed to parse legacy {}: {}",
                        legacy_path.display(),
                        e
                    );
                }
            }
        }
    }

    config
}

/// Get the project's configured tmux session (convenience helper).
pub fn project_tmux_session() -> Option<String> {
    load_project().tmux_session
}

/// Save project config to `.agent-doc/config.toml`.
pub fn save_project(config: &ProjectConfig) -> Result<()> {
    save_project_to(config, &project_config_path())
}

/// Save project config to an explicit path. Used by `save_project()` and tests.
pub(crate) fn save_project_to(config: &ProjectConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Update the project's configured tmux session.
/// Called by `agent-doc session set <name>` when the user explicitly pins a session.
pub fn update_project_tmux_session(new_session: &str) -> Result<()> {
    let mut config = load_project();
    let old = config.tmux_session.clone();
    config.tmux_session = Some(new_session.to_string());
    save_project(&config)?;
    eprintln!(
        "[config] updated tmux_session: {} → {}",
        old.as_deref().unwrap_or("(none)"),
        new_session
    );
    Ok(())
}

/// Clear the project's configured tmux session, returning to auto-detect mode.
pub fn clear_project_tmux_session() -> Result<()> {
    let mut config = load_project();
    let old = config.tmux_session.clone();
    config.tmux_session = None;
    save_project(&config)?;
    eprintln!(
        "[config] cleared tmux_session: {} → (auto-detect)",
        old.as_deref().unwrap_or("(none)"),
    );
    Ok(())
}

/// Resolve the path to `.agent-doc/config.toml`, walking up from CWD.
/// Exposed for testing.
fn project_config_path() -> PathBuf {
    // Walk up from CWD to find the .agent-doc/ project root. This avoids
    // CWD-sensitivity when subcommands run from a subdirectory (e.g., a
    // submodule that changed directory mid-session).
    if let Ok(cwd) = std::env::current_dir() {
        let mut current: &Path = &cwd;
        loop {
            if current.join(".agent-doc").is_dir() {
                return current.join(".agent-doc").join("config.toml");
            }
            match current.parent() {
                Some(p) => current = p,
                None => break,
            }
        }
        // No .agent-doc/ found walking up — fall back to CWD (uninitialized project).
        cwd.join(".agent-doc").join("config.toml")
    } else {
        PathBuf::from(".agent-doc").join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        dir.join(".agent-doc").join("config.toml")
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        let cfg = load_project_from(&config_path);
        assert!(cfg.tmux_session.is_none());
        assert!(cfg.components.is_empty());
    }

    #[test]
    fn load_valid_config() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::write(
            &config_path,
            "tmux_session = \"test\"\n\n[components.exchange]\npatch = \"append\"\n",
        )
        .unwrap();
        let cfg = load_project_from(&config_path);
        assert_eq!(cfg.tmux_session.as_deref(), Some("test"));
        assert_eq!(cfg.components["exchange"].patch, "append");
    }

    #[test]
    fn load_guard_config() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::write(
            &config_path,
            "[guards]\npending_capture = \"strict\"\npending_done = \"strict\"\n",
        )
        .unwrap();
        let cfg = load_project_from(&config_path);
        assert_eq!(
            cfg.guards.pending_capture,
            Some(crate::frontmatter::PendingCaptureGuardMode::Strict)
        );
        assert_eq!(
            cfg.guards.pending_done,
            Some(crate::frontmatter::PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());

        let mut cfg = ProjectConfig::default();
        cfg.tmux_session = Some("rt".to_string());
        cfg.components.insert(
            "status".to_string(),
            ComponentConfig {
                patch: "replace".to_string(),
                ..Default::default()
            },
        );
        save_project_to(&cfg, &config_path).unwrap();

        let loaded = load_project_from(&config_path);
        assert_eq!(loaded.tmux_session.as_deref(), Some("rt"));
        assert_eq!(loaded.components["status"].patch, "replace");
    }

    #[test]
    fn migrate_components_toml() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());

        // Write legacy components.toml
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[exchange]\nmode = \"append\"\n\n[status]\nmode = \"replace\"\n",
        )
        .unwrap();

        let cfg = load_project_from(&config_path);
        // Components should be migrated
        assert_eq!(cfg.components["exchange"].patch, "append");
        assert_eq!(cfg.components["status"].patch, "replace");
        // Legacy file should be removed
        assert!(!dir.path().join(".agent-doc/components.toml").exists());
        // config.toml should exist with merged content
        assert!(config_path.exists());
    }

    #[test]
    fn migrate_preserves_existing_config() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());

        // Write config.toml with tmux_session and one component
        std::fs::write(
            &config_path,
            "tmux_session = \"main\"\n\n[components.exchange]\npatch = \"replace\"\n",
        )
        .unwrap();
        // Write legacy components.toml with exchange (append) and status (replace)
        std::fs::write(
            dir.path().join(".agent-doc/components.toml"),
            "[exchange]\nmode = \"append\"\n\n[status]\nmode = \"replace\"\n",
        )
        .unwrap();

        let cfg = load_project_from(&config_path);
        // config.toml's exchange=replace should take precedence over legacy's append
        assert_eq!(cfg.components["exchange"].patch, "replace");
        // status should be migrated from legacy
        assert_eq!(cfg.components["status"].patch, "replace");
        // tmux_session preserved
        assert_eq!(cfg.tmux_session.as_deref(), Some("main"));
        // Legacy file removed
        assert!(!dir.path().join(".agent-doc/components.toml").exists());
    }
}
