use std::path::Path;

use agent_doc_controller::editor_route_error::{
    EditorRouteErrorClearResult, clear_editor_route_error_for_file,
};

pub fn clear_for_success(file: &Path, reason: &str) -> bool {
    match clear_editor_route_error_for_file(file) {
        EditorRouteErrorClearResult::Cleared { path } => {
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
        EditorRouteErrorClearResult::NotFound { .. }
        | EditorRouteErrorClearResult::PathUnavailable => false,
        EditorRouteErrorClearResult::Failed { path, error } => {
            crate::ops_log::log_op(
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

        assert!(clear_for_success(&doc, "test"));
        assert!(!path.exists());
        let ops_log = dir.path().join(".agent-doc/logs/ops.log");
        let log = std::fs::read_to_string(ops_log).unwrap();
        assert!(log.contains("editor_route_error_cleared"));
        assert!(log.contains("reason=test"));
        assert!(log.contains(&format!("path={}", path.display())));
        assert!(!clear_for_success(&doc, "test"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap(),
            log
        );
    }

    #[test]
    fn clear_for_success_returns_false_without_log_for_missing_diagnostic() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs5.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        assert!(!clear_for_success(&doc, "test"));
        assert!(!dir.path().join(".agent-doc/logs/ops.log").exists());
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

        assert!(!clear_for_success(&doc, "test"));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("editor_route_error_clear_failed"));
        assert!(log.contains("reason=test"));
        assert!(log.contains(&format!("path={}", path.display())));
        assert!(log.contains("err="));
    }
}
