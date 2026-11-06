//! `agent-doc queue repair-completion-cells <FILE> [--apply]`
//! (`#completioncellrestore`): rebuild the binary completion cells a closeout
//! deleted from `agent:exchange` while their attempts were still open, from the
//! worker responses `report-complete` saved in the attempts' receipts.
//!
//! The pure decision lives in
//! [`agent_doc_queue::completion_cell_restore`]. This adapter loads the claim
//! ledger and the receipts for one document cut. The CLI composes the
//! successor (the same exchange insertion `report-complete` uses) and writes
//! it through the editor/CRDT authority under the completion-feedback lock.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_queue::completion_cell_restore::{
    RestoredCompletionCell, SkippedRestore, plan_completion_cell_restore,
};
use anyhow::{Context, Result};

/// What the restore found (dry run) or added (`--apply`) in one document.
#[derive(Debug, Clone, Default)]
pub struct CompletionCellRestoreReport {
    pub file: PathBuf,
    /// Cells to add (dry run) or added (`--apply`), oldest attempt first.
    pub restored: Vec<RestoredCompletionCell>,
    /// Attempts whose attested cell the document already carries.
    pub present: usize,
    /// Attempts with a saved response that get no cell, and why.
    pub skipped: Vec<SkippedRestore>,
    /// Named keys no receipt carries.
    pub missing_keys: Vec<String>,
    /// True when the restored cells were written.
    pub applied: bool,
    /// Retained editor-write intent of the applied successor, when an
    /// attached editor owns its projection.
    pub intent_id: Option<String>,
}

impl CompletionCellRestoreReport {
    pub fn is_clean(&self) -> bool {
        self.restored.is_empty()
    }
}

/// Plan the restore of `content` (a cut of `file`), including the closed
/// attempts named in `named_keys`, without changing anything.
pub fn inspect(
    file: &Path,
    content: &str,
    named_keys: &HashSet<String>,
) -> Result<CompletionCellRestoreReport> {
    let claims = crate::queue_claim::load_ledger_following(file, content)
        .context("read the queue claim ledger")?;
    let receipts =
        crate::claim_release_receipt::load(file).context("read the reported-release receipts")?;
    let plan = plan_completion_cell_restore(content, &claims, &receipts, named_keys).map_err(
        |refusal| {
            anyhow::anyhow!(
                "{}: refusing to restore completion cells: {refusal}; nothing was changed",
                file.display()
            )
        },
    )?;
    Ok(CompletionCellRestoreReport {
        file: file.to_path_buf(),
        restored: plan.restored,
        present: plan.present,
        skipped: plan.skipped,
        missing_keys: plan.missing_keys,
        applied: false,
        intent_id: None,
    })
}

/// `content` with every restored cell inserted into `agent:exchange` the way
/// `report-complete` inserts one (before the last boundary marker, else at the
/// end), in report order.
pub fn compose(content: &str, report: &CompletionCellRestoreReport) -> Result<String> {
    let mut updated = content.to_string();
    for restored in &report.restored {
        updated = agent_doc_element_exchange::insert_prompt_line_before_boundary(
            &updated,
            &restored.cell,
        )
        .context("insert a restored completion cell into agent:exchange")?;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn session(dir: &Path, exchange: &str, queue: &str) -> PathBuf {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("session.md");
        fs::write(
            &doc,
            format!(
                concat!(
                    "---\nagent_doc_session: restore-io\nagent_doc_format: template\n---\n\n",
                    "<!-- agent:exchange patch=append -->\n{exchange}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue subagents -->\n{queue}<!-- /agent:queue -->\n",
                ),
                exchange = exchange,
                queue = queue,
            ),
        )
        .unwrap();
        doc
    }

    /// One attempt claimed, started, response-captured, and reported, as
    /// `report-complete` leaves it; `release` also ends the claim.
    fn reported(doc: &Path, item: &str, owner: &str, response: &str, release: bool) -> String {
        let (_, claimed) = crate::queue_claim::claim(doc, item, owner, 600).unwrap();
        let running = crate::queue_claim::start_attempt(
            doc,
            item,
            owner,
            &format!("/root/{owner}"),
            600,
            claimed.generation,
        )
        .unwrap();
        let key = crate::queue_claim::completion_feedback_key(&running).unwrap();
        crate::claim_release_receipt::capture_response_for_attempt(
            doc,
            item,
            running.generation,
            response,
        )
        .unwrap();
        crate::queue_claim::report_attempt(doc, item, Some(running.generation)).unwrap();
        if release {
            crate::claim_release_receipt::release(
                doc,
                item,
                crate::claim_release_receipt::ReleaseKind::Reported,
            )
            .unwrap();
        }
        key
    }

    #[test]
    fn restores_the_cells_report_complete_wrote_from_the_saved_responses() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session(
            dir.path(),
            "### Re: prior — gpt-5\n\nDone.\n\n",
            "- finish sdk\n- finish dev\n",
        );
        // Claimed oldest-first AND in item order, so the expected order holds
        // whether or not the two claims land in the same clock second.
        let dev = reported(&doc, "finish dev", "subagent:dev", "Dev done.", true);
        let sdk = reported(
            &doc,
            "finish sdk",
            "subagent:sdk",
            "SDK done.\n\n- tests pass",
            false,
        );
        let content = fs::read_to_string(&doc).unwrap();

        let report = inspect(&doc, &content, &HashSet::new()).unwrap();
        assert_eq!(
            report
                .restored
                .iter()
                .map(|cell| cell.key.as_str())
                .collect::<Vec<_>>(),
            [dev.as_str(), sdk.as_str()],
            "{report:#?}"
        );
        let restored = compose(&content, &report).unwrap();
        let expected_sdk = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:sdk",
            "finish sdk",
            &sdk,
            "SDK done.\n\n- tests pass",
        )
        .unwrap();
        assert!(
            restored.contains(&format!(
                "Done.\n\n{}\n\n{expected_sdk}\n<!-- agent:boundary:keep -->",
                report.restored[0].cell
            )),
            "{restored}"
        );
        assert!(restored.contains("- finish sdk\n- finish dev\n"));
        let evidence = agent_doc_document::subagent_completion::completion_evidence(&restored);
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            agent_doc_document::subagent_completion::completion_response(&restored, &dev)
                .as_deref(),
            Some("Dev done.")
        );

        let again = inspect(&doc, &restored, &HashSet::new()).unwrap();
        assert!(again.is_clean(), "{again:#?}");
        assert_eq!(again.present, 2);
    }
}
