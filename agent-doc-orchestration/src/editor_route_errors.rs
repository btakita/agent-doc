use std::path::Path;

use agent_doc_controller::editor_route_error::editor_route_error_path_for_file;

pub fn clear_for_success(file: &Path, reason: &str) -> bool {
    let Some(path) = editor_route_error_path_for_file(file) else {
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
    fn clear_for_success_removes_saved_diagnostic() {
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
        assert!(!clear_for_success(&doc, "test"));
    }
}
