//! Startup-miss marker and supervisor session-log path helpers.

use std::path::{Path, PathBuf};

use anyhow::Result;

const STARTUP_MISS_DIR: &str = ".agent-doc/state/startup-miss";
const SUPERVISOR_LOG_DIR: &str = ".agent-doc/logs";

/// Return the project root used for startup-miss sidecars for `file`.
pub fn startup_miss_project_root(file: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    agent_doc_fs::find_project_root(&canonical)
}

/// Compute `.agent-doc/state/startup-miss/<doc-hash>.json` for `file`.
pub fn startup_miss_state_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    Ok(Some(
        root.join(STARTUP_MISS_DIR).join(format!("{hash}.json")),
    ))
}

/// Compute `.agent-doc/logs/<session_id>.log` for `file`'s project.
pub fn supervisor_session_log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(SUPERVISOR_LOG_DIR)
            .join(format!("{session_id}.log")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_project(tmp: &Path) -> PathBuf {
        std::fs::create_dir_all(tmp.join(".agent-doc/state/startup-miss")).unwrap();
        let doc = tmp.join("nested").join("test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# test\n").unwrap();
        doc
    }

    fn temp_dir_without_agent_doc_ancestor() -> Option<tempfile::TempDir> {
        for base in [
            PathBuf::from("/var/tmp"),
            PathBuf::from("/dev/shm"),
            std::env::temp_dir(),
        ] {
            if !base.is_dir() || has_agent_doc_ancestor(&base) {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-supervisor-io-no-root")
                .tempdir_in(base)
            {
                return Some(dir);
            }
        }
        None
    }

    fn has_agent_doc_ancestor(path: &Path) -> bool {
        let Ok(mut current) = path.canonicalize() else {
            return false;
        };
        loop {
            if current.join(".agent-doc").is_dir() {
                return true;
            }
            if !current.pop() {
                return false;
            }
        }
    }

    #[test]
    fn startup_miss_state_path_uses_project_root_and_document_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let canonical = doc.canonicalize().unwrap();
        let hash = agent_doc_fs::document_state_hash(&canonical).unwrap();

        assert_eq!(
            startup_miss_state_path(&doc).unwrap(),
            Some(
                tmp.path()
                    .join(".agent-doc/state/startup-miss")
                    .join(format!("{hash}.json"))
            )
        );
    }

    #[test]
    fn supervisor_session_log_path_uses_project_root_and_session_id() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());

        assert_eq!(
            supervisor_session_log_path(&doc, "session-123").unwrap(),
            Some(tmp.path().join(".agent-doc/logs/session-123.log"))
        );
    }

    #[test]
    fn path_helpers_return_none_without_project_root() {
        let Some(tmp) = temp_dir_without_agent_doc_ancestor() else {
            return;
        };
        let doc = tmp.path().join("test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        assert!(startup_miss_state_path(&doc).unwrap().is_none());
        assert!(
            supervisor_session_log_path(&doc, "session-123")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn startup_miss_project_root_handles_missing_document_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("missing.md");

        assert_eq!(
            startup_miss_project_root(&doc),
            Some(tmp.path().to_path_buf())
        );
    }
}
