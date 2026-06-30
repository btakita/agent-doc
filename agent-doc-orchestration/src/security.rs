use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_frontmatter::frontmatter::{CollaborationMode, Frontmatter};

pub fn enforce_cross_document_review(
    action: &str,
    source: &Path,
    source_fm: &Frontmatter,
    target: &Path,
    target_fm: Option<&Frontmatter>,
) -> Result<()> {
    if same_document(source, target) {
        return Ok(());
    }

    let mut missing = Vec::new();
    if source_fm.collaboration_mode() == CollaborationMode::Shared
        && !source_fm.has_security_review()
    {
        missing.push(source.display().to_string());
    }
    if let Some(fm) = target_fm
        && fm.collaboration_mode() == CollaborationMode::Shared
        && !fm.has_security_review()
    {
        missing.push(target.display().to_string());
    }

    if missing.is_empty() {
        return Ok(());
    }

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

    #[test]
    fn enforce_cross_document_review_only_blocks_shared_without_review() {
        let shared = Frontmatter {
            collaboration: Some(CollaborationMode::Shared),
            ..Default::default()
        };
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
}
