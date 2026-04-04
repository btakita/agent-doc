//! # Module: snapshot
//!
//! ## Spec
//! - `doc_hash(doc)`: compute SHA-256 hex of the document's canonical absolute path.
//!   Used as a stable, collision-resistant filename key for all per-doc state files.
//! - `find_project_root(path)`: walk up directory tree to find the directory containing
//!   `.agent-doc/`. Returns `None` if not found (e.g., in tests without project scaffolding).
//! - `path_for(doc)`: compute snapshot path `<project_root>/.agent-doc/snapshots/<hash>.md`.
//!   Falls back to a relative path when no project root is found.
//! - `lock_path_for(doc)`: compute advisory lock path `<project_root>/.agent-doc/locks/<hash>.lock`.
//! - `pending_path_for(doc)`: compute pending response path `<project_root>/.agent-doc/pending/<hash>.md`.
//! - `crdt_path_for(doc)`: compute CRDT state path `<project_root>/.agent-doc/crdt/<hash>.yrs`.
//! - `pre_response_path_for(doc)`: compute pre-response snapshot path
//!   `<project_root>/.agent-doc/pre-response/<hash>.md`.
//! - `SnapshotLock::acquire(doc)`: acquire an exclusive advisory flock on the snapshot lock
//!   file. Blocks until available. Released on drop.
//! - `load(doc)`: acquire snapshot lock, return snapshot content or `None` if absent.
//! - `save(doc, content)`: acquire snapshot lock, atomically write content via tempfile+rename.
//! - `delete(doc)`: acquire snapshot lock, remove snapshot file if present. No-op if absent.
//! - `resolve(doc)`: authoritative baseline for merge. Returns snapshot file content when it
//!   exists. Falls back to `git show HEAD:<doc>` only when no snapshot file exists and git
//!   content differs from current file (recovery path). Returns `None` on first submit.
//! - `save_pre_response(doc, content)`: atomically write content to the pre-response path
//!   (saved before the agent's response is applied, enabling undo).
//! - `load_pre_response(doc)`: return pre-response snapshot content or `None`.
//! - `delete_pre_response(doc)`: remove pre-response snapshot if present.
//! - `load_crdt(doc)`: acquire CRDT advisory lock, return CRDT state bytes or `None`.
//! - `save_crdt(doc, state)`: acquire CRDT advisory lock, atomically write state bytes.
//! - `delete_crdt(doc)`: acquire CRDT advisory lock, remove CRDT state file if present.
//!
//! ## Agentic Contracts
//! - All writes (snapshot, pre-response, CRDT) are atomic: written to a temp file in the
//!   same directory then renamed, ensuring no partial reads under concurrent access.
//! - `load`, `save`, `delete`, `load_crdt`, `save_crdt`, `delete_crdt` are all
//!   flock-protected — safe to call from concurrent processes on the same document.
//! - `resolve` prefers the snapshot file unconditionally when it exists. Git is only used
//!   as a recovery fallback. This prevents false baselines after a step-0b commit.
//! - `doc_hash` is deterministic and stable: same canonical path → same hash across runs.
//! - `path_for`, `lock_path_for`, `pending_path_for`, `crdt_path_for`, and
//!   `pre_response_path_for` all use the same `doc_hash`, so files for the same document
//!   always colocate under the same project root `.agent-doc/` tree.
//! - `delete` and `delete_crdt` are idempotent: calling them on an absent file is not an error.
//! - Pre-response snapshots are not flock-protected (single-writer assumption: only the
//!   active write path saves them).
//!
//! ## Evals
//! - `path_for_consistent_hash`: calling `path_for` twice on the same doc returns equal paths.
//! - `path_for_different_files_different_hashes`: two distinct files → distinct snapshot paths.
//! - `path_for_has_correct_structure`: path contains `.agent-doc/snapshots/`, ends with `.md`,
//!   stem is 64 lowercase hex chars.
//! - `load_returns_none_when_no_snapshot`: no snapshot file → `load` returns `None`.
//! - `snapshot_write_and_read_directly`: write then read snapshot file → content round-trips.
//! - `snapshot_overwrite`: writing twice → second value persists.
//! - `snapshot_delete_by_removing_file`: remove snapshot file → `read` returns `None`.
//! - `delete_no_error_when_missing`: `delete` on absent snapshot → no error.
//! - `flock_acquire_and_release_on_drop`: flock released after drop → second acquire succeeds.
//! - `flock_serializes_concurrent_access`: 10 threads increment a counter under flock →
//!   final value is exactly 10 (no lost updates).
//! - `atomic_write_via_tempfile_produces_correct_content`: tempfile+rename → correct content.
//! - `atomic_write_overwrites_existing`: atomic write over existing file → new content visible.
//! - `crdt_path_has_correct_extension`: CRDT path contains `.agent-doc/crdt/`, ends `.yrs`.
//! - `crdt_save_and_load_roundtrip`: save bytes then load → same bytes returned.
//! - `crdt_load_returns_none_when_missing`: no CRDT file → `load_crdt` returns `None`.
//! - `crdt_delete_removes_file`: save then delete → `load_crdt` returns `None`.
//! - `crdt_delete_no_error_when_missing`: `delete_crdt` on absent file → no error.
//! - `concurrent_atomic_writes_no_partial_content`: 20 threads atomically overwrite same
//!   file → final content is exactly one complete write (no corruption).
//! - `resolve_prefers_snapshot_over_git`: snapshot file present with different content than
//!   disk → `resolve` returns snapshot content, not disk/git content.

use anyhow::{Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const SNAP_DIR: &str = ".agent-doc/snapshots";
const LOCK_DIR: &str = ".agent-doc/locks";
const PENDING_DIR: &str = ".agent-doc/pending";
const CRDT_DIR: &str = ".agent-doc/crdt";

/// Compute the SHA256 hex hash of a document's canonical path.
/// Used for both snapshot filenames and lock filenames.
pub fn doc_hash(doc: &Path) -> Result<String> {
    let canonical = doc.canonicalize()?;
    Ok(hash_path_str(&canonical.to_string_lossy()))
}

/// Compute the SHA256 hex hash from an absolute path string.
///
/// Unlike [`doc_hash`], this does not call `canonicalize()` and therefore works
/// for paths that no longer exist on disk (e.g., the old path after a rename).
pub fn doc_hash_from_str(absolute_path: &str) -> String {
    hash_path_str(absolute_path)
}

fn hash_path_str(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute the advisory lock file path for a given document.
/// Walks up from the document to find the `.agent-doc/` project root.
/// Returns `<project_root>/.agent-doc/locks/<sha256_hash>.lock`.
/// Falls back to the document's parent directory if no project root found.
pub fn lock_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(LOCK_DIR).join(format!("{}.lock", hash)))
}

/// Compute the pending response file path for a given document.
/// Returns `<project_root>/.agent-doc/pending/<sha256_hash>.md`.
pub fn pending_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(PENDING_DIR).join(format!("{}.md", hash)))
}

/// Walk up from a path to find the directory containing `.agent-doc/`.
pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_file() {
        path.parent()?
    } else {
        path
    };
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

// ---------------------------------------------------------------------------
// Advisory file lock for snapshot operations
// ---------------------------------------------------------------------------

/// RAII guard for exclusive advisory lock on a snapshot file.
///
/// Acquire via `SnapshotLock::acquire(doc_path)`. The lock file is
/// `<snapshot_path>.lock` (sibling file). The lock is released when the
/// guard is dropped.
pub struct SnapshotLock {
    _file: File,
    lock_path: PathBuf,
}

impl SnapshotLock {
    /// Acquire an exclusive advisory lock for the snapshot of the given document.
    /// Blocks until the lock is available.
    pub fn acquire(doc: &Path) -> Result<Self> {
        let snap = path_for(doc)?;
        let lock_path = snap.with_extension("md.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open snapshot lock {}", lock_path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("failed to acquire snapshot lock on {}", lock_path.display()))?;
        Ok(Self { _file: file, lock_path })
    }
}

impl Drop for SnapshotLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
        // Delete the lock file on release to prevent stale lock accumulation.
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Compute the snapshot file path for a given document.
/// Returns an absolute path: `<project_root>/.agent-doc/snapshots/<hash>.md`.
/// Falls back to relative path if no project root found (e.g., tests without `.agent-doc/`).
pub fn path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let filename = format!("{}.md", hash);
    // Try to find project root for absolute path (consistent with lock_path_for/pending_path_for)
    if let Ok(canonical) = doc.canonicalize()
        && let Some(root) = find_project_root(&canonical)
    {
        return Ok(root.join(SNAP_DIR).join(filename));
    }
    // Fallback: relative path (legacy behavior for tests without .agent-doc/)
    Ok(PathBuf::from(SNAP_DIR).join(filename))
}

/// Load the snapshot content under an exclusive lock.
pub fn load(doc: &Path) -> Result<Option<String>> {
    let snap = path_for(doc)?;
    if !snap.exists() {
        return Ok(None);
    }
    let _lock = SnapshotLock::acquire(doc)?;
    load_unlocked(doc)
}

/// Save the current document content as the snapshot under an exclusive lock.
pub fn save(doc: &Path, content: &str) -> Result<()> {
    let _lock = SnapshotLock::acquire(doc)?;
    save_unlocked(doc, content)?;
    crate::ops_log::log_op(doc, &format!(
        "snapshot_save file={} len={}",
        doc.display(),
        content.len()
    ));
    Ok(())
}

/// Delete the snapshot for a document.
pub fn delete(doc: &Path) -> Result<()> {
    let snap = path_for(doc)?;
    if !snap.exists() {
        return Ok(());
    }
    let _lock = SnapshotLock::acquire(doc)?;
    if snap.exists() {
        std::fs::remove_file(&snap)?;
    }
    Ok(())
}

/// Resolve the best snapshot content for diff computation.
///
/// The snapshot file is always authoritative when it exists — it records the
/// exact baseline written by `agent-doc write` / `submit`, excluding concurrent
/// user edits. Git is only used as a recovery fallback when no snapshot file
/// exists (e.g., first submit after cloning, or snapshot was deleted).
pub fn resolve(doc: &Path) -> Result<Option<String>> {
    let snap_path = path_for(doc)?;
    if snap_path.exists() {
        // Snapshot file exists — always use it (authoritative baseline)
        return load(doc);
    }

    // No snapshot file — try git as recovery fallback.
    // Only useful when git HEAD differs from current file (real recovery).
    // If they match, the file was likely just committed (step 0b) before the
    // first diff — no useful baseline exists.
    let git_mtime = crate::git::last_commit_mtime(doc).unwrap_or(None);
    if git_mtime.is_some() {
        match crate::git::show_head(doc)? {
            Some(git_content) => {
                let current = std::fs::read_to_string(doc).unwrap_or_default();
                if git_content == current {
                    eprintln!("[snapshot] No snapshot file, git matches current — treating as first submit");
                    Ok(None)
                } else {
                    eprintln!("[snapshot] No snapshot file, recovering from git");
                    Ok(Some(git_content))
                }
            }
            None => Ok(None),
        }
    } else {
        // First submit — no previous state
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Auto-initialization for new documents
// ---------------------------------------------------------------------------

/// Perform **Initialization** for a document entering the agent-doc lifecycle.
///
/// **Ontology:** Initialization ensures a document has all prerequisites for
/// participation in the pane lifecycle: a session UUID (for Binding/Reconciliation),
/// a snapshot (for diff computation), and git tracking (for the gutter boundary).
///
/// Called from `claim`, `preflight`, and `sync`'s `resolve_file` — the three
/// entrypoints where a file first enters agent-doc's awareness.
///
/// Steps:
/// 1. **ensure_session_uuid** — if file has `agent_doc_format` but no `agent_doc_session`, generate and write a UUID
/// 2. **ensure_snapshot** — if no snapshot exists, create one with stripped exchange content
/// 3. **ensure_git_tracked** — `git add` if untracked, then `git commit` for baseline
///
/// Returns `true` if any initialization was performed, `false` if already initialized.
pub fn ensure_initialized(doc: &Path) -> Result<bool> {
    // First: ensure the file has a session UUID if it has agent_doc_format.
    // This must happen before the snapshot check because claim (which normally
    // assigns UUIDs) may not have been called for this file.
    if let Ok(content) = std::fs::read_to_string(doc)
        && let Ok((fm, _)) = crate::frontmatter::parse(&content)
        && fm.format.is_some() && fm.session.is_none()
    {
        eprintln!(
            "[init] assigning session UUID to {} (has format but no session)",
            doc.display()
        );
        if let Ok((updated, session_id)) = crate::frontmatter::ensure_session(&content)
            && updated != content
        {
            if let Err(e) = std::fs::write(doc, &updated) {
                eprintln!("[init] warning: failed to write session UUID: {}", e);
            } else {
                eprintln!("[init] assigned session UUID: {}", session_id);
            }
        }
    }

    let snap = path_for(doc)?;
    if snap.exists() {
        return Ok(false);
    }

    let canonical = std::fs::canonicalize(doc).unwrap_or_else(|_| doc.to_path_buf());
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    eprintln!(
        "[init] auto-initializing {} (no snapshot found)",
        doc.display()
    );

    // Save initial snapshot with stripped exchange content so existing user
    // text in the exchange becomes a diff on the next run.
    if let Ok(content) = std::fs::read_to_string(doc) {
        let snapshot_content = crate::claim::strip_exchange_content(&content);
        if let Err(e) = save(doc, &snapshot_content) {
            eprintln!("[init] warning: failed to save initial snapshot: {}", e);
        }
    }

    // Stage the file if untracked
    let is_tracked = std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch"])
        .arg(&canonical)
        .current_dir(&project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !is_tracked {
        eprintln!("[init] file is untracked — staging with git add");
        let _ = std::process::Command::new("git")
            .args(["add", "--"])
            .arg(&canonical)
            .current_dir(&project_root)
            .status();
    }

    // Commit to establish baseline
    if let Err(e) = crate::git::commit(doc) {
        eprintln!("[init] warning: failed to commit after init: {}", e);
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Internal unlocked helpers (caller must hold SnapshotLock)
// ---------------------------------------------------------------------------

fn load_unlocked(doc: &Path) -> Result<Option<String>> {
    let snap = path_for(doc)?;
    if snap.exists() {
        Ok(Some(std::fs::read_to_string(&snap)?))
    } else {
        Ok(None)
    }
}

fn save_unlocked(doc: &Path, content: &str) -> Result<()> {
    let snap = path_for(doc)?;
    if let Some(parent) = snap.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Atomic write: temp file + rename to avoid partial reads
    let parent = snap.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, content.as_bytes())
        .with_context(|| "failed to write snapshot temp file")?;
    tmp.persist(&snap)
        .with_context(|| format!("failed to rename temp file to {}", snap.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pre-response snapshot (for undo/extract)
// ---------------------------------------------------------------------------

const PRE_RESPONSE_DIR: &str = ".agent-doc/pre-response";

/// Compute the pre-response snapshot path for a given document.
/// Returns `<project_root>/.agent-doc/pre-response/<hash>.md`.
pub fn pre_response_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(PRE_RESPONSE_DIR).join(format!("{}.md", hash)))
}

/// Save the pre-response snapshot (the document content before the agent's response).
/// Called by write paths before applying patches/appending response.
pub fn save_pre_response(doc: &Path, content: &str) -> Result<()> {
    let path = pre_response_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, content.as_bytes())
        .with_context(|| "failed to write pre-response temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    eprintln!("[snapshot] saved pre-response snapshot for {}", doc.display());
    Ok(())
}

/// Load the pre-response snapshot for a document.
pub fn load_pre_response(doc: &Path) -> Result<Option<String>> {
    let path = pre_response_path_for(doc)?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(&path)?))
}

/// Delete the pre-response snapshot for a document.
pub fn delete_pre_response(doc: &Path) -> Result<()> {
    let path = pre_response_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRDT state persistence (for stream mode)
// ---------------------------------------------------------------------------

/// Compute the CRDT state file path for a given document.
/// Returns `<project_root>/.agent-doc/crdt/<hash>.yrs`.
/// Falls back to doc's parent directory if no project root found.
pub fn crdt_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let filename = format!("{}.yrs", hash);
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(CRDT_DIR).join(filename))
}

/// Load CRDT state bytes for a document (if any).
pub fn load_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    let path = crdt_path_for(doc)?;
    if !path.exists() {
        return Ok(None);
    }
    let _lock = acquire_crdt_lock(doc)?;
    let bytes = std::fs::read(&path)
        .with_context(|| format!("failed to read CRDT state {}", path.display()))?;
    Ok(Some(bytes))
}

/// Save CRDT state bytes for a document.
pub fn save_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    let path = crdt_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = acquire_crdt_lock(doc)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, state)
        .with_context(|| "failed to write CRDT state temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

/// Delete CRDT state for a document.
pub fn delete_crdt(doc: &Path) -> Result<()> {
    let path = crdt_path_for(doc)?;
    if path.exists() {
        let _lock = acquire_crdt_lock(doc)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Acquire an advisory lock for CRDT state operations.
/// Uses a lock file adjacent to the CRDT state file.
/// Stale lock files (>1 hour old) are cleaned before acquiring.
fn acquire_crdt_lock(doc: &Path) -> Result<File> {
    let crdt_path = crdt_path_for(doc)?;
    let lock_path = crdt_path.with_extension("yrs.lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Clean stale lock file (>1 hour old, from crashed processes)
    if let Ok(meta) = std::fs::metadata(&lock_path)
        && let Some(age) = meta.modified().ok().and_then(|t| t.elapsed().ok())
        && age > std::time::Duration::from_secs(3600)
    {
        let _ = std::fs::remove_file(&lock_path);
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open CRDT lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire CRDT lock on {}", lock_path.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "# Test\n").unwrap();
        (dir, doc)
    }

    /// Helper: write a snapshot file directly (without changing CWD).
    fn write_snapshot_directly(dir: &Path, doc: &Path, content: &str) {
        let snap = snapshot_path_in(dir, doc);
        fs::create_dir_all(snap.parent().unwrap()).unwrap();
        fs::write(&snap, content).unwrap();
    }

    /// Helper: read a snapshot file directly (without changing CWD).
    fn read_snapshot_directly(dir: &Path, doc: &Path) -> Option<String> {
        let snap = snapshot_path_in(dir, doc);
        if snap.exists() {
            Some(fs::read_to_string(&snap).unwrap())
        } else {
            None
        }
    }

    /// Compute snapshot path within a specific directory.
    /// If path_for returns absolute (project root found), use it directly.
    /// Otherwise, join relative path with dir.
    fn snapshot_path_in(dir: &Path, doc: &Path) -> PathBuf {
        let p = path_for(doc).unwrap();
        if p.is_absolute() {
            p
        } else {
            dir.join(&p)
        }
    }

    #[test]
    fn path_for_consistent_hash() {
        let (_dir, doc) = setup();
        let p1 = path_for(&doc).unwrap();
        let p2 = path_for(&doc).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn path_for_different_files_different_hashes() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        fs::write(&doc_a, "a").unwrap();
        fs::write(&doc_b, "b").unwrap();
        let pa = path_for(&doc_a).unwrap();
        let pb = path_for(&doc_b).unwrap();
        assert_ne!(pa, pb);
    }

    #[test]
    fn path_for_has_correct_structure() {
        let (_dir, doc) = setup();
        let p = path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/snapshots/"));
        assert!(p.to_string_lossy().ends_with(".md"));
        // Hash is 64 hex chars
        let filename = p.file_stem().unwrap().to_string_lossy();
        assert_eq!(filename.len(), 64);
        assert!(filename.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn load_returns_none_when_no_snapshot() {
        let (_dir, doc) = setup();
        let result = load(&doc).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn snapshot_write_and_read_directly() {
        let (dir, doc) = setup();
        let content = "# Snapshot content\n\nWith body.\n";
        write_snapshot_directly(dir.path(), &doc, content);
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some(content));
    }

    #[test]
    fn snapshot_overwrite() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "first");
        write_snapshot_directly(dir.path(), &doc, "second");
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some("second"));
    }

    #[test]
    fn snapshot_delete_by_removing_file() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "content");
        assert!(read_snapshot_directly(dir.path(), &doc).is_some());

        let snap = snapshot_path_in(dir.path(), &doc);
        fs::remove_file(&snap).unwrap();
        assert!(read_snapshot_directly(dir.path(), &doc).is_none());
    }

    #[test]
    fn delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete(&doc).unwrap();
    }

    // -----------------------------------------------------------------------
    // Race condition tests
    // -----------------------------------------------------------------------

    /// Test that flock-based locking works: acquire, hold, release on drop.
    /// Uses raw fs2 flock to avoid SnapshotLock's dependency on path_for/CWD.
    #[test]
    fn flock_acquire_and_release_on_drop() {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // First acquire succeeds
        {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&lock_path)
                .unwrap();
            file.lock_exclusive().unwrap();
            // Lock held here
            file.unlock().unwrap();
        }

        // After drop/unlock, second acquire succeeds
        let file2 = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        file2.lock_exclusive().unwrap();
        file2.unlock().unwrap();
    }

    /// Test that concurrent flock acquisitions serialize properly
    /// (no data loss when multiple threads write through locks).
    #[test]
    fn flock_serializes_concurrent_access() {
        use fs2::FileExt;
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");
        let data_path = dir.path().join("data.txt");
        fs::write(&data_path, "0").unwrap();

        let n = 10usize;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for _ in 0..n {
            let lp = lock_path.clone();
            let dp = data_path.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&lp)
                    .unwrap();
                file.lock_exclusive().unwrap();
                // Read-modify-write under lock
                let val: usize = fs::read_to_string(&dp).unwrap().trim().parse().unwrap();
                fs::write(&dp, (val + 1).to_string()).unwrap();
                file.unlock().unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_val: usize = fs::read_to_string(&data_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(final_val, n, "all {} increments should be serialized", n);
    }

    #[test]
    fn atomic_write_via_tempfile_produces_correct_content() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");

        // Atomic write: tempfile + persist
        let parent = dir.path();
        let mut tmp = tempfile::NamedTempFile::new_in(parent).unwrap();
        std::io::Write::write_all(&mut tmp, b"atomic content").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "atomic content");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");
        fs::write(&target, "old").unwrap();

        let mut tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        std::io::Write::write_all(&mut tmp, b"new").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn crdt_path_has_correct_extension() {
        let (_dir, doc) = setup();
        let p = crdt_path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/crdt/"));
        assert!(p.to_string_lossy().ends_with(".yrs"));
    }

    #[test]
    fn crdt_save_and_load_roundtrip() {
        let (_dir, doc) = setup();
        let state = vec![1u8, 2, 3, 4, 5];
        save_crdt(&doc, &state).unwrap();
        let loaded = load_crdt(&doc).unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn crdt_load_returns_none_when_missing() {
        let (_dir, doc) = setup();
        let loaded = load_crdt(&doc).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn crdt_delete_removes_file() {
        let (_dir, doc) = setup();
        save_crdt(&doc, &[1, 2, 3]).unwrap();
        assert!(load_crdt(&doc).unwrap().is_some());
        delete_crdt(&doc).unwrap();
        assert!(load_crdt(&doc).unwrap().is_none());
    }

    #[test]
    fn crdt_delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete_crdt(&doc).unwrap();
    }

    #[test]
    fn concurrent_atomic_writes_no_partial_content() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("concurrent.md");
        fs::write(&target, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let path = target.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&path).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content must be exactly one valid write (no corruption/partial)
        let final_content = fs::read_to_string(&target).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn resolve_prefers_snapshot_over_git() {
        // Verify that resolve() uses the snapshot file when it exists,
        // regardless of git commit mtime. This prevents the bug where
        // step 0b commit makes git newer than snapshot, causing resolve()
        // to return git content (= current file) instead of the snapshot.
        let (dir, doc) = setup();
        let snapshot_content = "snapshot baseline content";
        write_snapshot_directly(dir.path(), &doc, snapshot_content);

        // Even though the doc file on disk has different content,
        // resolve should return the snapshot file content.
        let resolved = resolve(&doc).unwrap();
        assert_eq!(resolved.as_deref(), Some(snapshot_content),
            "resolve() should always prefer snapshot file when it exists");
    }
}
