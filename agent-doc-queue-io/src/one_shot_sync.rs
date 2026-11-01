//! File-backed one-shot backlog-to-queue sync for `agent-doc queue sync`.

use anyhow::{Context, Result, bail};
use std::path::Path;

use agent_doc_element::element;
use agent_doc_queue::backlog_sync;
use agent_doc_queue::backlog_sync::BacklogQueuePlacementDebt;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitBacklogQueueRoutingSync {
    pub changed: bool,
    pub newly_materialized: Vec<String>,
    pub placement_debt: Vec<BacklogQueuePlacementDebt>,
    pub snapshot_warning: Option<String>,
}

/// Apply explicit `queue=<id>` backlog routes before the legacy mirror pass.
///
/// This focused I/O boundary owns the structural projection and its write so
/// queue command orchestration never reimplements backlog-to-queue mutation.
pub fn sync_explicit_backlog_queue_routes_with_snapshot(
    file: &Path,
    content: &str,
    write_document: impl FnOnce(&Path, &str, &str) -> Result<()>,
    save_snapshot: impl FnOnce(&Path, &str) -> Result<()>,
) -> Result<ExplicitBacklogQueueRoutingSync> {
    let components = element::parse(content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let projection = backlog_sync::project_explicit_backlog_queue_routes(&components, content)?;
    let changed = projection.content.is_some();
    let snapshot_warning = if let Some(projected) = projection.content {
        write_document(file, content, &projected)
            .with_context(|| format!("failed to write {}", file.display()))?;
        save_snapshot(file, &projected)
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    Ok(ExplicitBacklogQueueRoutingSync {
        changed,
        newly_materialized: projection.newly_materialized,
        placement_debt: projection.placement_debt,
        snapshot_warning,
    })
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
    sync_configured_backlog_queue_with_snapshot(file, content, write_document, save_snapshot)?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{}: no agent:backlog/agent:icebox/agent:pending component carries a `queue` attribute or enqueue marker. \
                 Add `<!-- agent:backlog queue -->` (or `queue=sync`, `queue=prepend`) or mark an item with `:inbox_tray:` / `/enqueue`.",
                file.display()
            )
        })
}

/// Sync a configured backlog mirror, or report that this document does not
/// opt into backlog-to-queue mirroring.
///
/// Declarative document reconciliation calls this after its other queue
/// projections. A document without a `queue` attribute or enqueue marker is
/// already in sync for this projection; it is not a command error.
pub fn sync_configured_backlog_queue_with_snapshot(
    file: &Path,
    content: &str,
    write_document: impl FnOnce(&Path, &str, &str) -> Result<()>,
    save_snapshot: impl FnOnce(&Path, &str) -> Result<()>,
) -> Result<Option<OneShotQueueSyncResult>> {
    let components = element::parse(content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let Some(sync_request) =
        backlog_sync::collect_one_shot_backlog_queue_sync(&components, content)
    else {
        return Ok(None);
    };

    let Some(qc) = backlog_sync::mirror_queue_component(&components) else {
        bail!(
            "{}: no agent:queue component found. Add `<!-- agent:queue -->..<!-- /agent:queue -->` to the document.",
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

    let sibling_ids = backlog_sync::sibling_run_queue_reference_ids(
        &components,
        content,
        qc.open_start,
    )?;
    let synced = document_queue::sync_backlog_into_queue_with_external_ids(
        &entries,
        &ids,
        effective_mode,
        &sibling_ids,
    );
    let new_body = synced.as_ref().map_or_else(
        || {
            document_queue::normalize_top_level_navigation_headings(body)
                .unwrap_or_else(|| body.to_string())
        },
        |entries| document_queue::render(entries),
    );
    if new_body == body {
        return Ok(Some(OneShotQueueSyncResult::AlreadyInSync {
            requested_count: ids.len(),
            mode: effective_mode,
        }));
    }

    let new_content = qc.replace_content(content, &new_body);

    write_document(file, content, &new_content)
        .with_context(|| format!("failed to write {}", file.display()))?;

    let report_entries = synced.as_ref().unwrap_or(&entries);
    let report = backlog_sync::backlog_queue_sync_report(&entries, &ids, report_entries);
    let snapshot_warning = save_snapshot(file, &new_content)
        .err()
        .map(|err| err.to_string());

    Ok(Some(OneShotQueueSyncResult::Synced(OneShotQueueSyncApplied {
        requested_count: ids.len(),
        prompt_count: report.prompt_count,
        mode: effective_mode,
        already_present: report.already_present,
        newly_materialized: report.newly_materialized,
        snapshot_warning,
    })))
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
    fn configured_sync_is_a_noop_when_document_has_no_mirror_directive() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- keep this manual prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] not mirrored\n",
            "<!-- /agent:backlog -->\n",
        );

        let result = sync_configured_backlog_queue_with_snapshot(
            &doc,
            content,
            |_path, _current, _target| panic!("unconfigured mirror must not write"),
            |_path, _content| panic!("unconfigured mirror must not snapshot"),
        )
        .expect("an unconfigured mirror is already reconciled");

        assert_eq!(result, None);
    }

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
                assert!(new_content.contains("- ### do [#alpha]"));
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
            written.contains("- ### do [#alpha]"),
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
            |path, _current, target| {
                std::fs::write(path, target)?;
                Ok(())
            },
            |_path, content| {
                assert!(content.contains("- ### do [#alpha]\n"));
                Ok(())
            },
        )
        .expect("heading-only queue sync should be applied");

        let OneShotQueueSyncResult::Synced(applied) = result else {
            panic!("missing navigation heading must force a sync write");
        };
        assert_eq!(applied.requested_count, 1);
        assert!(applied.newly_materialized.is_empty());
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("- ### do [#alpha]\n")
        );
    }

    #[test]
    fn sync_recognizes_nested_id_in_sibling_run() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("api.md");
        let content = concat!(
            "<!-- agent:run id=\"main\" -->\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:run id=\"contracts-prs\" -->\n",
            "- https://github.com/haiven-dev/haiven-contracts/pull/170\n",
            "  - do [#closed]\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#closed] Why was this PR closed?\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();

        let result = sync_one_shot_backlog_queue_with_snapshot(
            &doc,
            content,
            |_path, _current, _target| panic!("sibling-owned id must not be duplicated"),
            |_path, _content| panic!("already-in-sync should not save snapshot"),
        )
        .expect("the sibling nested leaf satisfies the mirror");

        assert_eq!(
            result,
            OneShotQueueSyncResult::AlreadyInSync {
                requested_count: 1,
                mode: BacklogQueueSyncMode::Append,
            }
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }
}
