use anyhow::Result;
use std::path::{Component, Path, PathBuf};

use crate::frontmatter::{CollaborationMode, Frontmatter};

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

pub fn referenced_markdown_path(current_file: &Path, text: &str) -> Option<PathBuf> {
    let current = normalize_path(current_file);
    for raw in text.split_whitespace() {
        let candidate = raw.trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | ':'
            )
        });
        if !candidate.ends_with(".md") {
            continue;
        }

        let path = Path::new(candidate);
        let mut possibilities = Vec::new();
        if path.is_absolute() {
            possibilities.push(path.to_path_buf());
        } else {
            if let Some(root) = find_project_root(current_file) {
                possibilities.push(root.join(path));
                if let Some(stripped) = strip_redundant_project_prefix(&root, path) {
                    possibilities.push(root.join(stripped));
                }
            }
            possibilities.push(
                current_file
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(path),
            );
        }

        let mut fallback = None;
        let mut matched_current = false;
        for resolved in possibilities {
            let resolved = normalize_path(&resolved);
            if resolved == current {
                matched_current = true;
                break;
            }
            if resolved.exists() {
                return Some(resolved);
            }
            fallback.get_or_insert(resolved);
        }
        if matched_current {
            continue;
        }
        if let Some(resolved) = fallback {
            return Some(resolved);
        }
    }
    None
}

fn strip_redundant_project_prefix(root: &Path, path: &Path) -> Option<PathBuf> {
    let root_name = root.file_name()?;
    let mut components = path.components();
    let Component::Normal(first) = components.next()? else {
        return None;
    };
    if first != root_name {
        return None;
    }
    let stripped = components.as_path();
    (!stripped.as_os_str().is_empty()).then(|| stripped.to_path_buf())
}

fn same_document(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referenced_markdown_path_ignores_self_reference() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let file = dir.path().join("tasks/plan.md");
        std::fs::write(&file, "# plan\n").unwrap();
        assert_eq!(
            referenced_markdown_path(&file, "Update tasks/plan.md before closing"),
            None
        );
    }

    #[test]
    fn referenced_markdown_path_finds_other_doc_reference() {
        let file = Path::new("/tmp/tasks/plan.md");
        let path = referenced_markdown_path(file, "Follow tasks/other-plan.md next").unwrap();
        assert!(path.ends_with("tasks/other-plan.md"));
    }

    #[test]
    fn referenced_markdown_path_strips_redundant_project_prefix() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        let current = root.join("tasks/software/tmux-router.md");
        let target = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(&target, "# bugs\n").unwrap();

        let resolved = referenced_markdown_path(
            &current,
            "Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md",
        )
        .unwrap();

        assert_eq!(resolved, target.canonicalize().unwrap());
    }

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
