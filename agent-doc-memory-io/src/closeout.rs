//! Closeout capture adapter for `tsift memory`.

use std::path::Path;

use agent_doc_turn::response_text::{
    response_prompt_target_from_re_heading, summarize_response_for_hook,
};
use tsift_memory::{MemoryStore, agent_doc_closeout_events};

/// Outcome of a best-effort tsift-memory closeout capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutCaptureOutcome {
    Captured,
    SkippedNoProjectRoot,
    SkippedNoMemoryDb,
    SkippedEmptyResponseSummary,
    CaptureFailed(String),
}

/// Capture the current response into `tsift memory` after a successful commit.
///
/// This hot path uses the lightweight `tsift-memory` library directly. It must
/// not spawn the full tsift CLI from the controller/UI process. Best-effort:
/// missing project roots, absent memory databases, and write failures never
/// fail hook closeout.
pub fn capture_tsift_memory_closeout(file: &Path, response_body: &str) -> CloseoutCaptureOutcome {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return CloseoutCaptureOutcome::SkippedNoProjectRoot;
    };
    let memory_db = project_root.join(".tsift/memory.db");
    if !memory_db.exists() {
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
    let events = agent_doc_closeout_events(
        &canonical,
        &prompt_target,
        &response_summary,
        Some(&commit_hash),
        "committed",
    );
    let capture =
        MemoryStore::open_or_create(&memory_db).and_then(|mut store| store.insert_events(&events));
    match capture {
        Ok(_) => {
            eprintln!(
                "[hooks] tsift-memory closeout capture ok for {}",
                file.display()
            );
            CloseoutCaptureOutcome::Captured
        }
        Err(err) => {
            eprintln!("[hooks] tsift-memory closeout capture failed: {err}");
            CloseoutCaptureOutcome::CaptureFailed(err.to_string())
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

    #[test]
    fn closeout_captures_through_tsift_memory_without_cli() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".tsift")).unwrap();
        let memory_db = tmp.path().join(".tsift/memory.db");
        MemoryStore::open_or_create(&memory_db).unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();

        assert_eq!(
            capture_tsift_memory_closeout(&doc, "### Re: prompt\n\nresponse"),
            CloseoutCaptureOutcome::Captured
        );
        let events = tsift_memory::read_memory_events(&memory_db, 20).unwrap();
        assert!(events.iter().any(|event| {
            event.kind == tsift_memory::MemoryEventKind::ResponseSummary
                && event.text == "### Re: prompt\n\nresponse"
        }));
    }
}
