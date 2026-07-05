//! File-backed one-shot backlog-to-queue sync for `agent-doc queue sync`.

use anyhow::{Context, Result, bail};
use std::path::Path;

use agent_doc_element::element;
use agent_doc_queue::backlog_sync;
use agent_doc_queue::document_queue::{self, BacklogQueueSyncMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneShotQueueSyncApplied {
    pub requested_count: usize,
    pub prompt_count: usize,
    pub mode: BacklogQueueSyncMode,
    pub already_present: Vec<String>,
    pub newly_materialized: Vec<String>,
    pub snapshot_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneShotQueueSyncResult {
    AlreadyInSync {
        requested_count: usize,
        mode: BacklogQueueSyncMode,
    },
    Synced(OneShotQueueSyncApplied),
}

/// Sync explicitly marked backlog/icebox/pending items into `agent:queue`.
///
/// The caller injects snapshot persistence so this focused IO crate owns the
/// document mutation while orchestration remains responsible for its concrete
/// snapshot logging boundary.
pub fn sync_one_shot_backlog_queue_with_snapshot(
    file: &Path,
    content: &str,
    write_document: impl FnOnce(&Path, &str, &str) -> Result<()>,
    save_snapshot: impl FnOnce(&Path, &str) -> Result<()>,
) -> Result<OneShotQueueSyncResult> {
    let components = element::parse(content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let queue_comp = components.iter().find(|c| c.name == "queue");
    let Some(qc) = queue_comp else {
        bail!(
            "{}: no agent:queue component found. Add `<!-- agent:queue -->..<!-- /agent:queue -->` to the document.",
            file.display()
        );
    };

    let Some(sync_request) =
        backlog_sync::collect_one_shot_backlog_queue_sync(&components, content)
    else {
        bail!(
            "{}: no agent:backlog/agent:icebox/agent:pending component carries a `queue` attribute or enqueue marker. \
             Add `<!-- agent:backlog queue -->` (or `queue=sync`, `queue=prepend`) or mark an item with `:inbox_tray:` / `/enqueue`.",
            file.display()
        );
    };
    let effective_mode = sync_request.mode;
    let ids = sync_request.ids;

    if ids.is_empty() {
        bail!(
            "{}: no active backlog items found to sync. Add `[ ] [#id] ...` items to agent:backlog first.",
            file.display()
        );
    }

    let body = &content[qc.open_end..qc.close_start];
    let entries = document_queue::parse(body)
        .with_context(|| format!("failed to parse queue body in {}", file.display()))?;

    let Some(synced) = document_queue::sync_backlog_into_queue(&entries, &ids, effective_mode)
    else {
        return Ok(OneShotQueueSyncResult::AlreadyInSync {
            requested_count: ids.len(),
            mode: effective_mode,
        });
    };

    let new_body = document_queue::render(&synced);
    let new_content = qc.replace_content(content, &new_body);

    write_document(file, content, &new_content)
        .with_context(|| format!("failed to write {}", file.display()))?;

    let report = backlog_sync::backlog_queue_sync_report(&entries, &ids, &synced);
    let snapshot_warning = save_snapshot(file, &new_content)
        .err()
        .map(|err| err.to_string());

    Ok(OneShotQueueSyncResult::Synced(OneShotQueueSyncApplied {
        requested_count: ids.len(),
        prompt_count: report.prompt_count,
        mode: effective_mode,
        already_present: report.already_present,
        newly_materialized: report.newly_materialized,
        snapshot_warning,
    }))
}

pub fn format_queue_ids(ids: &[String]) -> String {
    backlog_sync::format_queue_ids(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn sync_accepts_enqueue_marker_without_queue_attr() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] :inbox_tray: add me\n",
            "- [ ] [#beta] leave me alone\n",
            "- [/] [#gated] /enqueue blocked\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let snapshot_called = Rc::new(Cell::new(false));
        let snapshot_called_for_save = snapshot_called.clone();

        let result = sync_one_shot_backlog_queue_with_snapshot(
            &doc,
            content,
            |path, _current, target| {
                std::fs::write(path, target)?;
                Ok(())
            },
            move |_path, new_content| {
                snapshot_called_for_save.set(true);
                assert!(new_content.contains("- do [#alpha]"));
                Ok(())
            },
        )
        .expect("enqueue marker should append to queue");

        let OneShotQueueSyncResult::Synced(applied) = result else {
            panic!("expected sync to materialize one queue prompt");
        };
        assert_eq!(applied.requested_count, 1);
        assert_eq!(applied.prompt_count, 1);
        assert_eq!(applied.newly_materialized, vec!["alpha".to_string()]);
        assert!(snapshot_called.get());

        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(
            written.contains("- do [#alpha]"),
            "marked item should be queued:\n{written}"
        );
        assert!(
            !written.contains("- do [#beta]"),
            "unmarked item must not be queued:\n{written}"
        );
        assert!(
            !written.contains("- do [#gated]"),
            "gated marker must not be queued:\n{written}"
        );
    }

    #[test]
    fn sync_reports_already_in_sync_without_snapshot_write() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue -->\n",
            "- [ ] [#alpha] already queued\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();

        let result = sync_one_shot_backlog_queue_with_snapshot(
            &doc,
            content,
            |_path, _current, _target| panic!("already-in-sync should not write document"),
            |_path, _content| panic!("already-in-sync should not save snapshot"),
        )
        .expect("already-synced queue should be accepted");

        assert_eq!(
            result,
            OneShotQueueSyncResult::AlreadyInSync {
                requested_count: 1,
                mode: BacklogQueueSyncMode::Append
            }
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }
}
