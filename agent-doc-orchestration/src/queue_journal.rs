//! Crash-durable operator queue-edit journal (`#qdurcrash`).
//!
//! Failure this fixes: an operator adds an `agent:queue` item, the turn starts,
//! the supervisor + tmux pane crash and restart, and the operator-added queue
//! item is **gone** — it lived only in the editor buffer / in-memory CRDT and
//! was never persisted to a crash-durable on-disk store before the process died,
//! so restart reloaded from the last snapshot/HEAD (predating the add) and
//! dropped it. (Distinct from `#semmerge` live-merge drop and `#routedeadshell`
//! dispatch-into-dead-shell.)
//!
//! Mechanism (additive — this never re-wires the post-commit worktree reconcile
//! that the `#fintol2`/`#pcwc` carry-forward invariant guards):
//!
//! - [`record`] runs every time the binary observes the live queue (preflight
//!   queue maintenance). It appends each operator queue *prompt* it sees to an
//!   append-only, fsync'd sidecar (`.agent-doc/queue-journal/<doc-hash>.jsonl`)
//!   if not already journaled. This is the "persist inbound operator queue
//!   patches immediately on receipt" half.
//! - [`replay_missing`] runs at supervisor (re)start. It returns the journaled
//!   prompts that are absent from the reloaded document's queue — i.e. operator
//!   additions lost to the crash — so the caller can re-insert them. This is the
//!   "supervisor-startup reconcile that merges the on-disk document queue against
//!   the reloaded snapshot" half.
//! - [`clear`] runs on every clean closeout/commit: once a cycle commits, the
//!   queue state is durable in the snapshot, so the journal is emptied. The
//!   journal therefore only ever holds operator additions observed since the last
//!   commit (the crash window), which bounds the only edge case (an add-then-
//!   delete entirely within one pre-commit window).
//!
//! Conservative + best-effort: it only ever *adds* queue prompts back, never
//! removes anything, and a missing/unreadable/unwritable journal degrades to a
//! no-op (logged, never fatal) so a journal hiccup cannot wedge a session.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document queue journals.
/// Mirrors the sibling `.agent-doc/drain-owner` / `.agent-doc/live-buffer`
/// sidecar layout.
const QUEUE_JOURNAL_DIR: &str = ".agent-doc/queue-journal";

/// One journaled operator queue prompt. `text` is the canonical prompt text as
/// parsed by [`agent_doc_queue::document_queue::parse`] (the `- ` bullet / fence markers stripped),
/// so it round-trips through [`agent_doc_queue::document_queue::QueuePrompt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueJournalEntry {
    /// Canonical prompt text (matches `QueuePrompt::text`).
    pub text: String,
    /// Whether the prompt is a multiline (`--- … ---`) queue entry.
    #[serde(default)]
    pub multiline: bool,
}

/// Resolve the journal path for `file`, or `None` when the project root cannot be
/// resolved (no `.agent-doc` ancestor). Best-effort: callers treat `None` as
/// "no journal" rather than an error.
fn journal_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = crate::snapshot::find_project_root(&canonical)?;
    let hash = crate::snapshot::doc_hash(&canonical).ok()?;
    Some(root.join(QUEUE_JOURNAL_DIR).join(format!("{hash}.jsonl")))
}

/// Parse the queue prompts (`Prompt` entries only — never `Completed`, presets,
/// dispatches, fences, or freeform noise) from a document's `agent:queue`
/// component. Returns an empty vec when there is no queue component.
fn queue_prompts(content: &str) -> Vec<agent_doc_queue::document_queue::QueuePrompt> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = agent_doc_queue::document_queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// Read the journal entries for `file` (empty when absent/unreadable).
fn read_journal(path: &Path) -> Vec<QueueJournalEntry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<QueueJournalEntry>(line).ok())
        .collect()
}

/// Record every operator queue prompt currently in `content` to the durable
/// journal, appending only prompts not already journaled. Best-effort: a
/// resolution/IO failure logs to stderr and returns `Ok(())` (never wedges a
/// cycle on a journal hiccup).
///
/// Called from preflight queue maintenance so an operator queue add is captured
/// to a crash-durable store on the first cycle that observes it.
pub fn record(file: &Path, content: &str) -> Result<()> {
    append_prompts(file, queue_prompts(content))
}

/// Journal operator queue prompts from the live editor buffer (`#qftloss`
/// mode-6).
///
/// Mode-6 is the gap *before* the first cycle observes an operator queue add:
/// `record` only ever sees on-disk content (it runs during preflight queue
/// maintenance), so an operator queue edit that lives **only** in the editor
/// buffer — never flushed to disk, never observed by any cycle — is invisible to
/// the disk-backed `#qdurcrash` journal. If a concurrent commit / convergence /
/// `git checkout HEAD` recovery overwrites that unsaved buffer (or the buffer is
/// killed) before a cycle reads it, the operator's free-text prompt is lost.
///
/// The JB / VS Code plugins already report the operator's full editor buffer to
/// `.agent-doc/live-buffer/<hash>` on every keystroke (debounced) via
/// [`agent_doc_debounce::document_changed_with_content`]. This reads that durable
/// editor-buffer sidecar and journals any operator queue prompt it contains that
/// is not already journaled, so the prompt enters the **same** `#qdurcrash`
/// journal substrate and is replayed by [`replay_missing`] on the next cycle /
/// restart — closing the pre-first-observation gap binary-side without changing
/// the IPC-first write path or the convergence/merge.
///
/// Best-effort and additive: a missing/unreadable sidecar is a no-op, and a
/// prompt already present in the journal is never double-recorded.
pub fn record_live_buffer(file: &Path) -> Result<()> {
    let Some(file_str) = file.to_str() else {
        return Ok(());
    };
    // Each open editor reports its own sidecar; journal the prompts from every
    // one so a multi-editor session never drops one editor's pending add.
    let mut prompts: Vec<agent_doc_queue::document_queue::QueuePrompt> = Vec::new();
    for snapshot in agent_doc_debounce::live_buffer_snapshots(file_str) {
        let Some(buffer) = snapshot.content.as_deref() else {
            // len/hash-only sidecar: we cannot recover the prompt text from a
            // digest, so there is nothing to journal from it.
            continue;
        };
        for prompt in queue_prompts(buffer) {
            if !prompts.iter().any(|p| p.text == prompt.text) {
                prompts.push(prompt);
            }
        }
    }
    append_prompts(file, prompts)
}

/// Shared append path for [`record`] and [`record_live_buffer`]: durably append
/// every prompt not already journaled. Best-effort (logs + degrades to a no-op on
/// any resolution/IO failure).
fn append_prompts(
    file: &Path,
    prompts: Vec<agent_doc_queue::document_queue::QueuePrompt>,
) -> Result<()> {
    if prompts.is_empty() {
        return Ok(());
    }
    let Some(path) = journal_path(file) else {
        return Ok(());
    };
    let existing = read_journal(&path);
    let mut appended = String::new();
    for prompt in prompts {
        let already = existing.iter().any(|e| e.text == prompt.text);
        if already {
            continue;
        }
        let entry = QueueJournalEntry {
            text: prompt.text.clone(),
            multiline: prompt.multiline,
        };
        match serde_json::to_string(&entry) {
            Ok(line) => {
                appended.push_str(&line);
                appended.push('\n');
            }
            Err(err) => {
                eprintln!(
                    "[agent-doc] queue_journal: failed to serialize entry ({err:#}) — skipping"
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
            "[agent-doc] queue_journal: failed to create {} ({err:#}) — skipping record",
            parent.display()
        );
        return Ok(());
    }
    // Append-only durability: open for append, write, fsync.
    use std::io::Write;
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

/// Return the journaled operator queue prompts that are **absent** from
/// `content`'s queue (neither a live `Prompt` nor a struck `Completed` entry) —
/// i.e. operator additions lost to a crash+restart. Empty when the journal is
/// absent or every journaled prompt is still present/consumed.
///
/// Called at supervisor (re)start: the caller re-inserts the returned prompts so
/// the crash+restart replays the pending queue edit instead of losing it.
pub fn replay_missing(file: &Path, content: &str) -> Vec<QueueJournalEntry> {
    let Some(path) = journal_path(file) else {
        return Vec::new();
    };
    let journal = read_journal(&path);
    if journal.is_empty() {
        return Vec::new();
    }
    // Present = a live prompt with the same text, OR a struck/completed entry
    // (the operator already worked/cancelled it — do NOT resurrect).
    let mut present_texts = present_queue_texts(content);
    present_texts.extend(durable_queue_texts(file));
    journal
        .into_iter()
        .filter(|entry| !present_texts.contains(&entry.text))
        .collect()
}

/// Queue prompt texts that have reached the binary's durable snapshot. If such a
/// prompt is absent from the live document, treat that absence as an operator
/// deletion rather than a crash-lost add; the journal has already served its
/// purpose once the prompt is snapshot-visible.
fn durable_queue_texts(file: &Path) -> std::collections::HashSet<String> {
    crate::snapshot::load(file)
        .ok()
        .flatten()
        .map(|content| present_queue_texts(&content))
        .unwrap_or_default()
}

/// All queue prompt texts currently represented in `content` — live `Prompt`
/// AND struck `Completed` — so a consumed/cancelled item is treated as present
/// (never resurrected by replay).
fn present_queue_texts(content: &str) -> std::collections::HashSet<String> {
    let mut texts = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return texts;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return texts;
    };
    let body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = agent_doc_queue::document_queue::parse(body) else {
        return texts;
    };
    for entry in entries {
        match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(p)
            | agent_doc_queue::document_queue::QueueEntry::Completed(p) => {
                texts.insert(p.text);
            }
            _ => {}
        }
    }
    texts
}

/// Re-insert the lost operator queue prompts (from [`replay_missing`]) at the end
/// of `content`'s `agent:queue` component, returning the rewritten content. Returns
/// `Ok(None)` when nothing is missing or there is no queue component to merge into
/// (conservative: never fabricate a queue component). Pure (content-only) so the
/// merge is unit-testable without disk.
pub fn merge_missing_into_content(
    missing: &[QueueJournalEntry],
    content: &str,
) -> Result<Option<String>> {
    if missing.is_empty() {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Ok(None);
    };
    let body = &content[queue.open_end..queue.close_start];
    let mut entries = agent_doc_queue::document_queue::parse(body)
        .context("queue_journal: failed to parse queue")?;
    let present = present_queue_texts(content);
    let mut added = false;
    for entry in missing {
        if present.contains(&entry.text) {
            continue;
        }
        entries.push(agent_doc_queue::document_queue::QueueEntry::Prompt(
            agent_doc_queue::document_queue::QueuePrompt {
                text: entry.text.clone(),
                multiline: entry.multiline,
            },
        ));
        added = true;
    }
    if !added {
        return Ok(None);
    }
    let rendered = agent_doc_queue::document_queue::render(&entries);
    let mut new_content = String::with_capacity(content.len() + rendered.len());
    new_content.push_str(&content[..queue.open_end]);
    if !rendered.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&rendered);
    new_content.push_str(&content[queue.close_start..]);
    Ok(Some(new_content))
}

/// Clear the journal for `file` (best-effort). Called on every clean
/// closeout/commit: once a cycle commits, the queue state is durable in the
/// snapshot, so the journal must be emptied to bound the crash window to
/// post-last-commit operator additions only.
pub fn clear(file: &Path) {
    let Some(path) = journal_path(file) else {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn record_then_replay_recovers_a_lost_queue_add() {
        let dir = tempfile::tempdir().unwrap();
        // Operator added two queue items; the binary observes them.
        let path = doc(dir.path(), &["- do [#alpha]", "- do [#beta]"]);
        record(&path, &content_of(&path)).unwrap();

        // Crash+restart: the reloaded document lost `#beta` (only `#alpha`
        // survived in the stale snapshot).
        let reloaded = doc(dir.path(), &["- do [#alpha]"]);
        let missing = replay_missing(&reloaded, &content_of(&reloaded));
        assert_eq!(missing.len(), 1, "exactly the lost add should replay");
        assert_eq!(missing[0].text, "do [#beta]");

        // Merge re-inserts the lost item into the queue component.
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
    fn replay_does_not_resurrect_a_consumed_item() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]"]);
        record(&path, &content_of(&path)).unwrap();

        // The item was consumed (struck) — it must NOT be replayed.
        let struck = doc(dir.path(), &["- ~~do [#alpha]~~"]);
        let missing = replay_missing(&struck, &content_of(&struck));
        assert!(
            missing.is_empty(),
            "a struck/consumed item must not be resurrected: {missing:?}"
        );
    }

    #[test]
    fn replay_does_not_resurrect_operator_deleted_snapshot_completed_items() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(
            dir.path(),
            &[
                "- :pushpin: do [#qeditdup] [#qftloss#qftloss]",
                "- :pushpin: JB `File Cache Conflict` occurrences",
            ],
        );
        record(&path, &content_of(&path)).unwrap();

        let snapshot_content = doc_body(&[
            "- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~",
            "- ~~:pushpin: JB `File Cache Conflict` occurrences~~ — auto-struck: answered this cycle (#ftstrike)",
        ]);
        crate::snapshot::save(&path, &snapshot_content).unwrap();

        let current_after_operator_delete = doc_body(&["- do [#stillopen]"]);
        std::fs::write(&path, &current_after_operator_delete).unwrap();

        let missing = replay_missing(&path, &current_after_operator_delete);
        assert!(
            missing.is_empty(),
            "snapshot-completed rows deleted by the operator must not be journal-replayed: {missing:?}"
        );
    }

    #[test]
    fn record_is_idempotent_and_clear_empties_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]"]);
        record(&path, &content_of(&path)).unwrap();
        record(&path, &content_of(&path)).unwrap();

        let journal_file = journal_path(&path).unwrap();
        assert_eq!(
            read_journal(&journal_file).len(),
            1,
            "record must not double-journal the same prompt"
        );

        clear(&path);
        assert!(
            replay_missing(&path, "no queue here").is_empty(),
            "clear must empty the journal"
        );
    }

    #[test]
    fn merge_is_a_noop_without_a_queue_component() {
        let missing = vec![QueueJournalEntry {
            text: "do [#alpha]".to_string(),
            multiline: false,
        }];
        // Conservative: never fabricate a queue component.
        assert!(
            merge_missing_into_content(&missing, "no components here")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn absent_journal_replays_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]"]);
        assert!(replay_missing(&path, &content_of(&path)).is_empty());
    }

    /// Build the same template-mode doc body `doc()` writes, returned as a string
    /// (not written to disk) — used to model the operator's live editor buffer.
    fn doc_body(queue_lines: &[&str]) -> String {
        let mut body = String::from(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue auto -->\n",
        );
        for line in queue_lines {
            body.push_str(line);
            body.push('\n');
        }
        body.push_str("<!-- /agent:queue -->\n");
        body
    }

    /// `#qftloss` mode-6: an operator free-text queue prompt that lives ONLY in
    /// the live editor buffer — never flushed to disk, never observed by any cycle
    /// — must survive a concurrent commit/convergence/recovery that overwrites the
    /// unsaved buffer with the (older) on-disk content.
    ///
    /// Scenario:
    ///   1. The doc on disk has a clean queue (`#alpha`).
    ///   2. The operator types a free-text prompt into the editor; the plugin
    ///      reports the full buffer to `.agent-doc/live-buffer/<hash>` (this is the
    ///      already-shipped buffer-flush-on-edit). The add is NOT on disk and no
    ///      cycle has observed it.
    ///   3. `record_live_buffer` (run at startup / early preflight) journals the
    ///      editor-buffer prompt into the `#qdurcrash` substrate.
    ///   4. A concurrent commit/recovery overwrites the editor buffer — the only
    ///      copy of the add — leaving disk == the pre-add content.
    ///   5. On the next cycle, `replay_missing` + `merge_missing_into_content`
    ///      re-insert the operator's free-text prompt instead of dropping it.
    #[test]
    fn live_buffer_only_operator_add_survives_concurrent_clobber() {
        let dir = tempfile::tempdir().unwrap();
        // Disk: clean queue, no operator add yet. (Step 1.)
        let path = doc(dir.path(), &["- do [#alpha]"]);
        let file_str = path.to_string_lossy().to_string();

        // No journal entry exists for the operator's free-text prompt yet: the
        // disk-backed `record` cannot see an unflushed editor-buffer edit.
        record(&path, &content_of(&path)).unwrap();
        assert!(
            replay_missing(&path, &content_of(&path)).is_empty(),
            "precondition: nothing journaled for the editor-only add yet"
        );

        // Step 2: operator types a free-text prompt; the plugin reports the full
        // editor buffer (disk queue + the new operator line) to the live-buffer
        // sidecar. The add never reaches disk.
        let editor_buffer = doc_body(&["- do [#alpha]", "- investigate the prod outage"]);
        agent_doc_debounce::document_changed_with_content(&file_str, &editor_buffer);

        // Step 3: capture the editor-buffer add into the durable journal.
        record_live_buffer(&path).unwrap();

        // Step 4: a concurrent commit/convergence/`git checkout HEAD` recovery
        // overwrites the unsaved editor buffer — disk is still the pre-add content
        // (the operator's only copy of the prompt is now gone from live state).
        let reloaded = content_of(&path);
        assert!(
            !reloaded.contains("investigate the prod outage"),
            "disk must NOT contain the editor-only add (it was clobbered)"
        );

        // Step 5: the next cycle replays the journaled operator prompt.
        let missing = replay_missing(&path, &reloaded);
        assert_eq!(
            missing.len(),
            1,
            "the editor-only operator add must be recoverable from the journal: {missing:?}"
        );
        assert_eq!(missing[0].text, "investigate the prod outage");

        let merged = merge_missing_into_content(&missing, &reloaded)
            .unwrap()
            .expect("the recovered editor-only add must produce merged content");
        assert!(
            merged.contains("- investigate the prod outage"),
            "operator free-text prompt must be re-inserted, not dropped:\n{merged}"
        );
        assert!(
            merged.contains("- do [#alpha]"),
            "the survivor must be preserved:\n{merged}"
        );
    }

    /// `#qftloss`: a live-buffer add that was subsequently consumed/struck on disk
    /// must NOT be resurrected (the operator already worked or cancelled it).
    #[test]
    fn live_buffer_record_does_not_resurrect_a_struck_add() {
        let dir = tempfile::tempdir().unwrap();
        let path = doc(dir.path(), &["- do [#alpha]"]);
        let file_str = path.to_string_lossy().to_string();

        // Operator's editor buffer held a free-text add; journal it from the buffer.
        let editor_buffer = doc_body(&["- do [#alpha]", "- review the migration plan"]);
        agent_doc_debounce::document_changed_with_content(&file_str, &editor_buffer);
        record_live_buffer(&path).unwrap();

        // The add was later worked and struck on disk — replay must treat it as
        // present (consumed), never resurrect it.
        let struck = doc(
            dir.path(),
            &["- do [#alpha]", "- ~~review the migration plan~~"],
        );
        let missing = replay_missing(&struck, &content_of(&struck));
        assert!(
            missing.is_empty(),
            "a struck/consumed editor-buffer add must not be resurrected: {missing:?}"
        );
    }
}
