use anyhow::Result;
use std::path::Path;

use agent_doc_git::SubmodulePointerDrift;

use crate::dirs::{narrow_to_submodule, resolve_to_git_root};
use crate::revision;

/// Check whether the parent repo's committed submodule pointer is current for a file in a submodule.
pub fn is_submodule_pointer_stale(file: &Path) -> bool {
    submodule_pointer_drift(file)
        .map(|drift| drift.is_some())
        .unwrap_or(false)
}

/// Return the exact parent gitlink drift for a document inside a submodule.
///
/// This compares the superproject's committed gitlink (`HEAD:<submodule>`)
/// against the submodule's current `HEAD`. Working-tree dirt inside the
/// submodule is intentionally ignored; closeout only owns the parent pointer
/// needed to make an already-created submodule commit reachable from the
/// parent repository.
///
/// `#splicectlcommit`: an untracked document that `.gitignore` matches never
/// crosses the submodule commit boundary — commit refuses it before the parent
/// pointer step — so a stale gitlink is never that document's to own, and
/// reporting it would fail its closeout with a recovery (`agent-doc commit`)
/// that can never succeed. Such a document reports no drift. The ignore check
/// runs only after a drift is observed, so the common no-drift path is unchanged.
pub fn submodule_pointer_drift(file: &Path) -> Result<Option<SubmodulePointerDrift>> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Ok(None);
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if !in_submodule {
        return Ok(None);
    }
    let rel = match git_root.strip_prefix(&super_root) {
        Ok(r) => r.to_string_lossy().to_string(),
        Err(_) => return Ok(None),
    };
    let Some(submodule_head) = revision::rev_parse(&git_root, "HEAD")? else {
        return Ok(None);
    };
    let parent_spec = format!("HEAD:{rel}");
    let parent_head = revision::rev_parse(&super_root, &parent_spec)?;
    let gitlink_current = parent_head.as_deref() == Some(submodule_head.as_str());
    if gitlink_current
        || resolved
            .strip_prefix(&git_root)
            .ok()
            .map(|doc_rel| crate::status::is_untracked_repo_path_ignored(&git_root, doc_rel))
            .transpose()?
            .unwrap_or(false)
    {
        Ok(None)
    } else {
        Ok(Some(SubmodulePointerDrift {
            relative_path: rel,
            parent_head,
            submodule_head,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

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
            let _ = std::env::set_current_dir(&self.previous);
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

    fn configure_repo(repo: &Path) {
        git(repo, &["config", "user.email", "test@test.com"]);
        git(repo, &["config", "user.name", "Test"]);
        git(repo, &["config", "protocol.file.allow", "always"]);
    }

    fn init_repo(repo: &Path) {
        git(repo, &["init"]);
        configure_repo(repo);
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

    fn project_with_submodule() -> (tempfile::TempDir, PathBuf, PathBuf) {
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
        configure_repo(&submodule_root);
        let doc = submodule_root.join("session.md");
        fs::write(&doc, "body\n").unwrap();
        (outer_dir, submodule_root, doc)
    }

    #[test]
    fn submodule_pointer_drift_is_none_when_parent_gitlink_matches() {
        let (outer_dir, _submodule_root, doc) = project_with_submodule();
        let _cwd = ScopedCurrentDir::set(outer_dir.path());

        assert_eq!(submodule_pointer_drift(&doc).unwrap(), None);
        assert!(!is_submodule_pointer_stale(&doc));
    }

    #[test]
    fn submodule_pointer_drift_reports_parent_gitlink_lag() {
        let (outer_dir, submodule_root, doc) = project_with_submodule();
        let _cwd = ScopedCurrentDir::set(outer_dir.path());
        commit_file(
            &submodule_root,
            "session.md",
            "changed\n",
            "advance submodule",
        );
        let submodule_head = revision::rev_parse(&submodule_root, "HEAD")
            .unwrap()
            .unwrap();

        let drift = submodule_pointer_drift(&doc).unwrap().unwrap();

        assert_eq!(drift.relative_path, "src/sub");
        assert_ne!(drift.parent_head.as_deref(), Some(submodule_head.as_str()));
        assert_eq!(drift.submodule_head, submodule_head);
        assert!(is_submodule_pointer_stale(&doc));
    }

    #[test]
    fn submodule_pointer_drift_is_none_for_ignored_untracked_document() {
        // `#splicectlcommit`: a stale gitlink is not an ignored scratch
        // document's to own — it can never cross the submodule commit boundary.
        let (outer_dir, submodule, _doc) = project_with_submodule();
        let _cwd = ScopedCurrentDir::set(outer_dir.path());
        commit_file(&submodule, ".gitignore", "tmp/\n", "ignore tmp");
        let scratch = submodule.join("tmp/scratch.md");
        fs::create_dir_all(scratch.parent().unwrap()).unwrap();
        fs::write(&scratch, "scratch\n").unwrap();
        let tracked = submodule.join(".gitignore");
        assert!(
            submodule_pointer_drift(&tracked).unwrap().is_some(),
            "fixture: the parent gitlink lags the submodule HEAD"
        );
        assert!(submodule_pointer_drift(&scratch).unwrap().is_none());
    }

    #[test]
    fn submodule_pointer_drift_is_none_for_top_level_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(root, "doc.md", "body\n", "init doc");
        let doc = root.join("doc.md");
        let _cwd = ScopedCurrentDir::set(root);

        assert_eq!(submodule_pointer_drift(&doc).unwrap(), None);
        assert!(!is_submodule_pointer_stale(&doc));
    }
}
