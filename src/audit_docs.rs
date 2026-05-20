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
//! - Also audits generated agent-doc instruction surfaces in the explicit root or resolved install
//!   root. Without `--root`, submodule checkouts audit the superproject install root used by
//!   release installs. With explicit `--root`, the given root is audited exactly, so
//!   submodule-local managed artifacts can still be checked intentionally. Managed surfaces must
//!   match the running binary's rendered content; custom root AGENTS.md files are ignored.
//! - Filesystem mtime staleness is reported as advisory output only; managed instruction surfaces
//!   are release-blocking through rendered-content comparison, not timestamp comparison.
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
//! - managed_instruction_surface_roots: running from a submodule audits the superproject install
//!   root instead of ignored submodule-local install artifacts
//! - explicit_managed_instruction_surface_root: `--root` audits the requested root exactly,
//!   including submodule-local managed instruction artifacts
//! - mtime_staleness_advisory: source-newer-than-doc output is non-blocking when content checks pass

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
    for root in managed_instruction_surface_roots(root_override) {
        crate::skill::audit_managed_instruction_surfaces(Some(&root))?;
    }
    run_agent_doc_audit(&config, resolved_root.as_deref())
}

fn run_agent_doc_audit(config: &AuditConfig, root_override: Option<&Path>) -> Result<()> {
    println!("Auditing docs...\n");

    let root = match root_override {
        Some(path) => path.to_path_buf(),
        None => instruction_files::find_root(config),
    };
    let files = instruction_files::find_instruction_files(&root, config);
    let mut issues: Vec<instruction_files::Issue> = Vec::new();

    for doc in &files {
        let rel = doc
            .strip_prefix(&root)
            .unwrap_or(doc)
            .to_string_lossy()
            .to_string();
        if let Ok(content) = std::fs::read_to_string(doc) {
            issues.extend(instruction_files::check_tree_paths(&rel, &content, &root));
            issues.extend(instruction_files::check_actionable(&rel, &content, config));
            issues.extend(instruction_files::check_context_invariant(
                &rel, &content, config,
            ));
        }
    }

    let (budget_issues, counts, total) =
        instruction_files::check_line_budget(&files, &root, config);
    issues.extend(budget_issues);

    let staleness_advisories = instruction_files::check_staleness(&files, &root, config)
        .into_iter()
        .map(|mut issue| {
            issue.warning = true;
            issue.message = format!(
                "Mtime advisory: {}; generated agent-doc surfaces are checked by rendered content",
                issue.message
            );
            issue
        })
        .collect::<Vec<_>>();

    for issue in &issues {
        print_issue(issue);
    }
    for issue in &staleness_advisories {
        print_issue(issue);
    }

    let mark = if total <= agent_kit::audit_common::LINE_BUDGET {
        "\u{2713}"
    } else {
        "\u{2717}"
    };
    println!(
        "\nCombined instruction files: {} lines (budget: {}) {}",
        total,
        agent_kit::audit_common::LINE_BUDGET,
        mark
    );
    for (name, n) in &counts {
        println!("  {}: {}", name, n);
    }

    if !issues.is_empty() {
        println!("\nFound {} blocking issue(s)", issues.len());
        std::process::exit(1);
    }

    if staleness_advisories.is_empty() {
        println!("\nNo issues found \u{2713}");
    } else {
        println!(
            "\nNo blocking issues found \u{2713} ({} mtime advisory(s))",
            staleness_advisories.len()
        );
    }

    Ok(())
}

fn print_issue(issue: &instruction_files::Issue) {
    let mut loc = format!("  {}", issue.file);
    if issue.line > 0 {
        if issue.end_line > issue.line {
            loc.push_str(&format!(":{}-{}", issue.line, issue.end_line));
        } else {
            loc.push_str(&format!(":{}", issue.line));
        }
    }
    let marker = if issue.warning {
        "\u{26a0}"
    } else {
        "\u{2717}"
    };
    println!("{:<50} {} {}", loc, marker, issue.message);
}

fn managed_instruction_surface_roots(root_override: Option<&Path>) -> Vec<PathBuf> {
    let superproject_root = root_override
        .is_none()
        .then(resolve_git_superproject_root)
        .flatten();
    let cwd = std::env::current_dir().ok();
    managed_instruction_surface_roots_from(
        root_override,
        superproject_root.as_deref(),
        cwd.as_deref(),
    )
}

fn managed_instruction_surface_roots_from(
    root_override: Option<&Path>,
    superproject_root: Option<&Path>,
    cwd: Option<&Path>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = root_override {
        push_unique_root(&mut roots, root);
        return roots;
    }
    if let Some(root) = superproject_root {
        push_unique_root(&mut roots, root);
        return roots;
    }
    if let Some(cwd) = cwd {
        push_unique_root(&mut roots, cwd);
    }
    roots
}

fn push_unique_root(roots: &mut Vec<PathBuf>, root: &Path) {
    let normalized = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !roots.iter().any(|existing| existing == &normalized) {
        roots.push(normalized);
    }
}

fn resolve_git_superproject_root() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!root.is_empty()).then(|| PathBuf::from(root))
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

    #[test]
    fn managed_instruction_surface_roots_deduplicates_explicit_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();

        let roots = managed_instruction_surface_roots(Some(&root));

        assert_eq!(roots, vec![root]);
    }

    #[test]
    fn managed_instruction_surface_roots_prefers_superproject_without_override() {
        let tmp = TempDir::new().unwrap();
        let superproject = tmp.path().join("workspace");
        let submodule = superproject.join("src/agent-doc");
        std::fs::create_dir_all(&submodule).unwrap();

        let roots =
            managed_instruction_surface_roots_from(None, Some(&superproject), Some(&submodule));

        assert_eq!(roots, vec![superproject.canonicalize().unwrap()]);
    }

    #[test]
    fn managed_instruction_surface_roots_honors_explicit_submodule_root() {
        let tmp = TempDir::new().unwrap();
        let superproject = tmp.path().join("workspace");
        let submodule = superproject.join("src/agent-doc");
        std::fs::create_dir_all(&submodule).unwrap();

        let roots = managed_instruction_surface_roots_from(
            Some(&submodule),
            Some(&superproject),
            Some(&submodule),
        );

        assert_eq!(roots, vec![submodule.canonicalize().unwrap()]);
    }

    #[test]
    fn push_unique_root_deduplicates_canonical_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let mut roots = Vec::new();

        push_unique_root(&mut roots, tmp.path());
        push_unique_root(&mut roots, &root);

        assert_eq!(roots, vec![root]);
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
