//! Project-config I/O -- `&Path`-taking and `std::fs`-using wrappers around the
//! pure [`agent_doc_frontmatter::project_config`] surface. Kept out of
//! `agent-doc-frontmatter` so the focused crate stays parse-only.
//!
//! Orchestration and the CLI shell call this crate directly for file-backed
//! project configuration effects.

#![allow(dead_code)]

use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_frontmatter::project_config::{
    ProjectConfig, parse_legacy_components_toml, parse_project_toml,
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
/// Delegates to [`agent_doc_fs::find_project_root`].
pub fn project_root_for_doc(file: &Path) -> Option<PathBuf> {
    agent_doc_fs::find_project_root(file)
}

/// Resolve the optional project-default document used for dogfooding
/// `#agent-doc-bug` backlog capture. Relative configured paths are interpreted
/// with the same project-root / redundant-project-prefix rules as explicit
/// markdown backlog targets. `None` means "use the current document".
pub fn agent_doc_bug_target_document_for_doc(file: &Path) -> Result<Option<PathBuf>> {
    let config = load_project_for_doc(file);
    let Some(target) = config.agent_doc_bug_target_document.as_deref() else {
        return Ok(None);
    };
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    agent_doc_fs::referenced_markdown_path_checked(file, target)
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

/// Get the project's explicitly configured tmux executable.
pub fn project_tmux_bin() -> Option<String> {
    load_project()
        .tmux_bin
        .filter(|binary| !binary.trim().is_empty())
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
    use agent_doc_frontmatter::project_config::ComponentConfig;
    use tempfile::TempDir;

    fn setup_project(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        dir.join(".agent-doc").join("config.toml")
    }

    #[test]
    fn agent_doc_bug_target_document_resolves_relative_to_project_root() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/software")).unwrap();
        let current = dir.path().join("tasks/software/source.md");
        let target = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(&target, "# bugs\n").unwrap();
        std::fs::write(
            &config_path,
            r#"agent_doc_bug_target_document = "tasks/agent-doc/agent-doc-bugs2.md"
"#,
        )
        .unwrap();

        let resolved = agent_doc_bug_target_document_for_doc(&current)
            .unwrap()
            .unwrap();

        assert_eq!(resolved, target.canonicalize().unwrap());
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
            "tmux_session = \"test\"\ntmux_bin = \"/opt/tmux/bin/tmux\"\nagent_doc_auto_compact = 240\n\n[components.exchange]\npatch = \"append\"\n",
        )
        .unwrap();
        let cfg = load_project_from(&config_path);
        assert_eq!(cfg.tmux_session.as_deref(), Some("test"));
        assert_eq!(cfg.tmux_bin.as_deref(), Some("/opt/tmux/bin/tmux"));
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
            tmux_bin: Some("/opt/tmux/bin/tmux".to_string()),
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
        assert_eq!(loaded.tmux_bin.as_deref(), Some("/opt/tmux/bin/tmux"));
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
