use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

const SNAPSHOT_DIR: &str = ".agent-doc/snapshots";
const BASELINE_DIR: &str = ".agent-doc/baselines";
const LOCK_DIR: &str = ".agent-doc/locks";
const PENDING_DIR: &str = ".agent-doc/pending";
const TURN_SCOPE_DIR: &str = ".agent-doc/turn-scope";
const CRDT_DIR: &str = ".agent-doc/crdt";
const PRE_RESPONSE_DIR: &str = ".agent-doc/pre-response";
const CYCLE_STATE_DIR: &str = ".agent-doc/state/cycles";
const STARTING_DIR: &str = ".agent-doc/starting";
const BASELINE_OVERLAY_EXT: &str = "overlay.yrs";

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

/// Compute the SHA-256 hex hash used to key per-document state sidecars.
pub fn document_state_hash(doc: &Path) -> Result<String> {
    let canonical = canonical_document_path(doc)?;
    Ok(agent_doc_hash::path_string_hash(
        &canonical.to_string_lossy(),
    ))
}

/// Compute the per-document state hash from an already-resolved path string.
///
/// This avoids filesystem access for paths that no longer exist, such as the
/// old document path during rename recovery.
pub fn document_state_hash_from_str(absolute_path: &str) -> String {
    agent_doc_hash::path_string_hash(absolute_path)
}

/// Compute `<project_root>/.agent-doc/snapshots/<hash>.md` for a document.
///
/// If no `.agent-doc` project root exists, this preserves the historical
/// relative fallback `.agent-doc/snapshots/<hash>.md`.
pub fn snapshot_path_for(doc: &Path) -> Result<PathBuf> {
    let canonical = canonical_document_path(doc)?;
    let filename = format!(
        "{}.md",
        agent_doc_hash::path_string_hash(&canonical.to_string_lossy())
    );
    if let Some(root) = find_project_root(&canonical) {
        return Ok(root.join(SNAPSHOT_DIR).join(filename));
    }
    Ok(PathBuf::from(SNAPSHOT_DIR).join(filename))
}

/// Compute `<project_root>/.agent-doc/locks/<hash>.lock` for a document.
pub fn state_lock_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, LOCK_DIR, "lock")
}

/// Compute `<project_root>/.agent-doc/pending/<hash>.md` for a document.
pub fn pending_response_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, PENDING_DIR, "md")
}

/// Compute `<project_root>/.agent-doc/turn-scope/<hash>.json` for a document.
pub fn turn_scope_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, TURN_SCOPE_DIR, "json")
}

/// Compute `<project_root>/.agent-doc/state/cycles/<hash>.json` for a document.
///
/// Returns `Ok(None)` when `doc` cannot be canonicalized or no `.agent-doc`
/// project root exists.
pub fn cycle_state_path_for(doc: &Path) -> Result<Option<PathBuf>> {
    let canonical = match doc.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = document_state_hash(&canonical)?;
    Ok(Some(
        root.join(CYCLE_STATE_DIR).join(format!("{hash}.json")),
    ))
}

/// Compute `<project_root>/.agent-doc/starting` for a document.
///
/// Returns `None` when `doc` cannot be canonicalized or a root/fallback parent
/// cannot be resolved.
pub fn startup_starting_dir_for(doc: &Path) -> Option<PathBuf> {
    let canonical = doc.canonicalize().ok()?;
    let base =
        find_project_root(&canonical).or_else(|| canonical.parent().map(Path::to_path_buf))?;
    Some(base.join(STARTING_DIR))
}

/// Compute the startup lock filename for a tmux session.
pub fn startup_session_lock_name(session_name: &str) -> String {
    let hash = document_state_hash_from_str(&format!("session:{session_name}"));
    format!("session-{hash}.lock")
}

/// Compute `<project_root>/.agent-doc/starting/<hash>.lock` for a document.
///
/// Returns `None` when the startup directory cannot be resolved. Falls back to
/// hashing the input path text when the document hash cannot be derived from the
/// filesystem.
pub fn startup_document_lock_path_for(doc: &Path) -> Option<PathBuf> {
    let starting_dir = startup_starting_dir_for(doc)?;
    let hash = document_state_hash(doc)
        .unwrap_or_else(|_| document_state_hash_from_str(&doc.to_string_lossy()));
    Some(starting_dir.join(format!("{hash}.lock")))
}

/// Compute `<project_root>/.agent-doc/starting/session-<hash>.lock`.
pub fn startup_session_lock_path_for(doc: &Path, session_name: &str) -> Option<PathBuf> {
    Some(startup_starting_dir_for(doc)?.join(startup_session_lock_name(session_name)))
}

/// Compute `<project_root>/.agent-doc/baselines/<hash>.md` for a document.
pub fn baseline_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, BASELINE_DIR, "md")
}

/// Compute `<project_root>/.agent-doc/baselines/<hash>.overlay.yrs`.
pub fn baseline_overlay_path_for(doc: &Path) -> Result<PathBuf> {
    let (root, hash) = state_root_and_hash(doc)?;
    Ok(root
        .join(BASELINE_DIR)
        .join(format!("{}.{}", hash, BASELINE_OVERLAY_EXT)))
}

/// Compute `<project_root>/.agent-doc/pre-response/<hash>.md`.
pub fn pre_response_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, PRE_RESPONSE_DIR, "md")
}

/// Compute `<project_root>/.agent-doc/crdt/<hash>.yrs`.
pub fn crdt_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path(doc, CRDT_DIR, "yrs")
}

/// Compute `<project_root>/.agent-doc/crdt/<hash>.overlay.yrs`.
pub fn overlay_crdt_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path_with_suffix(doc, CRDT_DIR, "overlay.yrs")
}

/// Compute `<project_root>/.agent-doc/crdt/<hash>.nodes.yrs`.
pub fn multinode_crdt_path_for(doc: &Path) -> Result<PathBuf> {
    hashed_state_path_with_suffix(doc, CRDT_DIR, "nodes.yrs")
}

/// Compute the snapshot flock path adjacent to the snapshot sidecar.
pub fn snapshot_flock_path_for(doc: &Path) -> Result<PathBuf> {
    Ok(snapshot_path_for(doc)?.with_extension("md.lock"))
}

/// Compute the CRDT flock path adjacent to the legacy CRDT sidecar.
pub fn crdt_flock_path_for(doc: &Path) -> Result<PathBuf> {
    Ok(crdt_path_for(doc)?.with_extension("yrs.lock"))
}

/// Rewrite `file_path` to be relative to `cwd` so a spawned command resolves
/// correctly when its working directory is narrowed to a submodule root.
///
/// When pane cwd resolution narrows to a submodule, a caller's super-root
/// relative path does not resolve inside that cwd. On any filesystem miss or
/// non-descendant path, the original string is returned unchanged.
pub fn rewrite_start_path(file: &Path, cwd: &Path, original: &str) -> String {
    let Ok(abs_file) = std::fs::canonicalize(file) else {
        return original.to_string();
    };
    let Ok(abs_cwd) = std::fs::canonicalize(cwd) else {
        return original.to_string();
    };
    match abs_file.strip_prefix(&abs_cwd) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => original.to_string(),
    }
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

fn canonical_document_path(doc: &Path) -> Result<PathBuf> {
    doc.canonicalize()
        .with_context(|| format!("canonicalize document path for hash: {}", doc.display()))
}

fn state_root_and_hash(doc: &Path) -> Result<(PathBuf, String)> {
    let canonical = canonical_document_path(doc)?;
    let hash = agent_doc_hash::path_string_hash(&canonical.to_string_lossy());
    let root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok((root, hash))
}

fn hashed_state_path(doc: &Path, dir: &str, extension: &str) -> Result<PathBuf> {
    hashed_state_path_with_suffix(doc, dir, extension)
}

fn hashed_state_path_with_suffix(doc: &Path, dir: &str, suffix: &str) -> Result<PathBuf> {
    let (root, hash) = state_root_and_hash(doc)?;
    Ok(root.join(dir).join(format!("{}.{}", hash, suffix)))
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
    use super::{
        baseline_overlay_path_for, baseline_path_for, crdt_flock_path_for, crdt_path_for,
        cycle_state_path_for, document_state_hash, document_state_hash_from_str,
        multinode_crdt_path_for, overlay_crdt_path_for, pending_response_path_for,
        pre_response_path_for, read_optional, referenced_markdown_path,
        referenced_markdown_path_checked, rewrite_start_path, snapshot_flock_path_for,
        snapshot_path_for, startup_document_lock_path_for, startup_session_lock_name,
        startup_session_lock_path_for, startup_starting_dir_for, state_lock_path_for,
        turn_scope_path_for,
    };
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
    fn document_state_hash_uses_canonical_path_string() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let canonical = doc.canonicalize().unwrap();

        assert_eq!(
            document_state_hash(&doc).unwrap(),
            document_state_hash_from_str(&canonical.to_string_lossy())
        );
    }

    #[test]
    fn snapshot_path_uses_project_root_when_available() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("nested").join("doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();

        assert_eq!(
            snapshot_path_for(&doc).unwrap(),
            tmp.path()
                .join(".agent-doc")
                .join("snapshots")
                .join(format!("{hash}.md"))
        );
    }

    #[test]
    fn snapshot_path_preserves_relative_fallback_without_project_root() {
        let Some(tmp) = temp_dir_without_agent_doc_ancestor() else {
            return;
        };
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();

        assert_eq!(
            snapshot_path_for(&doc).unwrap(),
            Path::new(".agent-doc")
                .join("snapshots")
                .join(format!("{hash}.md"))
        );
    }

    fn temp_dir_without_agent_doc_ancestor() -> Option<tempfile::TempDir> {
        for base in [
            std::path::PathBuf::from("/var/tmp"),
            std::path::PathBuf::from("/dev/shm"),
            std::env::temp_dir(),
        ] {
            if !base.is_dir() || has_agent_doc_ancestor(&base) {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-fs-no-root")
                .tempdir_in(base)
            {
                return Some(dir);
            }
        }
        None
    }

    fn has_agent_doc_ancestor(path: &Path) -> bool {
        let Ok(mut current) = path.canonicalize() else {
            return false;
        };
        loop {
            if current.join(".agent-doc").is_dir() {
                return true;
            }
            if !current.pop() {
                return false;
            }
        }
    }

    #[test]
    fn document_state_sidecar_paths_share_hash_and_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();
        let agent_doc = tmp.path().join(".agent-doc");

        assert_eq!(
            state_lock_path_for(&doc).unwrap(),
            agent_doc.join("locks").join(format!("{hash}.lock"))
        );
        assert_eq!(
            pending_response_path_for(&doc).unwrap(),
            agent_doc.join("pending").join(format!("{hash}.md"))
        );
        assert_eq!(
            turn_scope_path_for(&doc).unwrap(),
            agent_doc.join("turn-scope").join(format!("{hash}.json"))
        );
        assert_eq!(
            baseline_path_for(&doc).unwrap(),
            agent_doc.join("baselines").join(format!("{hash}.md"))
        );
        assert_eq!(
            baseline_overlay_path_for(&doc).unwrap(),
            agent_doc
                .join("baselines")
                .join(format!("{hash}.overlay.yrs"))
        );
        assert_eq!(
            pre_response_path_for(&doc).unwrap(),
            agent_doc.join("pre-response").join(format!("{hash}.md"))
        );
        assert_eq!(
            crdt_path_for(&doc).unwrap(),
            agent_doc.join("crdt").join(format!("{hash}.yrs"))
        );
        assert_eq!(
            overlay_crdt_path_for(&doc).unwrap(),
            agent_doc.join("crdt").join(format!("{hash}.overlay.yrs"))
        );
        assert_eq!(
            multinode_crdt_path_for(&doc).unwrap(),
            agent_doc.join("crdt").join(format!("{hash}.nodes.yrs"))
        );
    }

    #[test]
    fn turn_scope_path_uses_project_root_and_document_hash() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("nested").join("doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();

        assert_eq!(
            turn_scope_path_for(&doc).unwrap(),
            tmp.path()
                .join(".agent-doc")
                .join("turn-scope")
                .join(format!("{hash}.json"))
        );
    }

    #[test]
    fn cycle_state_path_uses_project_root_and_document_hash() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("nested").join("doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();

        assert_eq!(
            cycle_state_path_for(&doc).unwrap(),
            Some(
                tmp.path()
                    .join(".agent-doc")
                    .join("state")
                    .join("cycles")
                    .join(format!("{hash}.json"))
            )
        );
    }

    #[test]
    fn cycle_state_path_returns_none_without_project_root() {
        let Some(tmp) = temp_dir_without_agent_doc_ancestor() else {
            return;
        };
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(cycle_state_path_for(&doc).unwrap(), None);
    }

    #[test]
    fn startup_lock_paths_use_project_starting_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "content").unwrap();

        let starting_dir = startup_starting_dir_for(&doc).unwrap();
        assert_eq!(starting_dir, tmp.path().join(".agent-doc/starting"));

        let doc_hash = document_state_hash(&doc).unwrap();
        assert_eq!(
            startup_document_lock_path_for(&doc).unwrap(),
            starting_dir.join(format!("{doc_hash}.lock"))
        );
        assert_eq!(
            startup_session_lock_path_for(&doc, "session-a").unwrap(),
            starting_dir.join(startup_session_lock_name("session-a"))
        );
    }

    #[test]
    fn cycle_state_path_returns_none_when_canonicalize_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let missing = tmp.path().join("missing.md");

        assert_eq!(cycle_state_path_for(&missing).unwrap(), None);
    }

    #[test]
    fn turn_scope_path_falls_back_to_document_parent_without_project_root() {
        let Some(tmp) = temp_dir_without_agent_doc_ancestor() else {
            return;
        };
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let hash = document_state_hash(&doc).unwrap();

        assert_eq!(
            turn_scope_path_for(&doc).unwrap(),
            tmp.path()
                .join(".agent-doc")
                .join("turn-scope")
                .join(format!("{hash}.json"))
        );
    }

    #[test]
    fn flock_paths_are_adjacent_to_snapshot_and_crdt_sidecars() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(
            snapshot_flock_path_for(&doc).unwrap(),
            snapshot_path_for(&doc).unwrap().with_extension("md.lock")
        );
        assert_eq!(
            crdt_flock_path_for(&doc).unwrap(),
            crdt_path_for(&doc).unwrap().with_extension("yrs.lock")
        );
    }

    #[test]
    fn rewrite_start_path_narrows_to_submodule_relative() {
        let tmp = tempfile::TempDir::new().unwrap();
        let super_root = tmp.path();
        let sub_root = super_root.join("src").join("sub");
        let tasks_dir = sub_root.join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let doc = tasks_dir.join("foo.md");
        std::fs::write(&doc, "# foo\n").unwrap();

        let rewritten = rewrite_start_path(&doc, &sub_root, "src/sub/tasks/foo.md");

        assert_eq!(
            rewritten,
            format!("tasks{}foo.md", std::path::MAIN_SEPARATOR)
        );
    }

    #[test]
    fn rewrite_start_path_noops_when_file_path_is_already_cwd_relative() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = root.join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        assert_eq!(rewrite_start_path(&doc, root, "plan.md"), "plan.md");
    }

    #[test]
    fn rewrite_start_path_falls_back_when_canonicalize_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ghost = tmp.path().join("does-not-exist.md");

        assert_eq!(
            rewrite_start_path(&ghost, tmp.path(), "does-not-exist.md"),
            "does-not-exist.md"
        );
    }

    #[test]
    fn rewrite_start_path_falls_back_when_file_not_under_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let outside = tmp.path().join("outside.md");
        std::fs::write(&outside, "# outside\n").unwrap();
        let unrelated_cwd = tempfile::TempDir::new().unwrap();

        assert_eq!(
            rewrite_start_path(&outside, unrelated_cwd.path(), "outside.md"),
            "outside.md"
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
