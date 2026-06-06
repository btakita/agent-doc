//! Boundary ID helpers — used across all write paths.
//!
//! - [`new_boundary_id`] — 8-hex-char UUID v4 prefix (length controlled by
//!   [`BOUNDARY_ID_LEN`]).
//! - [`new_boundary_id_with_summary`] — same ID optionally suffixed with a
//!   3-word, 20-char-max slug derived from `summary`
//!   (format: `a0cfeb34:boundary-fix`).
//! - [`format_boundary_marker`] — renders the canonical HTML comment form
//!   `<!-- agent:boundary:<id> -->` used in document component boundaries.
//!
//! Non-deterministic: `new_boundary_id` uses UUID v4 — callers must not rely
//! on ordering.

/// Default number of hex characters for boundary IDs.
pub const BOUNDARY_ID_LEN: usize = 8;

/// Generate a new boundary ID (short hex string from UUID v4).
pub fn new_boundary_id() -> String {
    let full = uuid::Uuid::new_v4().to_string().replace('-', "");
    full[..BOUNDARY_ID_LEN.min(full.len())].to_string()
}

/// Generate a boundary ID with an optional summary suffix.
///
/// Format: `a0cfeb34` or `a0cfeb34:boundary-fix` (with summary).
/// The summary is slugified (lowercase, non-alphanumeric → `-`, max 20 chars).
pub fn new_boundary_id_with_summary(summary: Option<&str>) -> String {
    with_summary_slug(new_boundary_id(), summary)
}

/// Derive a *deterministic* boundary ID from a stable seed (for example the IPC
/// `patch_id`), optionally suffixed with a summary slug. Same seed → same ID.
///
/// This is the key to `#finalize-visible-buffer-ipc-timeout-race` corruption:
/// a single logical write can build its IPC patches more than once (socket
/// attempt, then file-IPC fallback, then the `run_stream` timeout re-write).
/// When each build minted a fresh random boundary, the plugin received the same
/// response under different boundary IDs and, unable to match an already-applied
/// boundary, appended it a second time — doubling the editor buffer. Seeding the
/// boundary from the stable `patch_id` makes every rebuild carry an identical
/// boundary the plugin can dedup/replace instead of appending.
pub fn boundary_id_from_seed_with_summary(seed: &str, summary: Option<&str>) -> String {
    let hex: String = seed
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let id = if hex.len() >= BOUNDARY_ID_LEN {
        hex[..BOUNDARY_ID_LEN].to_string()
    } else {
        // Seed without enough hex digits (shouldn't happen for a UUID patch_id):
        // fold to a stable 8-hex value so the result is still deterministic.
        let folded = seed
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        format!("{folded:08x}")
    };
    with_summary_slug(id, summary)
}

/// Append a slugified summary suffix to a boundary ID, if `summary` is non-empty.
fn with_summary_slug(id: String, summary: Option<&str>) -> String {
    match summary {
        Some(s) if !s.is_empty() => {
            let slug: String = s
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .take(3) // max 3 words
                .collect::<Vec<&str>>()
                .join("-");
            let slug = &slug[..slug.len().min(20)];
            format!("{}:{}", id, slug)
        }
        _ => id,
    }
}

/// Format a boundary marker comment.
pub fn format_boundary_marker(id: &str) -> String {
    format!("<!-- agent:boundary:{} -->", id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_boundary_id_len() {
        assert_eq!(new_boundary_id().len(), BOUNDARY_ID_LEN);
        assert!(new_boundary_id().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn new_boundary_id_with_summary_none() {
        assert_eq!(new_boundary_id_with_summary(None).len(), BOUNDARY_ID_LEN);
    }

    #[test]
    fn new_boundary_id_with_summary_empty() {
        assert_eq!(
            new_boundary_id_with_summary(Some("")).len(),
            BOUNDARY_ID_LEN
        );
    }

    #[test]
    fn new_boundary_id_with_summary_slug() {
        let id = new_boundary_id_with_summary(Some("Boundary Fix"));
        assert!(id.ends_with(":boundary-fix"), "got: {}", id);
    }

    #[test]
    fn new_boundary_id_with_summary_truncate() {
        let long = "this is a very long summary that should be truncated and clipped";
        let id = new_boundary_id_with_summary(Some(long));
        let (_, slug) = id.split_once(':').unwrap();
        assert!(slug.len() <= 20, "slug too long: {}", slug);
    }

    #[test]
    fn format_boundary_marker_renders_correctly() {
        assert_eq!(
            format_boundary_marker("abc123"),
            "<!-- agent:boundary:abc123 -->"
        );
    }

    #[test]
    fn boundary_id_from_seed_is_deterministic() {
        // #finalize-visible-buffer-ipc-timeout-race: the same patch_id seed must
        // always derive the same boundary so socket/file/fallback rebuilds carry
        // an identical boundary the plugin dedups instead of appending.
        let patch_id = "2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a";
        let a = boundary_id_from_seed_with_summary(patch_id, None);
        let b = boundary_id_from_seed_with_summary(patch_id, None);
        assert_eq!(a, b, "same seed must yield the same boundary id");
        assert_eq!(a.len(), BOUNDARY_ID_LEN);
        assert_eq!(a, "2ffa57c0", "boundary derives from the seed's hex prefix");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn boundary_id_from_seed_differs_across_seeds() {
        let a = boundary_id_from_seed_with_summary("2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a", None);
        let b = boundary_id_from_seed_with_summary("99887766-1111-2222-3333-444455556666", None);
        assert_ne!(a, b, "different writes must not collide on one boundary");
    }

    #[test]
    fn boundary_id_from_seed_keeps_summary_slug() {
        let id = boundary_id_from_seed_with_summary(
            "2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a",
            Some("Agent Doc Bugs"),
        );
        assert_eq!(id, "2ffa57c0:agent-doc-bugs");
    }

    #[test]
    fn boundary_id_from_seed_without_hex_is_still_deterministic() {
        let a = boundary_id_from_seed_with_summary("zzzz-non-hex-seed", None);
        let b = boundary_id_from_seed_with_summary("zzzz-non-hex-seed", None);
        assert_eq!(a, b);
        assert_eq!(a.len(), BOUNDARY_ID_LEN);
    }
}
