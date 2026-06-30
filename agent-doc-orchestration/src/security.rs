use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_frontmatter::frontmatter::Frontmatter;
use agent_doc_frontmatter::security_review::{
    SecurityReviewSubject, cross_document_security_review_decision,
};

pub fn enforce_cross_document_review(
    action: &str,
    source: &Path,
    source_fm: &Frontmatter,
    target: &Path,
    target_fm: Option<&Frontmatter>,
) -> Result<()> {
    let same_document = same_document(source, target);
    let decision = cross_document_security_review_decision(same_document, source_fm, target_fm);
    if decision.is_allowed() {
        return Ok(());
    }

    let missing: Vec<String> = decision
        .missing
        .iter()
        .map(|subject| match subject {
            SecurityReviewSubject::Source => source.display().to_string(),
            SecurityReviewSubject::Target => target.display().to_string(),
        })
        .collect();

    anyhow::bail!(
        "{} across documents is blocked for shared agent-doc files without `agent_doc_security_review`. Missing review on: {}. Cross-document transfers and plan/backlog reads can expose one user's backlog, icebox, or plan content to another user.",
        action,
        missing.join(", ")
    );
}

fn same_document(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_frontmatter() -> Frontmatter {
        agent_doc_frontmatter::frontmatter::parse("---\nagent_doc_collaboration: shared\n---\n")
            .expect("shared frontmatter")
            .0
    }

    #[test]
    fn enforce_cross_document_review_formats_shared_missing_review_error() {
        let shared = shared_frontmatter();
        let err = enforce_cross_document_review(
            "transfer",
            Path::new("/tmp/a.md"),
            &shared,
            Path::new("/tmp/b.md"),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("blocked for shared agent-doc files")
        );
    }

    #[test]
    fn enforce_cross_document_review_allows_same_path_without_review() {
        let shared = shared_frontmatter();

        enforce_cross_document_review(
            "transfer",
            Path::new("/tmp/a.md"),
            &shared,
            Path::new("/tmp/a.md"),
            None,
        )
        .expect("same path does not need cross-document review");
    }
}
