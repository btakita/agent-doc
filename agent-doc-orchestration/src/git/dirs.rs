//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn absolute_git_dir_at(git_root: &Path) -> Option<PathBuf> {
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

pub(crate) fn is_git_dir(path: &Path) -> bool {
    path.join("HEAD").is_file() && path.join("config").is_file()
}

pub(crate) fn collect_nested_git_dirs(root: &Path, dirs: &mut Vec<PathBuf>) {
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

pub(crate) fn nested_git_dirs_under(git_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_nested_git_dirs(&git_dir.join("modules"), &mut dirs);
    dirs
}

pub(crate) fn commit_lock_scope_path(git_root: &Path) -> Option<PathBuf> {
    absolute_git_dir_at(git_root)
}

pub(crate) fn commit_lock_path_for_git_root(git_root: &Path) -> Option<PathBuf> {
    let scope_path = commit_lock_scope_path(git_root)
        .or_else(|| git_root.canonicalize().ok())
        .unwrap_or_else(|| git_root.to_path_buf());
    let key = crate::snapshot::doc_hash_from_str(scope_path.to_string_lossy().as_ref());
    Some(
        scope_path
            .join("agent-doc-locks")
            .join(format!("commit-repo-{}.lock", key)),
    )
}

pub(crate) fn push_workspace_access_dir(
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

pub(crate) fn resolve_absolute_to_git_root(
    file: &Path,
    cwd_fallback: &Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let parent = file.parent().unwrap_or(Path::new("/"));
    if let Some(superproject) = git_superproject_at(parent) {
        return (superproject, file.to_path_buf());
    }
    let root = git_toplevel_at(parent).unwrap_or_else(|| cwd_fallback.to_path_buf());
    (root, file.to_path_buf())
}

pub(crate) fn resolve_relative_to_git_root_from(
    cwd: &Path,
    file: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let cwd_candidate = cwd.join(file);
    if cwd_candidate.exists() {
        let resolved = cwd_candidate.canonicalize().unwrap_or(cwd_candidate);
        return Ok(resolve_absolute_to_git_root(&resolved, cwd));
    }

    // Try superproject first (handles submodule CWD case)
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Try git toplevel
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
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
///
/// When route.rs sends trigger commands to tmux panes, relative paths resolve
/// against the pane's CWD — which may be narrowed to a submodule root. This
/// function canonicalizes relative paths against the process CWD so the trigger
/// always targets the correct file.
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
pub fn resolve_to_git_root(file: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
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
pub fn git_toplevel_at(dir: &Path) -> Option<std::path::PathBuf> {
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
                Some(std::path::PathBuf::from(s))
            }
        })
}

/// Get the superproject working tree from a specific directory.
/// Returns `Some(path)` only when `dir` is inside a submodule. Returns `None`
/// for top-level repos or when git is unavailable.
pub fn git_superproject_at(dir: &Path) -> Option<std::path::PathBuf> {
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
                Some(std::path::PathBuf::from(s))
            }
        })
}

/// If `file` lives inside a submodule of `super_root`, return the submodule's
/// own git toplevel and `true`. Otherwise return `super_root` unchanged.
///
/// This lets `commit()` run git operations from within the submodule (where
/// they're valid) instead of the parent repo (where the file appears as both
/// a submodule entry and a tracked path, causing `update-index --cacheinfo`
/// and `git add` to refuse the path with "appears as both a file and as a
/// directory" / "Pathspec ... is in submodule" errors).
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
    #![allow(unused_imports)]
    use super::*;
    #[test]
    fn commit_in_submodule_routes_through_submodule_repo() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        // Initialize a "submodule" repo inside a temp dir
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        // Allow file:// transport inside this test invocation
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        // Initialize the outer repo
        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        // Add the submodule
        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_path = outer.join("src/sub");
        // Configure the checked-out submodule for committing
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Sanity: narrow_to_submodule returns the submodule path, not the outer
        let doc = submodule_path.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let (narrowed, in_sub) = narrow_to_submodule(outer, &doc);
        assert!(in_sub, "doc inside src/sub should be detected as submodule");
        assert_eq!(
            narrowed, submodule_path,
            "narrowed root should be the submodule toplevel"
        );

        // Stage + commit the file inside the submodule so it's tracked
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Modify the file (simulate an agent response landing) and create snapshot
        let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
        fs::write(&doc, new_content).unwrap();
        let snap_rel = crate::snapshot::path_for(&doc).unwrap();
        // The snapshot path is computed against the project root (walks for .agent-doc).
        // For this test, ensure the .agent-doc dir exists at the outer root and write the snapshot there.
        let project_root = crate::snapshot::find_project_root(&doc.canonicalize().unwrap())
            .unwrap_or_else(|| outer.to_path_buf());
        let snap_abs = project_root.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, new_content).unwrap();

        // Run commit() — should route through the submodule, succeed, and update parent pointer
        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "commit should succeed for submodule file: {:?}",
            result.err()
        );

        // Verify the submodule has a new agent-doc commit
        let sub_log = Command::new("git")
            .current_dir(&submodule_path)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let sub_log_str = String::from_utf8_lossy(&sub_log.stdout);
        assert!(
            sub_log_str.contains("agent-doc(session)"),
            "submodule git log should contain agent-doc commit, got:\n{sub_log_str}"
        );

        // Verify the parent has a submodule-pointer commit
        let outer_log = Command::new("git")
            .current_dir(outer)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let outer_log_str = String::from_utf8_lossy(&outer_log.stdout);
        assert!(
            outer_log_str.contains("(submodule pointer)"),
            "parent git log should contain pointer-update commit, got:\n{outer_log_str}"
        );
    }
    #[test]
    fn external_git_dirs_for_submodule_include_submodule_and_parent_gitdirs() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

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
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

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
    fn narrow_to_submodule_returns_super_root_for_non_submodule_file() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
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
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();

        let shadow_dir = outer.join("tasks");
        fs::create_dir_all(&shadow_dir).unwrap();
        fs::write(shadow_dir.join("monsterrodholders.md"), "outer shadow\n").unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(sub_origin.join("tasks")).unwrap();
        fs::write(
            sub_origin.join("tasks/monsterrodholders.md"),
            "submodule doc\n",
        )
        .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_add = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/boost-client",
            ])
            .output()
            .unwrap();
        assert!(
            sub_add.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_add.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_root = outer.join("src/boost-client");
        let (super_root, resolved) = resolve_relative_to_git_root_from(
            &submodule_root,
            Path::new("tasks/monsterrodholders.md"),
        )
        .unwrap();

        assert_eq!(
            super_root, outer,
            "superproject root should still be returned for IPC/project-root coordination"
        );
        assert_eq!(
            resolved,
            submodule_root
                .join("tasks/monsterrodholders.md")
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
        std::fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("plan.md");
        std::fs::write(&doc, "# Plan\n").unwrap();

        let _cwd = crate::test_support::ScopedCurrentDir::set(&root);

        let resolved = resolve_absolute_file_path(Path::new("tasks/plan.md"));
        assert!(resolved.is_absolute(), "resolved path must be absolute");
        assert_eq!(resolved, doc, "must resolve to the CWD-relative file");
    }
    #[test]
    fn resolve_absolute_file_path_preserves_absolute_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let doc = root.join("test.md");
        std::fs::write(&doc, "test\n").unwrap();

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
    fn commit_serializes_closeout_per_git_root() {
        use std::fs;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_a = root.join("plan-a.md");
        let doc_b = root.join("plan-b.md");
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc_a, initial).unwrap();
        fs::write(&doc_b, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "plan-a.md", "plan-b.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let updated_a =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nA\n\n";
        let updated_b =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nB\n\n";
        fs::write(&doc_a, updated_a).unwrap();
        fs::write(&doc_b, updated_b).unwrap();
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        crate::snapshot::save(&doc_a, updated_a).unwrap();
        crate::snapshot::save(&doc_b, updated_b).unwrap();

        let lock_path = commit_lock_path_for_git_root(root).unwrap();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let held = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        held.lock_exclusive().unwrap();

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for doc in [doc_a.clone(), doc_b.clone()] {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                let result = commit(&doc);
                tx.send((doc, result)).unwrap();
            }));
        }
        drop(tx);

        assert!(
            rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "both commit threads should be waiting on the shared repo lock"
        );

        held.unlock().unwrap();

        let results = vec![
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        ];
        for handle in handles {
            handle.join().unwrap();
        }

        for (doc, result) in results {
            let did_commit = result
                .unwrap_or_else(|e| panic!("commit should succeed for {}: {e}", doc.display()));
            assert!(did_commit, "{} should create a git commit", doc.display());
        }

        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-4"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(plan-a):"),
            "git log should contain the plan-a closeout, got:\n{log_str}"
        );
        assert!(
            log_str.contains("agent-doc(plan-b):"),
            "git log should contain the plan-b closeout, got:\n{log_str}"
        );
    }
    /// #ipc-drift-writeback-serialize: two supervisors writing back to the same
    /// superproject must serialize on one repo-scoped lock, so a submodule doc's
    /// parent-pointer commit cannot interleave with a concurrent superproject-root
    /// commit. Both must land cleanly (no interleaved partial commits, no
    /// stranded response) once the shared lock is released.
    #[test]
    fn superproject_writeback_serializes_pointer_update_and_root_commit() {
        use std::fs;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        fn git(cwd: &Path, args: &[&str]) {
            let out = Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // Submodule origin repo.
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        git(sub_origin, &["init"]);
        git(sub_origin, &["config", "user.email", "test@test.com"]);
        git(sub_origin, &["config", "user.name", "Test"]);
        git(sub_origin, &["config", "protocol.file.allow", "always"]);
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        git(sub_origin, &["add", "README.md"]);
        git(sub_origin, &["commit", "-m", "init sub", "--no-verify"]);

        // Superproject repo with the submodule wired in.
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        git(outer, &["init"]);
        git(outer, &["config", "user.email", "test@test.com"]);
        git(outer, &["config", "user.name", "Test"]);
        git(outer, &["config", "protocol.file.allow", "always"]);
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        git(outer, &["add", "README.md"]);
        git(outer, &["commit", "-m", "init outer", "--no-verify"]);
        let sub_url = format!("file://{}", sub_origin.display());
        git(
            outer,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ],
        );
        git(outer, &["commit", "-m", "add submodule", "--no-verify"]);

        let submodule_path = outer.join("src/sub");
        git(&submodule_path, &["config", "user.email", "test@test.com"]);
        git(&submodule_path, &["config", "user.name", "Test"]);

        // A submodule-owned session doc and a superproject-root session doc.
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        let sub_doc = submodule_path.join("session.md");
        let root_doc = outer.join("root-doc.md");
        fs::write(&sub_doc, initial).unwrap();
        fs::write(&root_doc, initial).unwrap();
        git(&submodule_path, &["add", "session.md"]);
        git(
            &submodule_path,
            &["commit", "-m", "add sub doc", "--no-verify"],
        );
        git(outer, &["add", "root-doc.md"]);
        git(outer, &["commit", "-m", "add root doc", "--no-verify"]);

        // Agent responses land in both docs; snapshots stage the committed image.
        let sub_updated =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nSUB\n\n";
        let root_updated =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nROOT\n\n";
        fs::write(&sub_doc, sub_updated).unwrap();
        fs::write(&root_doc, root_updated).unwrap();
        fs::create_dir_all(outer.join(".agent-doc/snapshots")).unwrap();
        crate::snapshot::save(&sub_doc, sub_updated).unwrap();
        crate::snapshot::save(&root_doc, root_updated).unwrap();

        // Externally hold the superproject commit lock so both write-back paths
        // (the submodule pointer update and the root commit) must wait on it.
        let super_lock_path = commit_lock_path_for_git_root(outer).unwrap();
        fs::create_dir_all(super_lock_path.parent().unwrap()).unwrap();
        let held = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&super_lock_path)
            .unwrap();
        held.lock_exclusive().unwrap();

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for doc in [sub_doc.clone(), root_doc.clone()] {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                let result = commit(&doc);
                tx.send((doc, result)).unwrap();
            }));
        }
        drop(tx);

        // Neither write-back may finish while the superproject lock is held: the
        // root commit blocks at lock acquisition and the submodule pointer update
        // blocks before touching the parent index.
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "neither superproject write-back should complete while the shared lock is held"
        );

        held.unlock().unwrap();

        let results = vec![
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        ];
        for handle in handles {
            handle.join().unwrap();
        }
        for (doc, result) in results {
            let did_commit = result
                .unwrap_or_else(|e| panic!("commit should succeed for {}: {e}", doc.display()));
            assert!(did_commit, "{} should create a git commit", doc.display());
        }

        // The superproject HEAD chain holds both write-backs without interleave:
        // the submodule pointer update and the root-doc closeout each landed.
        let outer_log = Command::new("git")
            .current_dir(outer)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let outer_log_str = String::from_utf8_lossy(&outer_log.stdout);
        assert!(
            outer_log_str.contains("(submodule pointer)"),
            "superproject log should contain the submodule pointer update, got:\n{outer_log_str}"
        );
        assert!(
            outer_log_str.contains("agent-doc(root-doc):"),
            "superproject log should contain the root-doc closeout, got:\n{outer_log_str}"
        );

        // The captured response landed in each repo's HEAD (no stuck_captured_cycle).
        let sub_head = Command::new("git")
            .current_dir(&submodule_path)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&sub_head.stdout).contains("SUB"),
            "submodule HEAD should carry the captured response"
        );
        let root_head = Command::new("git")
            .current_dir(outer)
            .args(["show", "HEAD:root-doc.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&root_head.stdout).contains("ROOT"),
            "superproject HEAD should carry the captured response"
        );
    }
}
