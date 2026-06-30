//! SHA-256 hex hashing helpers shared across agent-doc crates.

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Compute the SHA-256 hex digest of UTF-8 text content.
pub fn content_hash(content: &str) -> String {
    bytes_hash(content.as_bytes())
}

/// Compute the SHA-256 hex digest of arbitrary bytes.
pub fn bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Compute the SHA-256 hex digest of a canonical document path.
pub fn path_hash(path: &Path) -> io::Result<String> {
    let canonical = path.canonicalize()?;
    Ok(path_string_hash(&canonical.to_string_lossy()))
}

/// Compute the SHA-256 hex digest of an already-resolved path string.
pub fn path_string_hash(path: &str) -> String {
    content_hash(path)
}

/// Compute the stable document id used by tracked-work and queue sidecars.
///
/// Existing documents are identified by the SHA-256 hex digest of their
/// canonical path. Missing paths keep the historical fallback to the display
/// string so callers that are already in best-effort cleanup paths do not panic.
pub fn document_id_for_path(path: &Path) -> String {
    path_hash(path).unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::{bytes_hash, content_hash, document_id_for_path, path_hash, path_string_hash};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn content_hash_matches_sha256_hex() {
        assert_eq!(
            content_hash("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            content_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn bytes_hash_matches_text_hash_for_utf8() {
        assert_eq!(bytes_hash(b"hello"), content_hash("hello"));
    }

    #[test]
    fn path_hash_uses_canonical_path_string() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let canonical = file.canonicalize().unwrap();

        assert_eq!(
            path_hash(&file).unwrap(),
            path_string_hash(&canonical.to_string_lossy())
        );
        assert_eq!(document_id_for_path(&file), path_hash(&file).unwrap());
    }

    #[test]
    fn document_id_for_path_preserves_missing_path_fallback() {
        let missing = Path::new("definitely/missing.md");
        assert_eq!(document_id_for_path(missing), missing.display().to_string());
    }
}
