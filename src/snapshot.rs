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
