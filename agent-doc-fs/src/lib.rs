use anyhow::Result;
use std::path::{Component, Path, PathBuf};

/// Walk up the directory tree from `path` to find the directory containing
/// `.agent-doc` (the project root). Returns `None` if no such ancestor exists.
pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_file() { path.parent()? } else { path };
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Canonicalize `path` first, then delegate to [`find_project_root`].
/// Returns `None` if canonicalization fails or no `.agent-doc` ancestor exists.
pub fn find_project_root_canonical(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    find_project_root(&canonical)
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

pub fn read_optional_text(path: &Path) -> Result<Option<String>> {
    read_optional(path, |path| std::fs::read_to_string(path))
}

pub fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    read_optional(path, |path| std::fs::read(path))
}

fn read_optional<T, F>(path: &Path, read: F) -> Result<Option<T>>
where
    F: FnOnce(&Path) -> std::io::Result<T>,
{
    match read(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
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
    use super::{read_optional, referenced_markdown_path, referenced_markdown_path_checked};
    use std::path::Path;

    #[test]
    fn read_optional_returns_none_on_not_found() {
        let value: Option<String> = read_optional(Path::new("missing"), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        })
        .unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn read_optional_preserves_other_errors() {
        let err = read_optional::<String, _>(Path::new("denied"), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        let message = err.to_string().to_ascii_lowercase();
        assert!(
            message.contains("permission denied"),
            "unexpected error: {err}"
        );
    }

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
}
