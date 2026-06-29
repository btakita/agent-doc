use std::path::{Path, PathBuf};

fn sanitized_route_error_name(relative_path: &str) -> String {
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

fn route_error_path_for_file(file: &Path) -> Option<PathBuf> {
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
            .join(".agent-doc/state/editor-route-errors")
            .join(format!("{}.txt", sanitized_route_error_name(&relative))),
    )
}

pub fn clear_for_success(file: &Path, reason: &str) -> bool {
    let Some(path) = route_error_path_for_file(file) else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "editor_route_error_cleared file={} reason={} path={}",
                    file.display(),
                    reason,
                    path.display()
                ),
            );
            true
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "editor_route_error_clear_failed file={} reason={} path={} err={}",
                    file.display(),
                    reason,
                    path.display(),
                    err
                ),
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_error_path_matches_editor_sanitization() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        let path = route_error_path_for_file(&doc).unwrap();

        assert_eq!(
            path,
            dir.path().join(
                ".agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs2.md.txt"
            )
        );
    }

    #[test]
    fn clear_for_success_removes_saved_diagnostic() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs5.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let path = route_error_path_for_file(&doc).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "Error: project controller returned ok response without data\n",
        )
        .unwrap();

        assert!(clear_for_success(&doc, "test"));
        assert!(!path.exists());
        assert!(!clear_for_success(&doc, "test"));
    }
}
