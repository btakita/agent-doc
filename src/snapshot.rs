use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SNAP_DIR: &str = ".agent-doc/snapshots";

/// Compute the snapshot file path for a given document.
pub fn path_for(doc: &Path) -> Result<PathBuf> {
    let canonical = doc.canonicalize()?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    Ok(PathBuf::from(SNAP_DIR).join(format!("{}.md", hash)))
}

/// Load the snapshot content, if it exists.
pub fn load(doc: &Path) -> Result<Option<String>> {
    let snap = path_for(doc)?;
    if snap.exists() {
        Ok(Some(std::fs::read_to_string(&snap)?))
    } else {
        Ok(None)
    }
}

/// Save the current document content as the snapshot.
pub fn save(doc: &Path, content: &str) -> Result<()> {
    let snap = path_for(doc)?;
    if let Some(parent) = snap.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&snap, content)?;
    Ok(())
}

/// Delete the snapshot for a document.
pub fn delete(doc: &Path) -> Result<()> {
    let snap = path_for(doc)?;
    if snap.exists() {
        std::fs::remove_file(&snap)?;
    }
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
}
