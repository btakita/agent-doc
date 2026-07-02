//! Crash-durable operator queue-edit journal (`#qdurcrash`).

use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};

use agent_doc_queue::document_queue::QueuePrompt;
use agent_doc_queue::queue_journal as queue_journal_policy;
use agent_doc_queue::queue_journal::QueueJournalEntry;

/// Directory (relative to the project root) holding per-document queue journals.
pub const QUEUE_JOURNAL_DIR: &str = ".agent-doc/queue-journal";

/// Resolve the queue journal sidecar path for `file`.
///
/// Returns `None` when no `.agent-doc` project root can be resolved or the
/// document state hash cannot be computed.
pub fn queue_journal_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)?;
    let hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    Some(root.join(QUEUE_JOURNAL_DIR).join(format!("{hash}.jsonl")))
}

/// Record every operator queue prompt currently in `content` to the durable
/// journal, appending only prompts not already journaled.
pub fn record(file: &Path, content: &str) -> Result<()> {
    append_prompts(file, queue_journal_policy::queue_prompts(content))
}

/// Journal operator queue prompts from durable live-editor buffer sidecars.
pub fn record_live_buffer(file: &Path) -> Result<()> {
    let Some(file_str) = file.to_str() else {
        return Ok(());
    };
    let snapshots = agent_doc_debounce::live_buffer_snapshots(file_str);
    let buffers = snapshots
        .iter()
        .filter_map(|snapshot| snapshot.content.as_deref());
    let prompts = queue_journal_policy::unique_queue_prompts_from_contents(buffers);
    append_prompts(file, prompts)
}

fn append_prompts(file: &Path, prompts: Vec<QueuePrompt>) -> Result<()> {
    if prompts.is_empty() {
        return Ok(());
    }
    let Some(path) = queue_journal_path(file) else {
        return Ok(());
    };
    let existing = read_journal(&path);
    let entries = queue_journal_policy::plan_append_entries(&existing, prompts);
    let mut appended = String::new();
    for entry in entries {
        match serde_json::to_string(&entry) {
            Ok(line) => {
                appended.push_str(&line);
                appended.push('\n');
            }
            Err(err) => {
                eprintln!(
                    "[agent-doc] queue_journal: failed to serialize entry ({err:#}); skipping"
                );
            }
        }
    }
    if appended.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "[agent-doc] queue_journal: failed to create {} ({err:#}); skipping record",
            parent.display()
        );
        return Ok(());
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            if let Err(err) = f.write_all(appended.as_bytes()) {
                eprintln!(
                    "[agent-doc] queue_journal: failed to append to {} ({err:#})",
                    path.display()
                );
                return Ok(());
            }
            if let Err(err) = f.sync_all() {
                eprintln!(
                    "[agent-doc] queue_journal: fsync failed for {} ({err:#})",
                    path.display()
                );
            }
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_journal: failed to open {} for append ({err:#})",
                path.display()
            );
        }
    }
    Ok(())
}

/// Return journaled operator queue prompts absent from the current document and
/// optional durable snapshot content.
pub fn replay_missing(
    file: &Path,
    content: &str,
    durable_content: Option<&str>,
) -> Vec<QueueJournalEntry> {
    let Some(path) = queue_journal_path(file) else {
        return Vec::new();
    };
    let journal = read_journal(&path);
    if journal.is_empty() {
        return Vec::new();
    }
    queue_journal_policy::replay_missing_entries(journal, content, durable_content)
}

/// Clear the journal for `file` best-effort.
pub fn clear(file: &Path) {
    let Some(path) = queue_journal_path(file) else {
        return;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_journal: failed to clear {} ({err:#})",
                path.display()
            );
        }
    }
}

fn read_journal(path: &Path) -> Vec<QueueJournalEntry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<QueueJournalEntry>(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_journal::merge_missing_into_content;
    use std::path::PathBuf;

    fn doc(dir: &Path, queue_lines: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let mut body = String::from(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue auto -->\n",
        );
        for line in queue_lines {
            body.push_str(line);
            body.push('\n');
        }
        body.push_str("<!-- /agent:queue -->\n");
        let path = dir.join("session.md");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn content_of(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn queue_journal_path_uses_project_root_and_document_state_hash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert_eq!(
            queue_journal_path(&doc),
            Some(
                dir.path()
                    .join(".agent-doc/queue-journal")
                    .join(format!("{hash}.jsonl"))
            )
        );
    }

    #[test]
    fn queue_journal_path_returns_none_without_project_root() {
        let doc = Path::new("/__agent_doc_queue_io_no_project__/session.md");
        assert_eq!(queue_journal_path(&doc), None);
    }

    #[test]
    fn record_then_replay_recovers_a_lost_queue_add() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]", "- do [#beta]"]);
        record(&path, &content_of(&path)).unwrap();

        let reloaded = doc(dir.path(), &["- do [#alpha]"]);
        let missing = replay_missing(&reloaded, &content_of(&reloaded), None);
        assert_eq!(missing.len(), 1, "exactly the lost add should replay");
        assert_eq!(missing[0].text, "do [#beta]");

        let merged = merge_missing_into_content(&missing, &content_of(&reloaded))
            .unwrap()
            .expect("a lost add must produce merged content");
        assert!(merged.contains("- do [#beta]"), "merged:\n{merged}");
        assert!(
            merged.contains("- do [#alpha]"),
            "must keep survivor:\n{merged}"
        );
    }

    #[test]
    fn clear_removes_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]"]);
        record(&path, &content_of(&path)).unwrap();
        let journal_path = queue_journal_path(&path).expect("journal path");
        assert!(journal_path.exists());

        clear(&path);

        assert!(!journal_path.exists());
    }
}
