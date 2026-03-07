use anyhow::{Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const SNAP_DIR: &str = ".agent-doc/snapshots";

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
        Ok(Self { _file: file })
    }
}

impl Drop for SnapshotLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

/// Compute the snapshot file path for a given document.
pub fn path_for(doc: &Path) -> Result<PathBuf> {
    let canonical = doc.canonicalize()?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    Ok(PathBuf::from(SNAP_DIR).join(format!("{}.md", hash)))
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
    save_unlocked(doc, content)
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
        let snap_rel = path_for(doc).unwrap();
        let snap_abs = dir.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
    }

    /// Helper: read a snapshot file directly (without changing CWD).
    fn read_snapshot_directly(dir: &Path, doc: &Path) -> Option<String> {
        let snap_rel = path_for(doc).unwrap();
        let snap_abs = dir.join(&snap_rel);
        if snap_abs.exists() {
            Some(fs::read_to_string(&snap_abs).unwrap())
        } else {
            None
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
        assert!(p.to_string_lossy().starts_with(".agent-doc/snapshots/"));
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

        let snap_rel = path_for(&doc).unwrap();
        let snap_abs = dir.path().join(&snap_rel);
        fs::remove_file(&snap_abs).unwrap();
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
}
