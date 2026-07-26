//! Project-root resolution adapters.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve the current working directory to the nearest `.agent-doc` project
/// root.
pub fn project_root_from_cwd() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to get CWD")?;
    agent_doc_fs::find_project_root(&cwd)
        .context("no .agent-doc/ directory found (walked up from CWD)")
}

/// Resolve `path` to its nearest `.agent-doc` project root.
pub fn project_root_containing(path: &Path) -> Option<PathBuf> {
    agent_doc_fs::find_project_root(path)
}

/// An explicitly-supplied root counts as supplied only when it is a real path.
///
/// `Some("")` is not a project root — it is a *missing* one wearing the shape of an
/// answer. Passed through, it reaches `Command::current_dir("")`, which fails with
/// `ENOENT` however healthy everything else is; the controller launcher carries a
/// dedicated guard naming exactly that shape. Treating an empty explicit root as
/// absent lets the normal document/cwd resolution produce a usable root instead.
///
/// Edge hardening, not a claimed fix for any particular outage: an empty path is
/// never a valid project root, whatever produced it.
fn explicit_root(project_root: Option<&Path>) -> Option<&Path> {
    project_root.filter(|root| !root.as_os_str().is_empty())
}

/// Resolve an explicit project root, or fall back to the nearest `.agent-doc`
/// ancestor of the current directory.
pub fn project_root_or_cwd(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit_root(project_root) {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Resolve an explicit project root, then a document's project root, then the
/// current working directory's project root.
pub fn project_root_for_target_or_cwd(
    project_root: Option<&Path>,
    document: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(root) = explicit_root(project_root) {
        return Ok(root.to_path_buf());
    }
    if let Some(document) = document
        && let Some(root) = agent_doc_fs::find_project_root(document)
    {
        return Ok(root);
    }
    project_root_or_cwd(None)
}

/// Resolve a file to the nearest `.agent-doc` root, or fall back to the
/// canonical parent directory when the file is outside an agent-doc project.
pub fn project_root_or_file_parent(file: &Path) -> Result<PathBuf> {
    match agent_doc_fs::find_project_root_canonical(file) {
        Some(root) => Ok(root),
        None => {
            let canonical = file
                .canonicalize()
                .with_context(|| format!("failed to canonicalize {}", file.display()))?;
            Ok(canonical.parent().unwrap_or(Path::new(".")).to_path_buf())
        }
    }
}

/// Resolve an optional CLI project-root argument to an agent-doc project root.
///
/// Relative arguments are interpreted from the current working directory. If no
/// `.agent-doc` ancestor is found, an explicit root that itself contains `.git`
/// or `.agent-doc` is accepted as a project root.
pub fn project_root_from_arg(root: Option<&Path>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let start = match root {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd,
    };
    let start = start.canonicalize().unwrap_or(start);
    agent_doc_fs::find_project_root(&start)
        .or_else(|| {
            if start.join(".git").exists() || start.join(".agent-doc").exists() {
                Some(start.to_path_buf())
            } else {
                None
            }
        })
        .with_context(|| format!("no project root found from {}", start.display()))
}

/// Resolve the IPC project root for `canonical` (an already-canonicalized file
/// path). Uses the nearest `.agent-doc/` directory to match editor plugin root
/// routing, constrained to the current git toplevel so an outer workspace root
/// cannot capture IPC sidecars for a nested repo.
pub fn resolve_ipc_project_root(canonical: &Path) -> PathBuf {
    // `Path::parent` returns `Some("")` — not `None` — for a bare relative file
    // name, so `unwrap_or("/")` does not cover the empty case and the empty
    // string flows through as if it were a directory. It reaches
    // `Command::current_dir`, which can only fail; the launch guard then
    // correctly reports `project root "" is empty or not a directory ...
    // retrying cannot help`, and every agent-doc command on the project fails
    // until a controller is started by hand (2026-07-26). This function's name
    // says `canonical`, but nothing enforces that, and a caller that is merely
    // wrong about its argument should not be able to produce an unusable root.
    //
    // Same lesson the empty-explicit-root test below already records: an empty
    // path is a *missing* answer wearing the shape of one.
    let parent = canonical
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
    let git_toplevel = agent_doc_git_io::dirs::git_toplevel_at(&parent);

    if let Some(root) = agent_doc_fs::find_project_root(canonical)
        && git_toplevel
            .as_ref()
            .is_none_or(|toplevel| root.starts_with(toplevel))
    {
        return root;
    }

    git_toplevel.unwrap_or(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// The same lesson one call deeper: a bare relative file name has
    /// `parent() == Some("")`, so the `unwrap_or("/")` guard never fired and the
    /// empty string became the returned project root. It then reached
    /// `Command::current_dir` and wedged every agent-doc command on the project
    /// behind `project root "" is empty or not a directory` until a controller
    /// was started by hand.
    #[test]
    fn a_bare_file_name_never_resolves_to_an_empty_project_root() {
        let resolved = resolve_ipc_project_root(Path::new("plan.md"));
        assert!(
            !resolved.as_os_str().is_empty(),
            "an empty root is unusable by every caller; it must resolve to a real directory"
        );
        assert!(
            resolved.is_dir(),
            "the fallback must be a directory `Command::current_dir` can accept, got {resolved:?}"
        );
    }

    /// An empty explicit root is a *missing* root wearing the shape of an answer.
    /// Returning it verbatim sends `""` to `Command::current_dir`, which can only
    /// fail — so it must be treated as absent and resolved normally instead.
    #[test]
    fn an_empty_explicit_project_root_is_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let document = root.join("plan.md");
        std::fs::write(&document, "# plan\n").unwrap();

        // Empty explicit root + a resolvable document => the document's root wins.
        let resolved =
            project_root_for_target_or_cwd(Some(Path::new("")), Some(document.as_path())).unwrap();
        assert!(
            !resolved.as_os_str().is_empty(),
            "an empty explicit root must never be returned verbatim"
        );
        assert_eq!(
            resolved.canonicalize().unwrap(),
            root.canonicalize().unwrap(),
            "resolution must fall through to the document's project root"
        );

        // A real explicit root is still honored exactly.
        let explicit =
            project_root_for_target_or_cwd(Some(root), Some(document.as_path())).unwrap();
        assert_eq!(explicit, root.to_path_buf());
    }

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

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git command failed to spawn");
        assert!(
            output.status.success(),
            "git {:?} failed: stderr={}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_fs::find_project_root(base).is_some() {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-project-root-io-")
                .tempdir_in(base)
                && agent_doc_fs::find_project_root(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn project_root_or_cwd_prefers_explicit_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("explicit");

        assert_eq!(project_root_or_cwd(Some(&root)).unwrap(), root);
    }

    #[test]
    fn project_root_for_target_or_cwd_uses_document_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("docs/session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(
            project_root_for_target_or_cwd(None, Some(&doc)).unwrap(),
            root
        );
    }

    #[test]
    fn project_root_for_target_or_cwd_falls_back_to_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        assert_eq!(project_root_for_target_or_cwd(None, None).unwrap(), root);
    }

    #[test]
    fn project_root_from_cwd_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("nested/deeper")).unwrap();
        let _cwd = ScopedCurrentDir::set(&root.join("nested/deeper"));

        assert_eq!(project_root_from_cwd().unwrap(), root);
    }

    #[test]
    fn project_root_containing_walks_up_from_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let nested = root.join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(project_root_containing(&nested).unwrap(), root);
    }

    #[test]
    fn project_root_containing_returns_none_without_agent_doc_root() {
        let dir = tempdir_without_agent_doc_ancestor();

        assert_eq!(project_root_containing(dir.path()), None);
    }

    #[test]
    fn project_root_or_file_parent_uses_agent_doc_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("nested/session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(
            project_root_or_file_parent(&doc).unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn project_root_or_file_parent_falls_back_to_file_parent() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("nested/session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(
            project_root_or_file_parent(&doc).unwrap(),
            doc.parent().unwrap().canonicalize().unwrap()
        );
    }

    #[test]
    fn project_root_from_arg_walks_up_from_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("nested")).unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        assert_eq!(
            project_root_from_arg(Some(Path::new("nested"))).unwrap(),
            root
        );
    }

    #[test]
    fn project_root_from_arg_accepts_explicit_agent_doc_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        assert_eq!(project_root_from_arg(Some(root)).unwrap(), root);
    }

    #[test]
    fn resolve_ipc_project_root_uses_nearest_agent_doc_for_submodule_file() {
        let parent_dir = tempfile::tempdir().unwrap();
        let sub_src_dir = tempfile::tempdir().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/patches")).unwrap();
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let project_root = resolve_ipc_project_root(&doc.canonicalize().unwrap());

        assert_eq!(
            project_root, submodule_root,
            "submodule docs must route IPC sidecars through the submodule root"
        );
        assert_ne!(
            project_root, parent,
            "superproject routing would split patch and lazily projection paths"
        );
    }

    #[test]
    fn resolve_ipc_project_root_ignores_agent_doc_outside_git_toplevel() {
        let outer_dir = tempfile::tempdir().unwrap();
        let outer = outer_dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(outer.join(".agent-doc/patches")).unwrap();

        let nested = outer.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init"]);
        let doc = nested.join("session.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        assert_eq!(
            resolve_ipc_project_root(&doc.canonicalize().unwrap()),
            nested,
            "an outer .agent-doc outside the current git toplevel must not capture IPC routing"
        );
    }
}
