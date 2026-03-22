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

/// Resolve the project root for skill installation.
///
/// When CWD is inside a git submodule (e.g., `src/agent-doc/`), the skill
/// should be installed to the superproject root, not the submodule. This
/// ensures the SKILL.md that Claude Code reads (from the project root's
/// `.claude/skills/`) matches the binary version.
fn resolve_root() -> Option<std::path::PathBuf> {
    // Try superproject first (handles submodule CWD)
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    // Fall back to git toplevel
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    None
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
#[allow(dead_code)]
pub fn install_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    config().install(resolved.as_deref())
}

/// Public entry point (resolves to superproject root, called from main).
#[allow(dead_code)]
pub fn install() -> Result<()> {
    install_at(None)
}

/// Install and return whether the file was actually updated (not just already up to date).
pub fn install_and_check_updated() -> Result<bool> {
    let cfg = config();
    let resolved = resolve_root();
    let path = cfg.skill_path(resolved.as_deref());

    // Check if already up to date before install
    let was_current = path.exists()
        && std::fs::read_to_string(&path)
            .map(|existing| existing == cfg.content)
            .unwrap_or(false);

    cfg.install(resolved.as_deref())?;
    Ok(!was_current)
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let up_to_date = config().check(resolved.as_deref())?;
    if !up_to_date {
        std::process::exit(1);
    }
    Ok(())
}

/// Public entry point (resolves to superproject root, called from main).
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
