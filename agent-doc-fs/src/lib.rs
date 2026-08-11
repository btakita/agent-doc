use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

pub mod install_freshness;

const SNAPSHOT_DIR: &str = ".agent-doc/snapshots";
const LOCK_DIR: &str = ".agent-doc/locks";
const RECOVERY_DIR: &str = ".agent-doc/recovery";
const STARTING_DIR: &str = ".agent-doc/starting";

/// Walk up the directory tree from `path` to find the directory containing
/// `.agent-doc` (the project root). Relative inputs are first anchored to the
/// process working directory so the returned root is never an empty relative
/// path. Returns `None` if no such ancestor exists.
pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let anchored = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut current = if anchored.is_file() {
        anchored.parent()?
    } else {
        anchored.as_path()
    };
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

/// The inode a process currently maps via `/proc/<pid>/exe`.
///
/// On Linux this magic symlink resolves to the real on-disk inode of the running
/// executable even after the install path has been replaced. Returns `None`
/// when `/proc` is unavailable, the process is gone, or the stat fails.
pub fn running_exe_inode_for_pid(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;

        std::fs::metadata(format!("/proc/{pid}/exe"))
            .ok()
            .map(|meta| meta.ino())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// Inode of the on-disk file at `path`. Returns `None` on non-Unix platforms or
/// any stat error.
pub fn inode_of_path(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        std::fs::metadata(path).ok().map(|meta| meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
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

/// Process-local sequence so concurrent [`write_atomic`] calls never collide on
/// the same sibling temp-file name.
static ATOMIC_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Atomically write `contents` to `path` by writing a sibling temp file and
/// renaming it into place. `rename(2)` on the same filesystem is atomic, so a
/// crash or `execve` mid-write leaves either the previous file or the
/// fully-written new one — never a truncated/0-byte file. Creates the parent
/// directory if missing.
///
/// This is the write counterpart to [`read_optional_text`] and the fix for an
/// interrupted write (e.g. an `auto_install_reexec` recycle killed mid-write)
/// leaving a 0-byte controller-state file that then wedges every future read.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir for {}", path.display()))?;
    }
    let dir = parent.unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent-doc-state");
    let seq = ATOMIC_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{stem}.tmp-{}-{seq}", std::process::id()));
    std::fs::write(&tmp, contents)
        .with_context(|| format!("failed to write temp file {}", tmp.display()))?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        // Best-effort cleanup so a failed rename does not litter temp files;
        // surface (never swallow) the original rename error.
        if let Err(cleanup) = std::fs::remove_file(&tmp) {
            eprintln!(
                "[agent-doc] warning: failed to clean up temp file {} after rename error: {cleanup}",
                tmp.display()
            );
        }
        return Err(err)
            .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()));
    }
    Ok(())
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

/// Read `path` and parse it with `parse`. Missing → `Ok(None)`. If the file
/// exists but is empty / whitespace-only / fails to parse (a *corrupt* state
/// file — e.g. a 0-byte `controller-state.json` left by a pre-[`write_atomic`]
/// interrupted write or an external truncation), quarantine the bad file by
/// renaming it aside and return `Ok(None)` so the caller reboots from a clean
/// slate instead of wedging on every future read.
///
/// This is the read counterpart to [`write_atomic`] (`#corrupt-state-quarantine`):
/// `write_atomic` stops *new* 0-byte files; this recovers automatically from an
/// *already-corrupt* one, so a partial write no longer manifests as the manual
/// `start "timed out waiting for project controller"` move-aside dance.
pub fn read_valid_or_quarantine<T, F>(path: &Path, parse: F) -> Result<Option<T>>
where
    F: FnOnce(&str) -> Option<T>,
{
    let Some(text) = read_optional_text(path)? else {
        return Ok(None);
    };
    if !text.trim().is_empty()
        && let Some(parsed) = parse(&text)
    {
        return Ok(Some(parsed));
    }
    quarantine_corrupt_file(path)?;
    Ok(None)
}

/// Rename a corrupt state file aside to a sibling `<name>.corrupt-<pid>-<seq>`
/// so it stops wedging reads while remaining available for forensics. A file
/// that raced away (already removed) is treated as success.
pub fn quarantine_corrupt_file(path: &Path) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent-doc-state");
    let seq = ATOMIC_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let quarantine =
        path.with_file_name(format!("{file_name}.corrupt-{}-{seq}", std::process::id()));
    match std::fs::rename(path, &quarantine) {
        Ok(()) => {
            eprintln!(
                "[agent-doc] quarantined corrupt state file {} -> {}",
                path.display(),
                quarantine.display()
            );
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to quarantine corrupt state file {} -> {}",
                path.display(),
                quarantine.display()
            )
        }),
    }
}

/// Preserve a buffer the merge is about to drop to a durable recovery sidecar at
/// `<project_root>/.agent-doc/recovery/<hash>.<pid>-<seq>.md`, so concurrent
/// operator text is recoverable instead of silently lost (`#qftlossdelta`).
/// Written atomically; returns the sidecar path. Best-effort by the caller —
/// this is a safety net alongside, not a replacement for, the merge decision.
pub fn preserve_dropped_operator_buffer(doc: &Path, content: &str) -> Result<PathBuf> {
    let (root, hash) = state_root_and_hash(doc)?;
    let seq = ATOMIC_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = root
        .join(RECOVERY_DIR)
        .join(format!("{hash}.{}-{seq}.md", std::process::id()));
    write_atomic(&path, content.as_bytes())?;
    Ok(path)
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

pub fn same_document_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
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
        document_state_hash, document_state_hash_from_str, find_project_root, inode_of_path,
        preserve_dropped_operator_buffer, quarantine_corrupt_file, read_optional,
        read_valid_or_quarantine, referenced_markdown_path, referenced_markdown_path_checked,
        rewrite_start_path, running_exe_inode_for_pid, same_document_path, snapshot_path_for,
        startup_document_lock_path_for, startup_session_lock_name, startup_session_lock_path_for,
        startup_starting_dir_for, state_lock_path_for, write_atomic,
    };
    use std::path::Path;

    #[test]
    fn write_atomic_creates_parent_and_writes_contents() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested/dir/state.json");
        write_atomic(&path, b"{\"k\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"k\":1}");
    }

    // #corrupt-state-quarantine

    #[test]
    fn read_valid_or_quarantine_missing_is_none_no_side_effect() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("controller-state.json");
        let got: Option<String> = read_valid_or_quarantine(&path, |s| Some(s.to_string())).unwrap();
        assert!(got.is_none());
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    }

    #[test]
    fn read_valid_or_quarantine_valid_returns_parsed_and_keeps_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("controller-state.json");
        std::fs::write(&path, "42").unwrap();
        let got: Option<u32> = read_valid_or_quarantine(&path, |s| s.trim().parse().ok()).unwrap();
        assert_eq!(got, Some(42));
        assert!(path.exists(), "valid file must be left intact");
    }

    #[test]
    fn read_valid_or_quarantine_zero_byte_quarantines_and_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("controller-state.json");
        std::fs::write(&path, "").unwrap(); // 0-byte, the classic wedge
        let got: Option<u32> = read_valid_or_quarantine(&path, |s| s.trim().parse().ok()).unwrap();
        assert!(got.is_none(), "empty state must not parse");
        assert!(!path.exists(), "0-byte file must be moved aside");
        let quarantined: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(quarantined.len(), 1, "exactly one quarantine sibling");
    }

    #[test]
    fn read_valid_or_quarantine_unparseable_quarantines() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("controller-state.json");
        std::fs::write(&path, "{not json").unwrap();
        let got: Option<u32> = read_valid_or_quarantine(&path, |s| s.trim().parse().ok()).unwrap();
        assert!(got.is_none());
        assert!(!path.exists(), "corrupt file must be moved aside");
    }

    #[test]
    fn quarantine_corrupt_file_missing_is_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        // A file that raced away is treated as success (idempotent recovery).
        quarantine_corrupt_file(&tmp.path().join("gone.json")).unwrap();
    }

    #[test]
    fn preserve_dropped_operator_buffer_writes_recovery_sidecar() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        let path =
            preserve_dropped_operator_buffer(&doc, "operator text that would be lost").unwrap();
        assert!(path.exists(), "recovery sidecar must be written");
        assert!(
            path.to_string_lossy().contains("/.agent-doc/recovery/"),
            "sidecar under .agent-doc/recovery: {}",
            path.display()
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "operator text that would be lost"
        );
    }

    #[test]
    fn write_atomic_overwrites_existing_and_leaves_no_temp_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("state.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new-longer-content").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "new-longer-content"
        );
        // No sibling ".state.json.tmp-*" temp files should survive a successful write.
        let leftover = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp-"));
        assert!(!leftover, "temp file leaked after atomic write");
    }

    #[cfg(unix)]
    #[test]
    fn inode_of_path_reads_existing_file_inode() {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("binary");
        std::fs::write(&path, b"agent-doc").unwrap();

        assert_eq!(
            inode_of_path(&path),
            Some(std::fs::metadata(&path).unwrap().ino())
        );
        assert_eq!(inode_of_path(&tmp.path().join("missing")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn running_exe_inode_for_pid_reads_current_process_inode() {
        use std::os::unix::fs::MetadataExt;

        let expected = std::fs::metadata(format!("/proc/{}/exe", std::process::id()))
            .unwrap()
            .ino();

        assert_eq!(
            running_exe_inode_for_pid(std::process::id()),
            Some(expected)
        );
    }

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
    fn same_document_path_matches_equal_unresolved_paths() {
        assert!(same_document_path(
            Path::new("/tmp/agent-doc-same.md"),
            Path::new("/tmp/agent-doc-same.md")
        ));
        assert!(!same_document_path(
            Path::new("/tmp/agent-doc-left.md"),
            Path::new("/tmp/agent-doc-right.md")
        ));
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
    fn project_root_from_relative_file_is_absolute_and_non_empty() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("agent-doc-fs-relative-root")
            .tempdir_in(&cwd)
            .unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("nested").join("session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# session\n").unwrap();
        let relative_doc = doc.strip_prefix(&cwd).unwrap();

        let root = find_project_root(relative_doc).unwrap();

        assert!(root.is_absolute(), "root should be absolute: {root:?}");
        assert!(!root.as_os_str().is_empty());
        assert_eq!(root, tmp.path().canonicalize().unwrap());
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
    fn document_state_paths_share_hash_and_project_root() {
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

use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Agent instruction surfaces (`#hookhashanchortags`)
//
// Lives here, once, because two consumers need the same answer: the
// `PreToolUse` coined-id guard and the post-commit `session-check` guard. Two
// copies of "which files define anchors" is exactly the drift this codebase
// keeps paying for -- the wording gets consolidated while the predicate quietly
// diverges -- so the file set has one home and both callers read it.
// ---------------------------------------------------------------------------

/// Glob-free list of the project's agent instruction surfaces.
///
/// Root instruction files, the runbook and spec directories, and every
/// installed harness skill directory. These are where agent-doc-style anchors
/// are DEFINED, so an id found here names a documented rule rather than
/// invented work.
pub fn instruction_surface_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = ["AGENTS.md", "CLAUDE.md", "SKILL.md", "SPEC.md"]
        .iter()
        .map(|name| root.join(name))
        .collect();

    fn markdown_in(files: &mut Vec<PathBuf>, dir: PathBuf) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }

    markdown_in(&mut files, root.join("runbooks"));
    markdown_in(&mut files, root.join("specs"));
    for skills_dir in [
        root.join(".claude/skills"),
        root.join(".codex/skills"),
        root.join(".opencode/skills"),
    ] {
        let Ok(entries) = std::fs::read_dir(&skills_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            files.push(entry.path().join("SKILL.md"));
            markdown_in(&mut files, entry.path().join("runbooks"));
        }
    }
    markdown_in(&mut files, root.join(".cursor/rules"));
    files.extend(submodule_instruction_files(root));
    files
}

/// Root instruction files of the git submodules this project declares.
///
/// A superproject's own surfaces do not define the anchors its submodules
/// document. In this repo `#ci-no-closeout-wait` and `#deploy-just-do-it` are
/// agent-doc *development* rules that live only in `src/agent-doc/AGENTS.md` and
/// never ship in the installed SKILL, so a response citing one was reported as
/// coining an id even after `#hookhashanchortags` landed.
///
/// Deliberately bounded on two axes, because this feeds a `PreToolUse` hook and
/// this superproject declares 70 submodules:
///
/// - only paths declared in `.gitmodules`, never a directory walk;
/// - only each submodule's ROOT instruction files, never its runbooks or specs,
///   which is where the bulk of the bytes live.
///
/// A path that is absolute or escapes the root is skipped: `.gitmodules` is
/// tracked content, and a surface set is not the place to follow it outward.
fn submodule_instruction_files(root: &Path) -> Vec<PathBuf> {
    let Ok(config) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for line in config.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "path" {
            continue;
        }
        let relative = Path::new(value.trim());
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            continue;
        }
        let submodule = root.join(relative);
        files.extend(
            ["AGENTS.md", "CLAUDE.md", "SKILL.md"]
                .iter()
                .map(|name| submodule.join(name)),
        );
    }
    files
}

/// Anchors defined by the project's agent instruction surfaces.
///
/// Best-effort by construction: a file that will not read contributes nothing.
/// That is the right failure direction here — this set only ever *widens* what
/// is allowed, so an unreadable instruction file costs a false block at worst,
/// never a missed one. It is the mirror image of `known_ids_for_document`,
/// which fails closed because it is the primary ledger.
pub fn instruction_surface_anchors(root: &Path) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    for file in instruction_surface_files(root) {
        if let Ok(content) = std::fs::read_to_string(&file) {
            anchors.extend(agent_doc_turn::coined_ids::extract_tags(&content));
        }
    }
    anchors
}
#[cfg(test)]
mod instruction_surface_tests {
    use super::*;

    #[test]
    fn anchors_come_from_every_instruction_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "rule (`#fromagents`)\n").unwrap();
        std::fs::write(root.join("CLAUDE.md"), "rule (`#fromclaude`)\n").unwrap();
        std::fs::create_dir_all(root.join("runbooks")).unwrap();
        std::fs::write(root.join("runbooks/commit.md"), "`#fromrunbook`\n").unwrap();
        std::fs::create_dir_all(root.join("specs")).unwrap();
        std::fs::write(root.join("specs/07-commands.md"), "`#fromspec`\n").unwrap();
        std::fs::create_dir_all(root.join(".claude/skills/agent-doc/runbooks")).unwrap();
        std::fs::write(
            root.join(".claude/skills/agent-doc/SKILL.md"),
            "`#fromskill`\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".claude/skills/agent-doc/runbooks/respond.md"),
            "`#fromskillrunbook`\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".cursor/rules")).unwrap();
        std::fs::write(root.join(".cursor/rules/agent-doc.md"), "`#fromcursor`\n").unwrap();

        let anchors = instruction_surface_anchors(root);
        for expected in [
            "fromagents",
            "fromclaude",
            "fromrunbook",
            "fromspec",
            "fromskill",
            "fromskillrunbook",
            "fromcursor",
        ] {
            assert!(
                anchors.contains(expected),
                "missing {expected}: {anchors:?}"
            );
        }
    }

    /// Best-effort by construction: this set only ever WIDENS what a guard
    /// allows, so an unreadable or absent surface must degrade to "no anchors"
    /// rather than erroring. A directory where a file belongs is the nastiest
    /// version of that.
    #[test]
    fn an_unreadable_surface_contributes_nothing_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("AGENTS.md")).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "`#stillfound`\n").unwrap();

        let anchors = instruction_surface_anchors(dir.path());
        assert!(anchors.contains("stillfound"), "{anchors:?}");
    }

    /// `#anchorsubmodulesurfaces`: a superproject does not define the anchors its
    /// submodules document. Confirmed live — `#ci-no-closeout-wait` is an
    /// agent-doc development rule that exists only in `src/agent-doc/AGENTS.md`
    /// and nowhere under the agent-loop root.
    #[test]
    fn anchors_include_declared_submodule_instruction_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "`#fromsuperproject`\n").unwrap();
        std::fs::write(
            root.join(".gitmodules"),
            "[submodule \"src/agent-doc\"]\n\tpath = src/agent-doc\n\turl = git@example.test:a.git\n\
             [submodule \"src/other\"]\n\tpath = src/other\n\turl = git@example.test:b.git\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src/agent-doc")).unwrap();
        std::fs::write(
            root.join("src/agent-doc/AGENTS.md"),
            "External CI is observed (`#cinocloseoutwait`)\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src/other")).unwrap();
        std::fs::write(root.join("src/other/SKILL.md"), "`#fromothersub`\n").unwrap();

        let anchors = instruction_surface_anchors(root);
        assert!(anchors.contains("fromsuperproject"), "{anchors:?}");
        assert!(anchors.contains("cinocloseoutwait"), "{anchors:?}");
        assert!(anchors.contains("fromothersub"), "{anchors:?}");
    }

    /// Only the submodule's ROOT instruction files are read. Its runbooks and
    /// specs are where the bytes are, and this feeds a hook that runs on every
    /// Edit.
    #[test]
    fn submodule_runbooks_and_specs_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitmodules"), "\tpath = sub\n").unwrap();
        std::fs::create_dir_all(root.join("sub/runbooks")).unwrap();
        std::fs::create_dir_all(root.join("sub/specs")).unwrap();
        std::fs::write(root.join("sub/AGENTS.md"), "`#subroot`\n").unwrap();
        std::fs::write(root.join("sub/runbooks/commit.md"), "`#subrunbook`\n").unwrap();
        std::fs::write(root.join("sub/specs/07.md"), "`#subspec`\n").unwrap();

        let anchors = instruction_surface_anchors(root);
        assert!(anchors.contains("subroot"), "{anchors:?}");
        assert!(!anchors.contains("subrunbook"), "{anchors:?}");
        assert!(!anchors.contains("subspec"), "{anchors:?}");
    }

    /// `.gitmodules` is tracked content; a surface set must not follow it out of
    /// the project.
    #[test]
    fn a_submodule_path_escaping_the_root_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "`#outsidetheroot`\n").unwrap();
        std::fs::write(
            root.join(".gitmodules"),
            "\tpath = ../\n\tpath = /etc\n\tpath = sub\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/AGENTS.md"), "`#insidetheroot`\n").unwrap();

        let anchors = instruction_surface_anchors(&root);
        assert!(anchors.contains("insidetheroot"), "{anchors:?}");
        assert!(!anchors.contains("outsidetheroot"), "{anchors:?}");
    }

    /// A project with no `.gitmodules` behaves exactly as before.
    #[test]
    fn a_project_without_submodules_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "`#onlysurface`\n").unwrap();
        let anchors = instruction_surface_anchors(dir.path());
        assert_eq!(anchors.len(), 1, "{anchors:?}");
        assert!(anchors.contains("onlysurface"));
    }

    #[test]
    fn an_empty_project_yields_no_anchors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(instruction_surface_anchors(dir.path()).is_empty());
    }

    /// Only Markdown is read from the surface directories, so a stray script or
    /// binary in `runbooks/` cannot inject an anchor.
    #[test]
    fn only_markdown_is_read_from_surface_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("runbooks")).unwrap();
        std::fs::write(dir.path().join("runbooks/notes.txt"), "`#notmarkdown`\n").unwrap();
        std::fs::write(dir.path().join("runbooks/real.md"), "`#ismarkdown`\n").unwrap();

        let anchors = instruction_surface_anchors(dir.path());
        assert!(anchors.contains("ismarkdown"), "{anchors:?}");
        assert!(!anchors.contains("notmarkdown"), "{anchors:?}");
    }
}
