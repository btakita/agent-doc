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
    let id = new_boundary_id();
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
        assert_eq!(new_boundary_id_with_summary(Some("")).len(), BOUNDARY_ID_LEN);
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
}
