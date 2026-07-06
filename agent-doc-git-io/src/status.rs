use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::dirs::{narrow_to_submodule, resolve_to_git_root};

/// Check whether `file` is inside a git work tree.
pub fn is_in_git_repo(file: &Path) -> bool {
    let dir = if file.is_absolute() {
        file.parent().unwrap_or(Path::new("/")).to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_tracked(file: &Path) -> Result<bool> {
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    is_repo_path_tracked(&git_root, &resolved)
}

pub fn is_repo_path_tracked(git_root: &Path, rel_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(rel_path)
        .output()?;
    Ok(output.status.success())
}

pub fn is_repo_path_ignored(git_root: &Path, rel_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["check-ignore", "--quiet", "--no-index", "--"])
        .arg(rel_path)
        .output()?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => anyhow::bail!(
            "git check-ignore failed for {}: {}",
            rel_path.display(),
            agent_doc_git::render_git_process_output(&output)
        ),
    }
}

pub fn is_untracked_repo_path_ignored(git_root: &Path, rel_path: &Path) -> Result<bool> {
    if is_repo_path_tracked(git_root, rel_path)? {
        return Ok(false);
    }
    is_repo_path_ignored(git_root, rel_path)
}

pub fn add(file: &Path) -> Result<()> {
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let output = Command::new("git")
        .current_dir(git_root.as_path())
        .args(["add", "--"])
        .arg(resolved.as_path())
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git add failed for {}: {}",
            resolved.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// List tracked modified paths in the owning git repo for `file`.
///
/// Paths are returned relative to the narrowed repo root (submodule when
/// applicable). Untracked files are excluded.
pub fn tracked_modified_paths(file: &Path) -> Result<Vec<String>> {
    if !is_in_git_repo(file) {
        return Ok(Vec::new());
    }
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let output = Command::new("git")
        .current_dir(git_root.as_path())
        .args([
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
            "--ignored=no",
        ])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    // #side-effect-exclude-submodules: a dirty submodule gitlink (e.g. an unrelated
    // sibling project sharing this superproject) is NEVER an agent-doc cycle
    // side-effect. Excluding submodule paths keeps closeout diagnostics focused
    // on files the cycle could have touched.
    let submodules = submodule_paths(&git_root);

    Ok(agent_doc_git::tracked_modified_paths_from_porcelain(
        &String::from_utf8_lossy(&output.stdout),
        &submodules,
    ))
}

pub fn focused_tracked_file_modified(file: &Path) -> Result<bool> {
    if !is_in_git_repo(file) {
        return Ok(false);
    }
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let output = Command::new("git")
        .current_dir(git_root.as_path())
        .args([
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
            "--ignored=no",
            "--",
        ])
        .arg(resolved.as_path())
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub fn tracked_side_effect_paths(file: &Path) -> Result<Vec<String>> {
    Ok(filter_tracked_side_effect_paths(
        file,
        tracked_modified_paths(file)?,
    ))
}

pub fn filter_tracked_side_effect_paths(
    file: &Path,
    modified_paths: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let doc_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    modified_paths
        .into_iter()
        .filter(|path| !path.starts_with(".agent-doc/"))
        .filter(|path| path != &doc_name && !path.ends_with(&format!("/{doc_name}")))
        .collect()
}

pub fn tracked_side_effect_note(file: &Path) -> Result<String> {
    let mut paths = tracked_side_effect_paths(file)?;
    if paths.is_empty() {
        return Ok(String::new());
    }
    let overflow = paths.len().saturating_sub(3);
    paths.truncate(3);
    let mut note = format!("; tracked side-effect edits: {}", paths.join(", "));
    if overflow > 0 {
        note.push_str(&format!(" (+{} more)", overflow));
    }
    Ok(note)
}

/// Paths registered as git submodules under `git_root` (from `git submodule
/// status`). Best-effort: a failed or absent submodule listing yields an empty
/// set, so callers simply do not exclude anything.
fn submodule_paths(git_root: &Path) -> std::collections::HashSet<String> {
    let Ok(output) = Command::new("git")
        .current_dir(git_root)
        .args(["submodule", "status"])
        .output()
    else {
        return std::collections::HashSet::new();
    };
    if !output.status.success() {
        return std::collections::HashSet::new();
    }
    agent_doc_git::parse_submodule_paths(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tracked_modified_paths_lists_tracked_dirty_files_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let doc = root.join("doc.md");
        fs::write(&doc, "before\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        fs::write(&doc, "after\n").unwrap();
        fs::write(root.join("untracked.md"), "ignored\n").unwrap();

        assert_eq!(tracked_modified_paths(&doc).unwrap(), vec!["doc.md"]);
    }

    #[test]
    fn is_tracked_and_add_cover_untracked_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let doc = root.join("doc.md");
        fs::write(&doc, "body\n").unwrap();

        assert!(!is_tracked(&doc).unwrap());
        add(&doc).unwrap();
        assert!(is_tracked(&doc).unwrap());
    }

    #[test]
    fn repo_path_status_helpers_detect_tracked_and_ignored_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        let tracked = Path::new("tracked.md");
        let ignored = Path::new("ignored.md");
        fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
        fs::write(root.join(tracked), "tracked\n").unwrap();
        fs::write(root.join(ignored), "ignored\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "tracked.md"])
            .output()
            .unwrap();

        assert!(is_repo_path_tracked(root, tracked).unwrap());
        assert!(!is_repo_path_tracked(root, ignored).unwrap());
        assert!(is_repo_path_ignored(root, ignored).unwrap());
        assert!(is_untracked_repo_path_ignored(root, ignored).unwrap());
    }

    #[test]
    fn untracked_ignored_check_returns_false_for_tracked_ignored_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        let ignored = Path::new("ignored.md");
        fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
        fs::write(root.join(ignored), "ignored\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "-f", "ignored.md"])
            .output()
            .unwrap();

        assert!(is_repo_path_tracked(root, ignored).unwrap());
        assert!(!is_untracked_repo_path_ignored(root, ignored).unwrap());
    }

    #[test]
    fn tracked_modified_paths_returns_empty_outside_git() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body\n").unwrap();

        assert!(tracked_modified_paths(&doc).unwrap().is_empty());
    }

    #[test]
    fn filter_tracked_side_effect_paths_excludes_agent_doc_and_session_doc() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("docs/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, "body\n").unwrap();

        let paths = filter_tracked_side_effect_paths(
            &doc,
            [
                ".agent-doc/snapshots/session.md".to_string(),
                "docs/session.md".to_string(),
                "other/session.md".to_string(),
                "src/lib.rs".to_string(),
            ],
        );

        assert_eq!(paths, vec!["src/lib.rs"]);
    }

    #[test]
    fn tracked_side_effect_note_limits_to_three_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        for path in [
            "docs/session.md",
            ".agent-doc/state.json",
            "a.txt",
            "b.txt",
            "c.txt",
            "src/d.txt",
        ] {
            fs::write(root.join(path), "before\n").unwrap();
        }
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        for path in [
            "docs/session.md",
            ".agent-doc/state.json",
            "a.txt",
            "b.txt",
            "c.txt",
            "src/d.txt",
        ] {
            fs::write(root.join(path), "after\n").unwrap();
        }

        let note = tracked_side_effect_note(&root.join("docs/session.md")).unwrap();

        assert_eq!(
            note,
            "; tracked side-effect edits: a.txt, b.txt, c.txt (+1 more)"
        );
    }
}
