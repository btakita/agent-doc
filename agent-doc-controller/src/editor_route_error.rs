//! Editor route-error diagnostic naming policy.

use std::path::{Path, PathBuf};

pub const EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR: &str = ".agent-doc/state/editor-route-errors";

pub fn editor_route_error_diagnostic_name(relative_path: &str) -> String {
    let mut sanitized = String::new();
    for ch in relative_path.replace('\\', "/").chars() {
        match ch {
            '/' => sanitized.push_str("__"),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => sanitized.push(ch),
            _ => sanitized.push('_'),
        }
    }
    if sanitized.is_empty() {
        "route-error".to_string()
    } else {
        sanitized
    }
}

pub fn editor_route_error_file_name(relative_path: &str) -> String {
    format!("{}.txt", editor_route_error_diagnostic_name(relative_path))
}

pub fn editor_route_error_path_for_file(file: &Path) -> Option<PathBuf> {
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let project_root = agent_doc_fs::find_project_root(&canonical)?;
    let relative = canonical
        .strip_prefix(&project_root)
        .ok()
        .unwrap_or(canonical.as_path())
        .to_string_lossy()
        .trim_start_matches(std::path::MAIN_SEPARATOR)
        .replace(std::path::MAIN_SEPARATOR, "/");
    Some(
        project_root
            .join(EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR)
            .join(editor_route_error_file_name(&relative)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_name_matches_editor_sanitization() {
        assert_eq!(
            editor_route_error_diagnostic_name("tasks/agent-doc/agent-doc-bugs2.md"),
            "tasks__agent-doc__agent-doc-bugs2.md"
        );
        assert_eq!(
            editor_route_error_file_name("tasks\\agent doc\\bug?.md"),
            "tasks__agent_doc__bug_.md.txt"
        );
    }

    #[test]
    fn diagnostic_name_falls_back_for_empty_paths() {
        assert_eq!(editor_route_error_diagnostic_name(""), "route-error");
        assert_eq!(editor_route_error_file_name(""), "route-error.txt");
    }

    #[test]
    fn route_error_path_matches_editor_sanitization() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        let path = editor_route_error_path_for_file(&doc).unwrap();

        assert_eq!(
            path,
            dir.path().join(
                ".agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs2.md.txt"
            )
        );
    }
}
