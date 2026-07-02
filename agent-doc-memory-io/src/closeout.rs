//! Closeout capture adapter for `tsift memory`.

use std::path::Path;

use agent_doc_turn::response_text::{
    response_prompt_target_from_re_heading, summarize_response_for_hook,
};

/// Outcome of a best-effort tsift-memory closeout capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutCaptureOutcome {
    Captured,
    SkippedNoProjectRoot,
    SkippedNoMemoryDb,
    SkippedEmptyResponseSummary,
    SpawnFailed(String),
    NonZeroExit(Option<i32>, String),
}

/// Capture the current response into `tsift memory` after a successful commit.
///
/// Best-effort: missing project roots, absent memory databases, spawn failures,
/// and non-zero exits never fail hook closeout.
pub fn capture_tsift_memory_closeout(file: &Path, response_body: &str) -> CloseoutCaptureOutcome {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return CloseoutCaptureOutcome::SkippedNoProjectRoot;
    };
    if !project_root.join(".tsift/memory.db").exists() {
        return CloseoutCaptureOutcome::SkippedNoMemoryDb;
    }
    let response_summary = summarize_response_for_hook(response_body);
    if response_summary.trim().is_empty() {
        eprintln!(
            "[hooks] tsift-memory closeout capture skipped for {}: empty response body",
            file.display()
        );
        return CloseoutCaptureOutcome::SkippedEmptyResponseSummary;
    }
    let prompt_target = response_prompt_target_from_re_heading(response_body)
        .unwrap_or_else(|| canonical.display().to_string());
    let commit_hash = git_head(&project_root).unwrap_or_else(|| "unknown".to_string());
    let output = std::process::Command::new("tsift")
        .arg("memory")
        .arg("capture-agent-doc-closeout")
        .arg(&project_root)
        .arg("--session-path")
        .arg(&canonical)
        .arg("--prompt-target")
        .arg(prompt_target)
        .arg("--response-summary")
        .arg(response_summary)
        .arg("--commit-hash")
        .arg(commit_hash)
        .arg("--session-check-status")
        .arg("committed")
        .arg("--json")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            eprintln!(
                "[hooks] tsift-memory closeout capture ok for {}",
                file.display()
            );
            CloseoutCaptureOutcome::Captured
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            eprintln!(
                "[hooks] tsift-memory closeout capture exited with code {:?}: {}",
                output.status.code(),
                stderr.trim()
            );
            CloseoutCaptureOutcome::NonZeroExit(output.status.code(), stderr)
        }
        Err(err) => {
            eprintln!("[hooks] tsift-memory closeout capture failed to spawn: {err}");
            CloseoutCaptureOutcome::SpawnFailed(err.to_string())
        }
    }
}

fn git_head(project_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!head.is_empty()).then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeout_skips_empty_summary_when_memory_db_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".tsift")).unwrap();
        std::fs::write(tmp.path().join(".tsift/memory.db"), "").unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();

        assert_eq!(
            capture_tsift_memory_closeout(&doc, ""),
            CloseoutCaptureOutcome::SkippedEmptyResponseSummary
        );
    }

    #[test]
    fn closeout_skips_without_memory_db() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();

        assert_eq!(
            capture_tsift_memory_closeout(&doc, "### Re: prompt\n\nresponse"),
            CloseoutCaptureOutcome::SkippedNoMemoryDb
        );
    }
}
