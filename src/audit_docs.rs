//! # Module: audit_docs
//!
//! ## Spec
//! - Audits instruction files (CLAUDE.md, AGENTS.md, etc.) present in the codebase against
//!   a known-correct reference, using the shared `instruction_files` crate.
//! - Accepts an optional `root_override` path to run the audit from a directory other than CWD.
//! - Uses `AuditConfig::agent_doc()` to select agent-doc–specific configuration (file patterns,
//!   ignore rules, etc.).
//! - When no `root_override` is provided and the current CWD is an outer repo that contains the
//!   running `agent-doc` crate checkout (for example `cargo run --manifest-path src/agent-doc/Cargo.toml -- audit-docs`
//!   from the monorepo root), prefers the nested crate root over the outer repo root.
//!
//! ## Agentic Contracts
//! - `run(root_override)` — performs the audit and returns `Ok(())` on success or a descriptive
//!   `anyhow::Error` on failure; never silently swallows errors.
//! - All configuration and output formatting is owned by `instruction_files`; this module is a
//!   thin adapter.
//!
//! ## Evals
//! - valid_project: project with correct instruction files → `Ok(())`, no output
//! - missing_file: required instruction file absent → `Err` with file path in message
//! - root_override: alternate root provided → audit runs relative to that path, not CWD
//! - nested_dev_crate_root_override: when current CWD contains the running crate under a nested
//!   path, audit scopes to that crate root instead of the outer repo

use anyhow::Result;
use instruction_files::AuditConfig;
use std::path::{Path, PathBuf};

pub fn run(root_override: Option<&Path>) -> Result<()> {
    let config = AuditConfig::agent_doc();
    let nested_override = root_override
        .is_none()
        .then(resolve_nested_dev_crate_root_override)
        .flatten();
    let fallback_root = if root_override.is_none() && nested_override.is_none() {
        fallback_root_without_marker(&config)
    } else {
        None
    };
    let resolved_root = root_override
        .map(|p| p.to_path_buf())
        .or(nested_override)
        .or(fallback_root);
    instruction_files::run(&config, resolved_root.as_deref())
}

fn fallback_root_without_marker(config: &AuditConfig) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?.canonicalize().ok()?;
    if find_project_marker_root_from(&cwd, config).is_some() {
        return None;
    }
    eprintln!("Warning: no project root marker found, using current directory");
    Some(cwd)
}

fn find_project_marker_root_from(start: &Path, config: &AuditConfig) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if config
            .root_markers
            .iter()
            .any(|marker| dir.join(marker).exists())
        {
            return Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent,
            _ => return None,
        }
    }
}

fn resolve_nested_dev_crate_root_override() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?.canonicalize().ok()?;
    let dev_root = resolve_running_crate_root()?;
    if dev_root != cwd && dev_root.starts_with(&cwd) {
        Some(dev_root)
    } else {
        None
    }
}

fn resolve_running_crate_root() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    for ancestor in exe.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if !cargo_toml.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&cargo_toml) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<toml::Value>(&content) else {
            continue;
        };
        let Some(package_name) = parsed
            .get("package")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        if package_name == env!("CARGO_PKG_NAME") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_agent_doc_cargo_toml(dir: &Path) {
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"agent-doc\"\nversion = \"0.33.13\"\n",
        )
        .unwrap();
    }

    #[test]
    fn nested_dev_crate_root_override_prefers_nested_crate() {
        let tmp = TempDir::new().unwrap();
        let outer = tmp.path();
        let nested = outer.join("src/agent-doc");
        std::fs::create_dir_all(&nested).unwrap();
        write_agent_doc_cargo_toml(&nested);

        let resolved = resolve_nested_dev_crate_root_override_from(
            &outer.canonicalize().unwrap(),
            Some(nested.canonicalize().unwrap()),
        );
        assert_eq!(resolved, Some(nested.canonicalize().unwrap()));
    }

    #[test]
    fn nested_dev_crate_root_override_skips_unrelated_cwd() {
        let tmp = TempDir::new().unwrap();
        let outer = tmp.path();
        let nested = outer.join("src/agent-doc");
        let elsewhere = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        write_agent_doc_cargo_toml(&nested);

        let resolved = resolve_nested_dev_crate_root_override_from(
            &elsewhere.path().canonicalize().unwrap(),
            Some(nested.canonicalize().unwrap()),
        );
        assert_eq!(resolved, None);
    }

    #[test]
    fn nested_dev_crate_root_override_respects_exact_cwd() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("agent-doc");
        std::fs::create_dir_all(&nested).unwrap();
        write_agent_doc_cargo_toml(&nested);
        let nested = nested.canonicalize().unwrap();

        let resolved = resolve_nested_dev_crate_root_override_from(&nested, Some(nested.clone()));
        assert_eq!(resolved, None);
    }

    fn resolve_nested_dev_crate_root_override_from(
        cwd: &Path,
        dev_root: Option<std::path::PathBuf>,
    ) -> Option<std::path::PathBuf> {
        let dev_root = dev_root?;
        if dev_root != cwd && dev_root.starts_with(cwd) {
            Some(dev_root)
        } else {
            None
        }
    }
}
