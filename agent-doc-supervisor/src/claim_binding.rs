//! Pure claim binding policy for registry-backed document ownership.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimRegistryEntry<'a> {
    pub registry_key: &'a str,
    pub session_id: &'a str,
    pub file: &'a str,
    pub cwd: &'a str,
    pub window: &'a str,
}

pub fn registry_entry_matches_claimed_document(
    claimed_file: &Path,
    claimed_session_id: &str,
    entry: ClaimRegistryEntry<'_>,
    normalize_path: impl Fn(&Path) -> PathBuf,
) -> bool {
    if entry.session_id == claimed_session_id {
        return true;
    }
    if normalize_path(Path::new(entry.registry_key)) == claimed_file {
        return true;
    }
    if entry.file.is_empty() {
        return false;
    }
    normalize_path(Path::new(entry.file)) == claimed_file
}

pub fn claimed_session_label(entry: ClaimRegistryEntry<'_>) -> String {
    let owner = if entry.session_id.is_empty() {
        entry.registry_key
    } else {
        entry.session_id
    };
    owner.chars().take(8).collect()
}

pub fn find_alive_window_in_registry<'a>(
    entries: impl IntoIterator<Item = ClaimRegistryEntry<'a>>,
    cwd: &str,
    check_alive: impl Fn(&str) -> bool,
) -> Option<String> {
    for entry in entries {
        if entry.cwd != cwd || entry.window.is_empty() {
            continue;
        }
        if check_alive(entry.window) {
            return Some(entry.window.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(
        registry_key: &'a str,
        session_id: &'a str,
        file: &'a str,
        cwd: &'a str,
        window: &'a str,
    ) -> ClaimRegistryEntry<'a> {
        ClaimRegistryEntry {
            registry_key,
            session_id,
            file,
            cwd,
            window,
        }
    }

    fn normalize_from_repo(path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new("/repo").join(path)
        }
    }

    #[test]
    fn find_alive_window_returns_first_alive_match() {
        let entries = [
            entry("s1", "session-1", "a.md", "/project", "@1"),
            entry("s2", "session-2", "b.md", "/project", "@2"),
            entry("s3", "session-3", "c.md", "/project", "@3"),
        ];

        let result = find_alive_window_in_registry(entries, "/project", |w| w == "@3");
        assert_eq!(result, Some("@3".to_string()));
    }

    #[test]
    fn find_alive_window_skips_wrong_cwd() {
        let entries = [
            entry("s1", "session-1", "a.md", "/other-project", "@5"),
            entry("s2", "session-2", "b.md", "/project", "@6"),
        ];

        let result = find_alive_window_in_registry(entries, "/project", |w| w == "@5" || w == "@6");
        assert_eq!(result, Some("@6".to_string()));
    }

    #[test]
    fn find_alive_window_skips_empty_window() {
        let entries = [
            entry("s1", "session-1", "a.md", "/project", ""),
            entry("s2", "session-2", "b.md", "/project", "@7"),
        ];

        let result = find_alive_window_in_registry(entries, "/project", |_| true);
        assert_eq!(result, Some("@7".to_string()));
    }

    #[test]
    fn find_alive_window_returns_none_when_all_dead() {
        let entries = [
            entry("s1", "session-1", "a.md", "/project", "@1"),
            entry("s2", "session-2", "b.md", "/project", "@2"),
        ];

        let result = find_alive_window_in_registry(entries, "/project", |_| false);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_for_empty_registry() {
        let entries: [ClaimRegistryEntry<'_>; 0] = [];
        let result = find_alive_window_in_registry(entries, "/project", |_| true);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_when_no_cwd_match() {
        let entries = [entry("s1", "session-1", "a.md", "/other", "@1")];

        let result = find_alive_window_in_registry(entries, "/project", |_| true);
        assert_eq!(result, None);
    }

    #[test]
    fn registry_entry_matches_claimed_document_for_session_id() {
        let entry = entry(
            "legacy-key",
            "a9421282-fd96-4943-9af5-3561ed5cb799",
            "",
            "/repo",
            "@73",
        );

        assert!(registry_entry_matches_claimed_document(
            Path::new("/repo/tasks/sampleorders.md"),
            "a9421282-fd96-4943-9af5-3561ed5cb799",
            entry,
            normalize_from_repo,
        ));
    }

    #[test]
    fn registry_entry_matches_claimed_document_for_normalized_registry_key() {
        let entry = entry(
            "/repo/tasks/sampleorders.md",
            "a9421282-fd96-4943-9af5-3561ed5cb799",
            "tasks/other.md",
            "/repo",
            "@73",
        );

        assert!(registry_entry_matches_claimed_document(
            Path::new("/repo/tasks/sampleorders.md"),
            "different-session-id",
            entry,
            normalize_from_repo,
        ));
    }

    #[test]
    fn registry_entry_matches_claimed_document_for_relative_entry_file() {
        let entry = entry(
            "legacy-registry-key",
            "a9421282-fd96-4943-9af5-3561ed5cb799",
            "tasks/sampleorders.md",
            "/repo",
            "@73",
        );

        assert!(registry_entry_matches_claimed_document(
            Path::new("/repo/tasks/sampleorders.md"),
            "different-session-id",
            entry,
            normalize_from_repo,
        ));
    }

    #[test]
    fn registry_entry_does_not_match_empty_file_without_key_or_session_match() {
        let entry = entry("legacy-registry-key", "session-a", "", "/repo", "@73");

        assert!(!registry_entry_matches_claimed_document(
            Path::new("/repo/tasks/sampleorders.md"),
            "different-session-id",
            entry,
            normalize_from_repo,
        ));
    }

    #[test]
    fn claimed_session_label_prefers_session_id() {
        let label = claimed_session_label(entry(
            "registry-key",
            "a9421282-fd96-4943-9af5-3561ed5cb799",
            "",
            "/repo",
            "@73",
        ));

        assert_eq!(label, "a9421282");
    }

    #[test]
    fn claimed_session_label_falls_back_to_registry_key() {
        let label = claimed_session_label(entry("registry-key", "", "", "/repo", "@73"));

        assert_eq!(label, "registry");
    }
}
