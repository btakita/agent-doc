//! SHA-256 hex hashing helpers shared across agent-doc crates.

use sha2::{Digest, Sha256};

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

#[cfg(test)]
mod tests {
    use super::{bytes_hash, content_hash};

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
}
