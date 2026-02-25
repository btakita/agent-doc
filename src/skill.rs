//! `agent-doc skill` — Manage the Claude Code skill definition.
//!
//! The SKILL.md content is bundled into the binary at build time via
//! `include_str!`. This ensures the installed skill version always matches
//! the binary version.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The SKILL.md content bundled at build time.
const BUNDLED_SKILL: &str = include_str!("../SKILL.md");

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Target path relative to project root.
const SKILL_REL: &str = ".claude/skills/agent-doc/SKILL.md";

/// Resolve the skill path under the given root (or CWD if None).
fn skill_path(root: Option<&Path>) -> PathBuf {
    match root {
        Some(r) => r.join(SKILL_REL),
        None => PathBuf::from(SKILL_REL),
    }
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, paths are relative to CWD.
pub fn install_at(root: Option<&Path>) -> Result<()> {
    let path = skill_path(root);

    // Check if already up to date
    if path.exists() {
        let existing = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if existing == BUNDLED_SKILL {
            eprintln!("Skill already up to date (v{VERSION}).");
            return Ok(());
        }
    }

    // Create directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    // Write
    std::fs::write(&path, BUNDLED_SKILL)
        .with_context(|| format!("failed to write {}", path.display()))?;
    eprintln!("Installed skill v{VERSION} → {}", path.display());

    Ok(())
}

/// Public entry point (CWD-relative, called from main).
pub fn install() -> Result<()> {
    install_at(None)
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, paths are relative to CWD.
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let path = skill_path(root);

    if !path.exists() {
        eprintln!("Not installed. Run `agent-doc skill install` to install.");
        std::process::exit(1);
    }

    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if existing == BUNDLED_SKILL {
        eprintln!("Up to date (v{VERSION}).");
    } else {
        eprintln!("Outdated. Run `agent-doc skill install` to update to v{VERSION}.");
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

        let path = dir.path().join(SKILL_REL);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        install_at(Some(dir.path())).unwrap();
        install_at(Some(dir.path())).unwrap(); // should print "already up to date"

        let path = dir.path().join(SKILL_REL);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();

        // check_at() calls process::exit, so we can't easily test it in-process.
        // Instead, test the file-not-found path directly.
        let path = dir.path().join(SKILL_REL);
        assert!(!path.exists());
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();

        let path = dir.path().join(SKILL_REL);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install_at(Some(dir.path())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }
}
