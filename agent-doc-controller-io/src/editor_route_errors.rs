use std::path::Path;

use agent_doc_controller::editor_route_error::{
    EditorRouteErrorClearResult, clear_editor_route_error_for_file,
};

/// Clear a saved editor route-error diagnostic after a successful route/sync
/// operation, logging only when there was a persisted diagnostic or a clear
/// failure.
pub fn clear_for_success(file: &Path, reason: &str, mut log_op: impl FnMut(&Path, &str)) -> bool {
    match clear_editor_route_error_for_file(file) {
        EditorRouteErrorClearResult::Cleared { path } => {
            log_op(
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
        EditorRouteErrorClearResult::NotFound { .. }
        | EditorRouteErrorClearResult::PathUnavailable => false,
        EditorRouteErrorClearResult::Failed { path, error } => {
            log_op(
                file,
                &format!(
                    "editor_route_error_clear_failed file={} reason={} path={} err={}",
                    file.display(),
                    reason,
                    path.display(),
                    error
                ),
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_controller::editor_route_error::editor_route_error_path_for_file;

    #[test]
    fn clear_for_success_removes_saved_diagnostic_and_logs() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs5.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let path = editor_route_error_path_for_file(&doc).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "Error: project controller returned ok response without data\n",
        )
        .unwrap();

        let mut logs = Vec::new();
        assert!(clear_for_success(&doc, "test", |_file, message| {
            logs.push(message.to_string());
        }));

        assert!(!path.exists());
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("editor_route_error_cleared"));
        assert!(logs[0].contains("reason=test"));
        assert!(logs[0].contains(&format!("path={}", path.display())));

        assert!(!clear_for_success(&doc, "test", |_file, message| {
            logs.push(message.to_string());
        }));
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn clear_for_success_returns_false_without_log_for_missing_diagnostic() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs5.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        let mut logs = Vec::new();
        assert!(!clear_for_success(&doc, "test", |_file, message| {
            logs.push(message.to_string());
        }));
        assert!(logs.is_empty());
    }

    #[test]
    fn clear_for_success_logs_failure_outcomes() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs5.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let path = editor_route_error_path_for_file(&doc).unwrap();
        std::fs::create_dir_all(&path).unwrap();

        let mut logs = Vec::new();
        assert!(!clear_for_success(&doc, "test", |_file, message| {
            logs.push(message.to_string());
        }));

        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("editor_route_error_clear_failed"));
        assert!(logs[0].contains("reason=test"));
        assert!(logs[0].contains(&format!("path={}", path.display())));
        assert!(logs[0].contains("err="));
    }
}
