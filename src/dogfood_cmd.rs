use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

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
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read dogfood source document {}", file.display()))?;
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
}
