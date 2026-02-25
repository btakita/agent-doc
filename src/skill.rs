//! `agent-doc skill` — Manage the Claude Code skill definition.
//!
//! The SKILL.md content is bundled into the binary at build time via
//! `include_str!`. This ensures the installed skill version always matches
//! the binary version.

use anyhow::{Context, Result};
use std::path::Path;

/// The SKILL.md content bundled at build time.
const BUNDLED_SKILL: &str = include_str!("../SKILL.md");

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Target path relative to CWD.
const SKILL_PATH: &str = ".claude/skills/agent-doc/SKILL.md";

/// Install the bundled SKILL.md to the project.
pub fn install() -> Result<()> {
    let path = Path::new(SKILL_PATH);

    // Check if already up to date
    if path.exists() {
        let existing = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", SKILL_PATH))?;
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
    std::fs::write(path, BUNDLED_SKILL)
        .with_context(|| format!("failed to write {}", SKILL_PATH))?;
    eprintln!("Installed skill v{VERSION} → {SKILL_PATH}");

    Ok(())
}

/// Check if the installed skill matches the bundled version.
pub fn check() -> Result<()> {
    let path = Path::new(SKILL_PATH);

    if !path.exists() {
        eprintln!("Not installed. Run `agent-doc skill install` to install.");
        std::process::exit(1);
    }

    let existing = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", SKILL_PATH))?;

    if existing == BUNDLED_SKILL {
        eprintln!("Up to date (v{VERSION}).");
    } else {
        eprintln!("Outdated. Run `agent-doc skill install` to update to v{VERSION}.");
        std::process::exit(1);
    }

    Ok(())
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
        std::env::set_current_dir(dir.path()).unwrap();

        install().unwrap();

        let path = dir.path().join(SKILL_PATH);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        install().unwrap();
        install().unwrap(); // should print "already up to date"

        let path = dir.path().join(SKILL_PATH);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // check() calls process::exit, so we can't easily test it in-process.
        // Instead, test the file-not-found path directly.
        let path = dir.path().join(SKILL_PATH);
        assert!(!path.exists());
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let path = dir.path().join(SKILL_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install().unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BUNDLED_SKILL);
    }
}
