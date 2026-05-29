//! Project-config I/O — `&Path`-taking and `std::fs`-using wrappers
//! around the pure [`agent_doc_core::project_config`] surface. Kept out of
//! `agent-doc-core` so it can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#bjrv` of `#adcr`; relocated to
//! `agent-doc-orchestration` under `#adoc-orchestration-crate` Direction A.
//!
//! Re-exported through the main crate's `project_config` shim so the ~25
//! existing call sites in main (session_check, session_accretion, mode,
//! claim, patch, lint_gate, etc.) continue resolving unchanged.

#![allow(dead_code)]

use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_doc_core::project_config::{
    ComponentConfig, ProjectConfig, parse_legacy_components_toml, parse_project_toml,
};

/// Load project config from `.agent-doc/config.toml` in CWD, or return defaults.
pub fn load_project() -> ProjectConfig {
    load_project_from(&project_config_path())
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
pub fn load_project_for_doc(file: &Path) -> ProjectConfig {
    if let Some(root) = project_root_for_doc(file) {
        return load_project_from(&root.join(".agent-doc").join("config.toml"));
    }
    load_project()
}

/// Resolve the nearest project root for a document by walking up to `.agent-doc/`.
pub fn project_root_for_doc(file: &Path) -> Option<PathBuf> {
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    None
}

/// Load project config from an explicit path. On absence, I/O error, or
/// parse error, returns `ProjectConfig::default()` and emits a warning
/// to stderr (never panics). Performs one-time migration from legacy
/// `components.toml` if present.
pub fn load_project_from(path: &Path) -> ProjectConfig {
    let mut config = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(content) => match parse_project_toml(&content) {
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

    if let Some(parent) = path.parent() {
        let legacy_path = parent.join("components.toml");
        if legacy_path.exists()
            && let Ok(legacy_content) = std::fs::read_to_string(&legacy_path)
        {
            match parse_legacy_components_toml(&legacy_content) {
                Ok(legacy_components) => {
                    let mut migrated = 0usize;
                    for (name, comp) in legacy_components {
                        config.components.entry(name).or_insert_with(|| {
                            migrated += 1;
                            comp
                        });
                    }
                    if let Err(e) = save_project_to(&config, path) {
                        eprintln!("warning: failed to save migrated config: {}", e);
                    } else if let Err(e) = std::fs::remove_file(&legacy_path) {
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
pub fn save_project_to(config: &ProjectConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Update the project's configured tmux session.
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

/// Clear the project's configured tmux session.
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
fn project_config_path() -> PathBuf {
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
            "tmux_session = \"test\"\nagent_doc_auto_compact = 240\n\n[components.exchange]\npatch = \"append\"\n",
        )
        .unwrap();
        let cfg = load_project_from(&config_path);
        assert_eq!(cfg.tmux_session.as_deref(), Some("test"));
        assert_eq!(cfg.agent_doc_auto_compact, Some(240));
        assert_eq!(cfg.components["exchange"].patch, "append");
    }

    #[test]
    fn project_root_for_doc_walks_up() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("nested/deep")).unwrap();
        let doc = dir.path().join("nested/deep/file.md");
        let root = project_root_for_doc(&doc).unwrap();
        assert_eq!(root, dir.path());
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());

        let mut cfg = ProjectConfig {
            tmux_session: Some("rt".to_string()),
            ..Default::default()
        };
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
    fn legacy_components_toml_migrates() {
        let dir = TempDir::new().unwrap();
        let _ = setup_project(dir.path());
        let legacy = dir.path().join(".agent-doc/components.toml");
        std::fs::write(
            &legacy,
            "[exchange]\npatch = \"append\"\n[status]\npatch = \"replace\"\n",
        )
        .unwrap();

        let cfg = load_project_from(&dir.path().join(".agent-doc/config.toml"));
        assert_eq!(cfg.components["exchange"].patch, "append");
        assert_eq!(cfg.components["status"].patch, "replace");
        assert!(!legacy.exists());
    }
}

/// Map a `BTreeMap` of components for callers that need it.
#[allow(dead_code)]
fn _unused_btreemap_marker() -> BTreeMap<String, ComponentConfig> {
    BTreeMap::new()
}
