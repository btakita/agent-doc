//! IPC forensic sidecar capture.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcFullPromptForensicCapture {
    pub baseline_path: PathBuf,
    pub candidate_path: PathBuf,
}

/// Best-effort: preserve the baseline + corrupted candidate buffers under
/// `.agent-doc/logs/ipcfullprompt/` so the exact corruption shape can be
/// analyzed later. Returns written paths when at least one write succeeds.
pub fn preserve_ipcfullprompt_forensic(
    file: &Path,
    patch_id: Option<&str>,
    baseline: &str,
    candidate: &str,
) -> Option<IpcFullPromptForensicCapture> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let dir = root.join(".agent-doc/logs/ipcfullprompt");
    std::fs::create_dir_all(&dir).ok()?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = format!("{}-{}", ts, patch_id.unwrap_or("nopatch"));
    let baseline_path = dir.join(format!("{stem}.baseline.md"));
    let candidate_path = dir.join(format!("{stem}.candidate.md"));
    let baseline_written = std::fs::write(&baseline_path, baseline).is_ok();
    let candidate_written = std::fs::write(&candidate_path, candidate).is_ok();
    (baseline_written || candidate_written).then_some(IpcFullPromptForensicCapture {
        baseline_path,
        candidate_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_project_root_io::project_root_containing(base).is_some()
            {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-ipc-forensics-io-")
                .tempdir_in(base)
                && agent_doc_project_root_io::project_root_containing(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn preserves_fullprompt_forensics_under_project_log_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let capture = preserve_ipcfullprompt_forensic(&doc, Some("patch-1"), "base", "candidate")
            .expect("capture paths");

        let baseline_name = capture.baseline_path.file_name().unwrap().to_string_lossy();
        let candidate_name = capture
            .candidate_path
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(baseline_name.ends_with("patch-1.baseline.md"));
        assert!(candidate_name.ends_with("patch-1.candidate.md"));
        assert_eq!(
            std::fs::read_to_string(capture.baseline_path).unwrap(),
            "base"
        );
        assert_eq!(
            std::fs::read_to_string(capture.candidate_path).unwrap(),
            "candidate"
        );
    }

    #[test]
    fn skips_files_outside_agent_doc_project() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert!(preserve_ipcfullprompt_forensic(&doc, None, "base", "candidate").is_none());
    }
}
