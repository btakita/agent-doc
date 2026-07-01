//! Git directory discovery and path resolution adapters.

use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn absolute_git_dir_at(git_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    path.canonicalize().ok().or(Some(path))
}

pub fn is_git_dir(path: &Path) -> bool {
    path.join("HEAD").is_file() && path.join("config").is_file()
}

pub fn collect_nested_git_dirs(root: &Path, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if is_git_dir(&path) {
            dirs.push(path);
            continue;
        }
        collect_nested_git_dirs(&path, dirs);
    }
}

pub fn nested_git_dirs_under(git_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_nested_git_dirs(&git_dir.join("modules"), &mut dirs);
    dirs
}

pub fn commit_lock_scope_path(git_root: &Path) -> Option<PathBuf> {
    absolute_git_dir_at(git_root)
}

pub fn commit_lock_path_for_git_root(git_root: &Path) -> Option<PathBuf> {
    let scope_path = commit_lock_scope_path(git_root)
        .or_else(|| git_root.canonicalize().ok())
        .unwrap_or_else(|| git_root.to_path_buf());
    let key = agent_doc_hash::path_string_hash(scope_path.to_string_lossy().as_ref());
    Some(
        scope_path
            .join("agent-doc-locks")
            .join(format!("commit-repo-{key}.lock")),
    )
}

pub fn push_workspace_access_dir(
    dirs: &mut Vec<PathBuf>,
    git_root: &Path,
    candidate: Option<PathBuf>,
) {
    let Some(dir) = candidate else {
        return;
    };
    if dir.starts_with(git_root) || dirs.contains(&dir) {
        return;
    }
    dirs.push(dir);
}

/// Return extra directories a workspace-scoped harness must be allowed to
/// write when operating on `file`.
///
/// Ordinary repos return an empty list because the working tree and `.git/`
/// both live under the repo root already. Submodule docs return the
/// superproject working tree (so the harness can patch parent-repo docs such
/// as shared backlog files) plus any external git metadata dirs needed for git
/// lifecycle operations.
pub fn workspace_access_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Vec::new();
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    let mut dirs = Vec::new();
    if in_submodule {
        push_workspace_access_dir(&mut dirs, &git_root, Some(super_root.clone()));
    }
    for dir in external_git_dirs_for_doc(file) {
        push_workspace_access_dir(&mut dirs, &git_root, Some(dir));
    }
    dirs
}

/// Return external git metadata directories a workspace-scoped harness must be
/// allowed to write when operating on `file`.
///
/// Plain repos expose any nested submodule gitdirs under `.git/modules/...`.
/// Submodules additionally expose their own external `.git/modules/...` gitdir
/// plus the superproject `.git` used by parent-pointer updates.
pub fn external_git_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Vec::new();
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    let mut dirs = Vec::new();
    if let Some(git_dir) = absolute_git_dir_at(&git_root) {
        push_workspace_access_dir(&mut dirs, &git_root, Some(git_dir.clone()));
        for nested in nested_git_dirs_under(&git_dir) {
            push_workspace_access_dir(&mut dirs, &git_root, Some(nested));
        }
    }
    if in_submodule {
        push_workspace_access_dir(&mut dirs, &git_root, absolute_git_dir_at(&super_root));
    }
    dirs
}

pub fn resolve_absolute_to_git_root(file: &Path, cwd_fallback: &Path) -> (PathBuf, PathBuf) {
    let parent = file.parent().unwrap_or(Path::new("/"));
    if let Some(superproject) = git_superproject_at(parent) {
        return (superproject, file.to_path_buf());
    }
    let root = git_toplevel_at(parent).unwrap_or_else(|| cwd_fallback.to_path_buf());
    (root, file.to_path_buf())
}

pub fn resolve_relative_to_git_root_from(cwd: &Path, file: &Path) -> Result<(PathBuf, PathBuf)> {
    let cwd_candidate = cwd.join(file);
    if cwd_candidate.exists() {
        let resolved = cwd_candidate.canonicalize().unwrap_or(cwd_candidate);
        return Ok(resolve_absolute_to_git_root(&resolved, cwd));
    }

    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    Ok((cwd.to_path_buf(), file.to_path_buf()))
}

/// Resolve a file path to absolute form, preferring the CWD's git root when
/// the same relative path exists in both the main repo and a submodule.
pub fn resolve_absolute_file_path(file: &Path) -> PathBuf {
    if file.is_absolute() {
        return file.to_path_buf();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidate = cwd.join(file);
    if candidate.exists() {
        candidate.canonicalize().unwrap_or(candidate)
    } else {
        file.to_path_buf()
    }
}

/// Resolve a relative path against the git root (superproject root if in a submodule).
/// Returns (git_root, resolved_file_path) so callers can run git commands in the correct repo.
pub fn resolve_to_git_root(file: &Path) -> Result<(PathBuf, PathBuf)> {
    if file.is_absolute() {
        return Ok(resolve_absolute_to_git_root(
            file,
            &std::env::current_dir().unwrap_or_default(),
        ));
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    resolve_relative_to_git_root_from(&cwd, file)
}

/// Get git toplevel from a specific directory.
pub fn git_toplevel_at(dir: &Path) -> Option<PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

/// Get the superproject working tree from a specific directory.
/// Returns `Some(path)` only when `dir` is inside a submodule. Returns `None`
/// for top-level repos or when git is unavailable.
pub fn git_superproject_at(dir: &Path) -> Option<PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

/// If `file` lives inside a submodule of `super_root`, return the submodule's
/// own git toplevel and `true`. Otherwise return `super_root` unchanged.
pub fn narrow_to_submodule(super_root: &Path, file: &Path) -> (PathBuf, bool) {
    let parent = file.parent().unwrap_or(Path::new("/"));
    if let Some(inner) = git_toplevel_at(parent)
        && inner != super_root
        && inner.starts_with(super_root)
    {
        return (inner, true);
    }
    (super_root.to_path_buf(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct ScopedCurrentDir {
        previous: PathBuf,
    }

    impl ScopedCurrentDir {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).unwrap();
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo: &Path) {
        git(repo, &["init"]);
        git(repo, &["config", "user.email", "test@test.com"]);
        git(repo, &["config", "user.name", "Test"]);
        git(repo, &["config", "protocol.file.allow", "always"]);
    }

    fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        git(repo, &["add", "--", rel]);
        git(repo, &["commit", "-m", msg, "--no-verify"]);
    }

    fn add_submodule(repo: &Path, origin: &Path, target: &str, msg: &str) {
        let url = format!("file://{}", origin.display());
        git(
            repo,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &url,
                target,
            ],
        );
        git(repo, &["commit", "-m", msg, "--no-verify"]);
    }

    #[test]
    fn external_git_dirs_for_submodule_include_submodule_and_parent_gitdirs() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");
        add_submodule(outer, sub_origin, "src/sub", "add submodule");

        let doc = outer.join("src/sub/session.md");
        fs::write(&doc, "test\n").unwrap();

        let dirs = external_git_dirs_for_doc(&doc);
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub")),
            "submodule gitdir should be exposed to workspace-write harnesses: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git")),
            "superproject gitdir should be exposed for pointer updates: {dirs:?}"
        );
    }

    #[test]
    fn external_git_dirs_for_submodule_include_nested_submodule_gitdirs() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        let nested_origin_dir = tempfile::TempDir::new().unwrap();
        let nested_origin = nested_origin_dir.path();
        init_repo(nested_origin);
        commit_file(nested_origin, "README.md", "# nested\n", "init nested");

        add_submodule(outer, sub_origin, "src/sub", "add submodule");

        let submodule_root = outer.join("src/sub");
        add_submodule(
            &submodule_root,
            nested_origin,
            "src/nested",
            "add nested submodule",
        );

        let doc = submodule_root.join("tasks/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, "test\n").unwrap();

        let dirs = external_git_dirs_for_doc(&doc);
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub")),
            "submodule gitdir should be exposed: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub/modules/src/nested")),
            "nested submodule gitdir should be exposed: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git")),
            "superproject gitdir should still be exposed: {dirs:?}"
        );
    }

    #[test]
    fn workspace_access_dirs_for_submodule_include_superproject_root_and_gitdirs() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");
        add_submodule(outer, sub_origin, "src/sub", "add submodule");

        let doc = outer.join("src/sub/session.md");
        fs::write(&doc, "test\n").unwrap();

        let dirs = workspace_access_dirs_for_doc(&doc);
        assert!(
            dirs.contains(&outer.to_path_buf()),
            "superproject working tree should be writable for parent-repo patchback targets: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub")),
            "submodule gitdir should still be exposed: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git")),
            "superproject gitdir should still be exposed: {dirs:?}"
        );
    }

    #[test]
    fn narrow_to_submodule_detects_submodule_root() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        add_submodule(outer, sub_origin, "src/sub", "add submodule");
        let submodule_root = outer.join("src/sub");
        let doc = submodule_root.join("session.md");
        fs::write(&doc, "x").unwrap();

        let (narrowed, in_sub) = narrow_to_submodule(outer, &doc);
        assert!(in_sub, "doc inside src/sub should be detected as submodule");
        assert_eq!(narrowed, submodule_root);
    }

    #[test]
    fn narrow_to_submodule_returns_super_root_for_non_submodule_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("session.md");
        fs::write(&doc, "x").unwrap();

        let (narrowed, in_sub) = narrow_to_submodule(root, &doc);
        assert!(
            !in_sub,
            "non-submodule file should not be detected as in-submodule"
        );
        assert_eq!(narrowed, root);
    }

    #[test]
    fn resolve_relative_path_prefers_existing_submodule_file_over_superproject_shadow() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(
            outer,
            "tasks/sampleorders.md",
            "outer shadow\n",
            "init outer",
        );

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(
            sub_origin,
            "tasks/sampleorders.md",
            "submodule doc\n",
            "init sub",
        );

        add_submodule(outer, sub_origin, "src/sample-app", "add submodule");

        let submodule_root = outer.join("src/sample-app");
        let (super_root, resolved) =
            resolve_relative_to_git_root_from(&submodule_root, Path::new("tasks/sampleorders.md"))
                .unwrap();

        assert_eq!(
            super_root, outer,
            "superproject root should still be returned for IPC/project-root coordination"
        );
        assert_eq!(
            resolved,
            submodule_root
                .join("tasks/sampleorders.md")
                .canonicalize()
                .unwrap(),
            "relative path should resolve to the existing submodule file, not the outer shadow file"
        );
    }

    #[test]
    fn resolve_absolute_file_path_returns_absolute_for_existing_relative() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let tasks = root.join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("plan.md");
        fs::write(&doc, "# Plan\n").unwrap();

        let _cwd = ScopedCurrentDir::set(&root);

        let resolved = resolve_absolute_file_path(Path::new("tasks/plan.md"));
        assert!(resolved.is_absolute(), "resolved path must be absolute");
        assert_eq!(resolved, doc, "must resolve to the CWD-relative file");
    }

    #[test]
    fn resolve_absolute_file_path_preserves_absolute_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let doc = root.join("test.md");
        fs::write(&doc, "test\n").unwrap();

        let resolved = resolve_absolute_file_path(&doc);
        assert_eq!(resolved, doc, "absolute paths must be returned as-is");
    }

    #[test]
    fn resolve_absolute_file_path_returns_relative_when_not_found() {
        let rel = Path::new("nonexistent/path.md");
        let resolved = resolve_absolute_file_path(rel);
        assert_eq!(
            resolved, rel,
            "missing files should return the original path"
        );
    }

    #[test]
    fn commit_lock_path_is_scoped_to_absolute_git_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);

        let scope = commit_lock_scope_path(root).unwrap();
        let lock_path = commit_lock_path_for_git_root(root).unwrap();

        assert_eq!(scope, root.join(".git"));
        assert!(lock_path.starts_with(&scope));
        assert_eq!(lock_path.parent().unwrap(), scope.join("agent-doc-locks"));
        assert!(
            lock_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("commit-repo-")
        );
    }
}
