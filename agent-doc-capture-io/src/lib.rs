//! Response capture ledger filesystem paths.

use anyhow::Result;
use std::path::{Path, PathBuf};

const CAPTURE_SUBDIR: &str = ".agent-doc/captures";

/// Directory for a document's response-capture sidecars:
/// `<project_root>/.agent-doc/captures/<doc-hash>`.
pub fn capture_dir_for(file: &Path) -> Result<PathBuf> {
    let canonical = file.canonicalize()?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    Ok(project_root.join(CAPTURE_SUBDIR).join(hash))
}

/// Path to a committed response-capture sidecar for `capture_id`.
pub fn capture_path_for(file: &Path, capture_id: &str) -> Result<PathBuf> {
    Ok(capture_dir_for(file)?.join(format!("{capture_id}.json")))
}

/// Path to a partial response checkpoint sidecar for `cycle_id`.
pub fn partial_capture_path_for(file: &Path, cycle_id: &str) -> Result<PathBuf> {
    Ok(capture_dir_for(file)?.join(format!("{cycle_id}.partial.json")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn capture_paths_resolve_under_agent_doc_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        fs::create_dir_all(root.join("nested")).unwrap();
        let doc = root.join("nested/doc.md");
        fs::write(&doc, "body").unwrap();

        let canonical = doc.canonicalize().unwrap();
        let hash = agent_doc_fs::document_state_hash(&canonical).unwrap();

        assert_eq!(
            capture_dir_for(&doc).unwrap(),
            root.join(".agent-doc/captures").join(&hash)
        );
        assert_eq!(
            capture_path_for(&doc, "cycle-a").unwrap(),
            root.join(".agent-doc/captures")
                .join(&hash)
                .join("cycle-a.json")
        );
        assert_eq!(
            partial_capture_path_for(&doc, "cycle-a").unwrap(),
            root.join(".agent-doc/captures")
                .join(&hash)
                .join("cycle-a.partial.json")
        );
    }

    #[test]
    fn capture_paths_follow_project_root_or_file_parent_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let canonical = doc.canonicalize().unwrap();
        let hash = agent_doc_fs::document_state_hash(&canonical).unwrap();
        let project_root =
            agent_doc_project_root_io::project_root_or_file_parent(&canonical).unwrap();

        assert_eq!(
            capture_dir_for(&doc).unwrap(),
            project_root.join(".agent-doc/captures").join(hash)
        );
    }
}
