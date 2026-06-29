use anyhow::Result;
use std::path::{Component, Path, PathBuf};

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

pub fn referenced_markdown_path(current_file: &Path, text: &str) -> Option<PathBuf> {
    referenced_markdown_path_checked(current_file, text)
        .ok()
        .flatten()
}

pub fn referenced_markdown_path_checked(
    current_file: &Path,
    text: &str,
) -> Result<Option<PathBuf>> {
    let current = normalize_path(current_file);
    let project_roots = project_roots_for(current_file);
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
        let mut possibilities = Vec::<PathBuf>::new();
        let has_project_prefix = first_component(path).is_some_and(|first| {
            project_roots.iter().any(|root| {
                root.file_name()
                    .is_some_and(|name| Component::Normal(name) == first)
            })
        });
        if path.is_absolute() {
            possibilities.push(path.to_path_buf());
        } else {
            for root in &project_roots {
                if let Some(stripped) = strip_redundant_project_prefix(root, path) {
                    possibilities.push(root.join(stripped));
                }
            }
            for root in &project_roots {
                possibilities.push(root.join(path));
                if let Some(stripped) = strip_redundant_project_prefix(root, path) {
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
        let mut existing = Vec::new();
        for resolved in possibilities {
            let resolved = normalize_path(&resolved);
            if resolved == current {
                matched_current = true;
                continue;
            }
            if resolved.exists() {
                if !existing.iter().any(|seen| seen == &resolved) {
                    existing.push(resolved);
                }
                continue;
            }
            fallback.get_or_insert(resolved);
        }
        if existing.len() > 1 {
            anyhow::bail!(
                "ambiguous markdown reference `{}` from {} matched multiple project roots: {}",
                candidate,
                current_file.display(),
                existing
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(resolved) = existing.into_iter().next() {
            return Ok(Some(resolved));
        }
        if has_project_prefix {
            let attempted = fallback
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| candidate.to_string());
            anyhow::bail!(
                "project-prefixed markdown reference `{}` from {} did not resolve to an existing file (first candidate: {})",
                candidate,
                current_file.display(),
                attempted
            );
        }
        if matched_current {
            continue;
        }
        if let Some(resolved) = fallback {
            return Ok(Some(resolved));
        }
    }
    Ok(None)
}

fn first_component(path: &Path) -> Option<Component<'_>> {
    path.components().next()
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

fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        match path.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Vec::new(),
        }
    };
    let mut roots = Vec::new();
    loop {
        if current.join(".agent-doc").is_dir() {
            roots.push(normalize_path(&current));
        }
        if !current.pop() {
            return roots;
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
    fn referenced_markdown_path_resolves_parent_project_prefix_from_nested_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        let nested = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let current = nested.join("tasks/root.md");
        let parent_target = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        let nested_target = nested.join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(&current, "# root\n").unwrap();
        std::fs::write(&parent_target, "# parent bugs\n").unwrap();
        std::fs::write(&nested_target, "# nested bugs\n").unwrap();

        let resolved = referenced_markdown_path_checked(
            &current,
            "Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md",
        )
        .unwrap()
        .unwrap();

        assert_eq!(resolved, parent_target.canonicalize().unwrap());
    }

    #[test]
    fn referenced_markdown_path_fails_on_ambiguous_nested_task_tree() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        let nested = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let current = nested.join("tasks/root.md");
        std::fs::write(&current, "# root\n").unwrap();
        std::fs::write(
            root.join("tasks/agent-doc/agent-doc-bugs2.md"),
            "# parent bugs\n",
        )
        .unwrap();
        std::fs::write(
            nested.join("tasks/agent-doc/agent-doc-bugs2.md"),
            "# nested bugs\n",
        )
        .unwrap();

        let err = referenced_markdown_path_checked(
            &current,
            "Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("ambiguous markdown reference"),
            "{err:#}"
        );
    }

    #[test]
    fn referenced_markdown_path_fails_missing_project_prefixed_target() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        let nested = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let current = nested.join("tasks/root.md");
        std::fs::write(&current, "# root\n").unwrap();

        let err = referenced_markdown_path_checked(
            &current,
            "Add to the backlog of agent-loop/tasks/agent-doc/missing.md",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("project-prefixed markdown reference"),
            "{err:#}"
        );
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
