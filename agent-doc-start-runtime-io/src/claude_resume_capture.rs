//! `#clauderesumeafterclear`: keep `resume.claude` on the conversation the
//! supervised Claude Code harness is actually running.
//!
//! A pane-launched Claude session reports its conversation id to agent-doc only
//! through hooks. `start` mints and records the id at launch, but an operator
//! `/clear` (and any non-resume startup) begins a NEW conversation under a new
//! id. Before this capture the document kept the pre-clear id, so every later
//! relaunch and Restart Agent resumed the pre-clear conversation (observed
//! 2026-10-20: `resume` stayed `ba846c4c` after the `/clear` that started
//! `e62ea89f`). Codex already follows its thread through the hook ledger
//! projection; this is the Claude counterpart, driven by the `SessionStart`
//! hook (`agent-doc turn-status idle`).
//!
//! The write goes through [`record_document_resume_id_for_harness`] — the
//! binary-owned, realtime-authority frontmatter write — and only after
//! [`agent_doc_turn_status_io::resolve_claude_session_start_resume`] proves the
//! document and pane. Every unprovable shape is a skip, never a guess.

use std::path::Path;

use crate::record_document_resume_id_for_harness;

/// Outcome of one hook payload, for logging and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeResumeCaptureOutcome {
    /// The payload is not a new-conversation `SessionStart` (most hooks).
    NotApplicable,
    /// A new conversation was reported but its ownership was not proven.
    Refused(&'static str),
    /// `resume.claude` now names the new id (`changed=false`: it already did).
    Recorded { changed: bool },
    /// Ownership was proven but the authority write failed.
    WriteFailed(String),
}

/// `turn-status idle` payload hook: record a proven new Claude conversation id
/// on the pane's document. Best-effort; never fails the harness hook.
pub fn capture_claude_session_start_resume(base: &Path, pane: &str, payload: &str) {
    let ancestors = agent_doc_turn_status_io::hook_process_ancestors();
    let _ = capture_claude_session_start_resume_with_ancestors(base, pane, payload, &ancestors);
}

/// [`capture_claude_session_start_resume`] with the hook process ancestry
/// injected (tests).
pub fn capture_claude_session_start_resume_with_ancestors(
    base: &Path,
    pane: &str,
    payload: &str,
    ancestors: &[(u32, String)],
) -> ClaudeResumeCaptureOutcome {
    let capture = match agent_doc_turn_status_io::resolve_claude_session_start_resume(
        base, pane, payload, ancestors,
    ) {
        Ok(capture) => capture,
        Err(skip) => {
            let Some(candidate) = skip.candidate else {
                return ClaudeResumeCaptureOutcome::NotApplicable;
            };
            let line = format!(
                "claude_session_start_resume_refused pane={pane} id={} source={} reason={}",
                candidate.session_id, candidate.source, skip.reason
            );
            eprintln!("[turn-status] {line}");
            if let Some(document) = skip.document.as_deref() {
                agent_doc_ops_log_io::log_op(document, &line);
            }
            return ClaudeResumeCaptureOutcome::Refused(skip.reason);
        }
    };
    match record_document_resume_id_for_harness(&capture.document, "claude", &capture.session_id) {
        Ok(changed) => {
            agent_doc_ops_log_io::log_op(
                &capture.document,
                &format!(
                    "claude_session_start_resume_recorded pane={pane} id={} source={} changed={changed}",
                    capture.session_id, capture.source
                ),
            );
            ClaudeResumeCaptureOutcome::Recorded { changed }
        }
        Err(err) => {
            let error = format!("{err:#}").replace('\n', "\\n");
            let line = format!(
                "claude_session_start_resume_record_failed pane={pane} id={} source={} error={error}",
                capture.session_id, capture.source
            );
            eprintln!("[turn-status] {line}");
            agent_doc_ops_log_io::log_op(&capture.document, &line);
            ClaudeResumeCaptureOutcome::WriteFailed(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_frontmatter::frontmatter;

    fn seed(base: &Path, doc: &Path, pane: &str, supervisor_pid: u32) {
        let conn = agent_doc_sqlite::state_store::open_state_db(base).unwrap();
        let id = doc.to_string_lossy().into_owned();
        conn.execute(
            "INSERT INTO documents (document_id, canonical_path, session_id, generation, pane_id, \
             window_id, harness_id, actor_state) VALUES (?1, ?1, 'doc-session', 3, ?2, '@1', 'claude-code', 'ready')",
            (&id, pane),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO supervisor_leases (document_id, generation, supervisor_pid) VALUES (?1, 3, ?2)",
            (&id, supervisor_pid),
        )
        .unwrap();
    }

    fn payload(source: &str, id: &str) -> String {
        format!(
            r#"{{"hook_event_name":"SessionStart","source":"{source}","session_id":"{id}","transcript_path":"/h/.claude/projects/p/{id}.jsonl"}}"#
        )
    }

    fn resume_ids(doc: &Path) -> (Option<String>, Option<String>) {
        let content = std::fs::read_to_string(doc).unwrap();
        let (fm, _) = frontmatter::parse(&content).unwrap();
        (
            fm.resume_for_harness("claude").map(str::to_string),
            fm.resume_for_harness("codex").map(str::to_string),
        )
    }

    /// `#clauderesumeafterclear` regression (2026-10-20): `/clear` started
    /// `e62ea89f` but the document kept `ba846c4c`, so relaunch and Restart
    /// Agent both resumed the pre-clear conversation.
    #[test]
    fn clear_session_start_moves_resume_claude_to_the_new_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("agent-doc-bugs.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\nresume:\n  claude: ba846c4c\n  codex: codex-thread\n---\n\n# Bugs\n",
        )
        .unwrap();
        seed(&base, &doc, "%2", 1605424);
        let from_harness = vec![
            (2909304, "claude".to_string()),
            (1605424, "agent-doc".to_string()),
        ];

        assert_eq!(
            capture_claude_session_start_resume_with_ancestors(
                &base,
                "%2",
                &payload("clear", "e62ea89f"),
                &from_harness
            ),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        assert_eq!(
            resume_ids(&doc),
            (Some("e62ea89f".into()), Some("codex-thread".into())),
            "the clear moves only the Claude pointer"
        );
        assert!(std::fs::read_to_string(&doc).unwrap().ends_with("# Bugs\n"));

        // `resume` reloads the same conversation: no write, pointer unchanged.
        assert_eq!(
            capture_claude_session_start_resume_with_ancestors(
                &base,
                "%2",
                &payload("resume", "ba846c4c"),
                &from_harness
            ),
            ClaudeResumeCaptureOutcome::NotApplicable
        );
        assert_eq!(resume_ids(&doc).0.as_deref(), Some("e62ea89f"));

        // A startup that reports the id `start` already recorded is a no-op.
        assert_eq!(
            capture_claude_session_start_resume_with_ancestors(
                &base,
                "%2",
                &payload("startup", "e62ea89f"),
                &from_harness
            ),
            ClaudeResumeCaptureOutcome::Recorded { changed: false }
        );
    }

    /// `#clauderesumeafterclear`: a nested `claude -p` run from inside the
    /// pane reports a fresh `startup` id with the pane's `$TMUX_PANE`; it must
    /// never move the document's pointer.
    #[test]
    fn nested_harness_session_start_never_moves_resume_claude() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("doc.md");
        std::fs::write(&doc, "---\nresume:\n  claude: main-session\n---\n").unwrap();
        seed(&base, &doc, "%3", 10);
        let nested = vec![
            (40, "claude".to_string()),
            (35, "zsh".to_string()),
            (20, "claude".to_string()),
            (10, "agent-doc".to_string()),
        ];
        assert_eq!(
            capture_claude_session_start_resume_with_ancestors(
                &base,
                "%3",
                &payload("startup", "subagent-session"),
                &nested
            ),
            ClaudeResumeCaptureOutcome::Refused("hook_not_from_pane_supervisor_harness")
        );
        assert_eq!(resume_ids(&doc).0.as_deref(), Some("main-session"));
    }
}
