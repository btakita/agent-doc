//! `agent-doc skill` — Manage the Claude Code skill definition.
//!
//! Delegates to `agent_kit::skill::SkillConfig` for the actual install/check logic.
//! The SKILL.md content is bundled into the binary at build time via `include_str!`.

use anyhow::Result;
use std::path::Path;

use agent_kit::skill::SkillConfig;

/// The SKILL.md content bundled at build time.
const BUNDLED_SKILL: &str = include_str!("../SKILL.md");

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn config() -> SkillConfig {
    SkillConfig::new("agent-doc", BUNDLED_SKILL, VERSION)
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, paths are relative to CWD.
#[allow(dead_code)]
pub fn install_at(root: Option<&Path>) -> Result<()> {
    config().install(root)
}

/// Public entry point (CWD-relative, called from main).
#[allow(dead_code)]
pub fn install() -> Result<()> {
    install_at(None)
}

/// Install and return whether the file was actually updated (not just already up to date).
pub fn install_and_check_updated() -> Result<bool> {
    let cfg = config();
    let path = cfg.skill_path(None);

    // Check if already up to date before install
    let was_current = path.exists()
        && std::fs::read_to_string(&path)
            .map(|existing| existing == cfg.content)
            .unwrap_or(false);

    cfg.install(None)?;
    Ok(!was_current)
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, paths are relative to CWD.
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let up_to_date = config().check(root)?;
    if !up_to_date {
        std::process::exit(1);
    }
    Ok(())
}

/// Public entry point (CWD-relative, called from main).
pub fn check() -> Result<()> {
    check_at(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skill_is_not_empty() {
        assert!(!BUNDLED_SKILL.is_empty());
    }

    #[test]
    fn bundled_skill_contains_agent_doc() {
        assert!(BUNDLED_SKILL.contains("agent-doc"));
    }

    #[test]
    fn install_creates_file() {
        let dir = tempfile::tempdir().unwrap();

        install_at(Some(dir.path())).unwrap();

        let path = dir.path().join(".claude/skills/agent-doc/SKILL.md");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        install_at(Some(dir.path())).unwrap();
        install_at(Some(dir.path())).unwrap();

        let path = dir.path().join(".claude/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();

        let path = dir.path().join(".claude/skills/agent-doc/SKILL.md");
        assert!(!path.exists());
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();

        let path = dir.path().join(".claude/skills/agent-doc/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install_at(Some(dir.path())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }
}
