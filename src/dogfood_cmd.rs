use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

use agent_doc_frontmatter::frontmatter::{self, DogfoodConfig, DogfoodOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DogfoodPolicyReport {
    pub enabled: bool,
    pub source_document: PathBuf,
    pub target: Option<PathBuf>,
    pub self_target: bool,
    pub auto: bool,
}

pub fn resolve(file: &Path) -> Result<DogfoodPolicyReport> {
    let source_document = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "dogfood_policy_document",
    )
    .with_context(|| {
        format!(
            "failed to resolve dogfood source document {}",
            file.display()
        )
    })?;
    let policy = agent_doc_frontmatter_io::session::effective_dogfood_for_file(&content, file)?;
    Ok(DogfoodPolicyReport {
        enabled: policy.enabled,
        source_document,
        target: policy.target,
        self_target: policy.self_target,
        auto: policy.auto,
    })
}

pub fn run(file: &Path, json: bool) -> Result<()> {
    let report = resolve(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.enabled {
        let target = report
            .target
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(none)".to_string());
        println!(
            "dogfood enabled: source={} target={} auto={} self_target={}",
            report.source_document.display(),
            target,
            report.auto,
            report.self_target
        );
    } else {
        println!(
            "dogfood disabled: source={}",
            report.source_document.display()
        );
    }
    Ok(())
}

/// Enable document-local dogfood routing through the public `dogfood:` field.
///
/// `path = None` targets the current document. `auto` defaults to false so a
/// caller must explicitly opt into queue activation. Reads and writes use the
/// editor-aware document authority; no disk-only mutation is permitted while
/// an editor owns the document.
pub fn set(file: &Path, path: Option<&Path>, auto: Option<bool>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "dogfood_set_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.dogfood_mode = Some(config_for_set(path, auto.unwrap_or(false))?);
    let updated = frontmatter::write_preserving(&content, &fm, body)?;

    // Resolve the just-produced policy before crossing the write boundary. In
    // particular, this proves an explicit destination is an unambiguous
    // project-relative markdown document.
    let policy = agent_doc_frontmatter_io::session::effective_dogfood_for_file(&updated, file)?;
    if !policy.enabled || policy.target.is_none() {
        anyhow::bail!("dogfood set produced a disabled or unresolved policy");
    }

    if updated == content {
        eprintln!("dogfood policy already set in {}", file.display());
        return Ok(());
    }

    agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated)
        .with_context(|| format!("failed to write {}", file.display()))?;
    eprintln!(
        "set dogfood policy in {}: target={} auto={}",
        file.display(),
        policy
            .target
            .as_deref()
            .map(|target| target.display().to_string())
            .unwrap_or_else(|| "(none)".to_string()),
        policy.auto
    );
    Ok(())
}

fn config_for_set(path: Option<&Path>, auto: bool) -> Result<DogfoodConfig> {
    let path = path.map(validate_project_relative_markdown).transpose()?;
    Ok(match (path, auto) {
        (None, false) => DogfoodConfig::Enabled(true),
        (Some(path), false) => DogfoodConfig::Target(path),
        (path, true) => DogfoodConfig::Options(DogfoodOptions { path, auto: true }),
    })
}

fn validate_project_relative_markdown(path: &Path) -> Result<String> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        anyhow::bail!(
            "dogfood destination must be a project-relative path without `..`: {}",
            path.display()
        );
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
        anyhow::bail!(
            "dogfood destination must be a markdown document: {}",
            path.display()
        );
    }
    let path = path
        .to_str()
        .filter(|path| !path.trim().is_empty())
        .context("dogfood destination must be valid UTF-8")?;
    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_preserves_typed_self_target_and_auto_policy() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(
            &doc,
            "---\ndogfood:\n  path: session.md\n  auto: true\n---\n",
        )
        .unwrap();

        let report = resolve(&doc).unwrap();

        assert!(report.enabled);
        assert!(report.auto);
        assert!(report.self_target);
        assert_eq!(report.target, Some(doc.clone()));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["target"], doc.display().to_string());
    }

    #[test]
    fn set_defaults_to_current_document_and_preserves_unrelated_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        let original = "---\n# keep this comment\nmodel: 'gpt-5'\nagent_doc_dogfood: false\n---\n# Session\n\nBody stays byte exact.\n";
        std::fs::write(&doc, original).unwrap();

        set(&doc, None, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated,
            "---\n# keep this comment\nmodel: 'gpt-5'\ndogfood: true\n---\n# Session\n\nBody stays byte exact.\n"
        );
        let policy = resolve(&doc).unwrap();
        assert!(policy.enabled);
        assert!(!policy.auto);
        assert!(policy.self_target);

        set(&doc, None, Some(false)).unwrap();
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), updated);
    }

    #[test]
    fn set_path_and_auto_writes_public_map_and_preserves_body() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tasks")).unwrap();
        let doc = tmp.path().join("session.md");
        let target = tmp.path().join("tasks/bugs.md");
        std::fs::write(&doc, "# Session\n\nBody\n").unwrap();
        std::fs::write(&target, "# Bugs\n").unwrap();

        set(&doc, Some(Path::new("tasks/bugs.md")), Some(true)).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated,
            "---\ndogfood:\n  path: tasks/bugs.md\n  auto: true\n---\n# Session\n\nBody\n"
        );
        let policy = resolve(&doc).unwrap();
        assert_eq!(policy.target, Some(target));
        assert!(policy.auto);
        assert!(!policy.self_target);
    }

    #[test]
    fn set_rejects_non_project_relative_or_non_markdown_destinations() {
        assert!(config_for_set(Some(Path::new("../bugs.md")), false).is_err());
        assert!(config_for_set(Some(Path::new("/tmp/bugs.md")), false).is_err());
        assert!(config_for_set(Some(Path::new("tasks/bugs.txt")), false).is_err());
    }
}
