//! One-time repair of stale subagent completion evidence
//! (`#stalecompletionrepair`).
//!
//! `agent-doc queue repair-stale-completion <FILE> [--apply]` finds the binary
//! completion cells and reported-release receipts whose work identity has no
//! live row in any queue of the document and no active claim attempt — the
//! leftovers of strikes made before `#strikeretires`, or made by hand in the
//! editor — and retires them through the strike paths' own retirement
//! functions. The default is a dry run that changes nothing.
//!
//! The pure decision lives in
//! [`agent_doc_queue::stale_completion_evidence`]. This adapter reads the
//! document through the editor/CRDT authority, converges the successor through
//! the same write path as the orphan `queue consume --id` strike (never disk
//! behind an attached editor), mirrors the retirement into the snapshot once
//! the successor is accepted, and only then drops the receipts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_document::subagent_completion::CompletionEvidence;
use agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt;
use agent_doc_queue::stale_completion_evidence::{
    KeptWork, StaleCompletionPlan, kept_work, plan_stale_completion_cells, retire_cells_by_key,
    stale_receipts,
};
use anyhow::{Context, Result};

use crate::queue_consume::{
    QueueConsumeWriteEffects, converge_awaiting_retained_delivery, load_snapshot_recovery_only,
    save_snapshot_recovery_only,
};

const SOURCE: &str = "queue_repair_stale_completion";

/// What the repair found (dry run) or retired (`--apply`) in one document.
#[derive(Debug, Clone, Default)]
pub struct StaleCompletionRepairReport {
    pub file: PathBuf,
    /// Stale completion cells, in document order.
    pub stale_cells: Vec<CompletionEvidence>,
    /// Stale reported-release receipts.
    pub stale_receipts: Vec<ClaimReleaseReceipt>,
    /// Cells kept because their work still has a live row.
    pub kept_live_cells: usize,
    /// Cells kept because their work has a live row, although no claim or
    /// receipt tracks their attempt: a leftover painting a NEW same-text row,
    /// or the only evidence a live row's work finished. Retired only when
    /// named with `--key`.
    pub untracked_live_cells: Vec<CompletionEvidence>,
    /// Cells kept only because an active claim attempt still owns their work.
    pub kept_claimed_cells: usize,
    /// `--key` values no cell carries (already retired, or mistyped).
    pub missing_keys: Vec<String>,
    /// True when the retirement was written.
    pub applied: bool,
    /// Snapshot cells retired alongside the document's.
    pub snapshot_cells_retired: usize,
}

impl StaleCompletionRepairReport {
    pub fn is_clean(&self) -> bool {
        self.stale_cells.is_empty() && self.stale_receipts.is_empty()
    }
}

fn plan_for(
    file: &Path,
    content: &str,
    explicit_keys: &HashSet<String>,
) -> Result<(KeptWork, StaleCompletionPlan)> {
    let claims = crate::queue_claim::load_ledger_following(file, content)
        .context("read the queue claim ledger")?;
    let receipts =
        crate::claim_release_receipt::load(file).context("read the reported-release receipts")?;
    let kept = kept_work(content, &claims).map_err(|refusal| {
        anyhow::anyhow!(
            "{}: refusing to judge completion evidence: {refusal}; nothing was changed",
            file.display()
        )
    })?;
    // Every attempt the durable state still tracks: a claim of any state
    // (whose cell is its exact-attempt evidence) or a reported receipt.
    let tracked_keys = claims
        .claims
        .iter()
        .filter_map(|claim| crate::queue_claim::completion_feedback_key(claim).ok())
        .chain(
            receipts
                .receipts
                .iter()
                .filter_map(|receipt| receipt.completion_key.clone()),
        )
        .collect::<HashSet<_>>();
    let plan = plan_stale_completion_cells(content, &kept, &tracked_keys, explicit_keys).map_err(
        |refusal| {
            anyhow::anyhow!(
                "{}: refusing --key: {refusal}; nothing was changed",
                file.display()
            )
        },
    )?;
    Ok((kept, plan))
}

/// Report the stale evidence of `content` (a cut of `file`) without changing
/// anything.
pub fn inspect(
    file: &Path,
    content: &str,
    explicit_keys: &HashSet<String>,
) -> Result<StaleCompletionRepairReport> {
    let (kept, plan) = plan_for(file, content, explicit_keys)?;
    let receipts =
        crate::claim_release_receipt::load(file).context("read the reported-release receipts")?;
    Ok(StaleCompletionRepairReport {
        file: file.to_path_buf(),
        stale_receipts: stale_receipts(&receipts, &kept)
            .into_iter()
            .cloned()
            .collect(),
        kept_live_cells: plan.kept_live_cells,
        kept_claimed_cells: plan.kept_claimed_cells,
        untracked_live_cells: plan.untracked_live_cells,
        missing_keys: plan.missing_keys,
        stale_cells: plan.stale_cells,
        ..StaleCompletionRepairReport::default()
    })
}

/// Find, and with `apply` retire, the stale completion cells and receipts of
/// `file`, plus the untracked live-row cells named in `explicit_keys`.
/// Idempotent: a second run finds nothing and writes nothing.
pub fn repair(
    file: &Path,
    apply: bool,
    explicit_keys: &HashSet<String>,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<StaleCompletionRepairReport> {
    let content = effects.current_document_content(file, SOURCE)?;
    let mut report = inspect(file, &content, explicit_keys)?;
    if !apply || report.is_clean() {
        return Ok(report);
    }
    let (_, plan) = plan_for(file, &content, explicit_keys)?;
    if !plan.is_empty() {
        converge_awaiting_retained_delivery(
            effects,
            file,
            &plan.content,
            &plan.retired_keys,
            SOURCE,
            || effects.converge_document_or_disk(file, &plan.content, &content, SOURCE),
        )
        .context("repair-stale-completion: failed to write document")?;
        if let Some(snapshot) = load_snapshot_recovery_only(file, "stale completion snapshot sync")
        {
            let retired = retire_cells_by_key(&snapshot, &plan.retired_keys);
            if retired != snapshot {
                report.snapshot_cells_retired =
                    agent_doc_document::subagent_completion::completion_evidence(&snapshot).len()
                        - agent_doc_document::subagent_completion::completion_evidence(&retired)
                            .len();
                save_snapshot_recovery_only(file, &retired, "stale completion snapshot sync");
            }
        }
    }
    // Receipts end only once the document successor is accepted, judged
    // against that successor (`#strikeretires` ordering).
    let (kept_after, _) = plan_for(file, &plan.content, &HashSet::new())?;
    report.stale_receipts =
        crate::claim_release_receipt::retire_without_live_work(file, &kept_after, SOURCE)?;
    report.applied = true;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_stale_completion_repaired file={} cells={} explicit_keys={} receipts={} snapshot_cells={} kept_live={} kept_untracked_live={} kept_claimed={} (#stalecompletionrepair)",
            file.display(),
            report.stale_cells.len(),
            explicit_keys.len(),
            report.stale_receipts.len(),
            report.snapshot_cells_retired,
            report.kept_live_cells,
            report.untracked_live_cells.len(),
            report.kept_claimed_cells,
        ),
    );
    Ok(report)
}

/// One session document of a `--project-root` scan: its report, or why it
/// could not be judged.
pub type ProjectScanEntry = (PathBuf, Result<StaleCompletionRepairReport>);

/// Report-only scan of every session document under `root`, read from disk.
/// Never writes; a document an editor holds may be ahead of its disk copy, so
/// run the per-document command for the authoritative answer.
pub fn scan_project(root: &Path) -> Vec<ProjectScanEntry> {
    agent_doc_fs::project_session_documents(root)
        .into_iter()
        .filter_map(|file| {
            let content = match std::fs::read_to_string(&file) {
                Ok(content) => content,
                Err(err) => {
                    let err = anyhow::Error::new(err).context("read session document");
                    return Some((file, Err(err)));
                }
            };
            if !content.contains(agent_doc_document::subagent_completion::MARKER_PREFIX)
                && crate::claim_release_receipt::load(&file)
                    .map(|ledger| ledger.receipts.is_empty())
                    .unwrap_or(false)
            {
                return None;
            }
            let report = inspect(&file, &content, &HashSet::new());
            Some((file, report))
        })
        .collect()
}

/// A one-line rendering of `text`, cut to `max` characters.
pub fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut = flat.chars().take(max.saturating_sub(1)).collect::<String>();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct DetachedEffects;

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    impl QueueConsumeWriteEffects for DetachedEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(fs::read_to_string(file)?)
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            agent_doc_fs::write_atomic(file, content.as_bytes())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            agent_doc_fs::write_atomic(file, target_content.as_bytes())
        }
    }

    fn shell(exchange: &str, queue: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: stale-repair\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n",
                "## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n",
            ),
            exchange = exchange,
            queue = queue,
        )
    }

    /// A claim reported and released for `item`, then its cell: the state a
    /// pre-`#strikeretires` strike left behind once the row was struck.
    fn reported_cell(doc: &Path, item: &str, owner: &str) -> String {
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
        let cell = agent_doc_document::subagent_completion::cell_with_response(
            &running.owner,
            &running.item_text,
            &crate::queue_claim::completion_feedback_key(&running).unwrap(),
            "Released 0.35.500.",
        )
        .unwrap();
        crate::queue_claim::report_attempt(doc, item, Some(running.generation)).unwrap();
        crate::claim_release_receipt::release(
            doc,
            item,
            crate::claim_release_receipt::ReleaseKind::Reported,
        )
        .unwrap();
        cell
    }

    /// `#stalecompletionrepair`: the live 2026-10-26 shape. A row struck
    /// before `#strikeretires` left its cell and receipt; a later identical
    /// row painted `🔄`. The dry run lists both and changes nothing; `--apply`
    /// retires the stale cell (document and snapshot) and receipt, keeps the
    /// live row's cell and receipt and the operator's exchange text byte for
    /// byte; a rerun is a no-op.
    #[test]
    fn dry_run_then_apply_retires_only_stale_evidence_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let stale_item = "release + publish";
        let live_item = "still open work";
        fs::write(&doc, shell("", &format!("- {stale_item}\n- {live_item}\n"))).unwrap();
        let stale_cell = reported_cell(&doc, stale_item, "subagent:release_publish_patch");
        let live_cell = reported_cell(&doc, live_item, "subagent:live");
        let operator = format!(
            "### Re: release — opus\n\n> **Queue prompt:** {stale_item}\n\nReleased and verified.\n\n\
             ### Re: Subagent completed: `subagent:x` returned work for queue item “{stale_item}” — agent-doc\n\
             Operator note quoting a heading, not a binary cell.\n"
        );
        let content = shell(
            &format!("{stale_cell}\n\n{live_cell}\n\n{operator}"),
            &format!("- ~~{stale_item}~~\n- ☑️ [#subagent:live#1] {live_item}\n"),
        );
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let receipts = || {
            crate::claim_release_receipt::load(&doc)
                .unwrap()
                .receipts
                .into_iter()
                .map(|receipt| receipt.item_text)
                .collect::<Vec<_>>()
        };
        assert_eq!(receipts().len(), 2);

        let dry = repair(&doc, false, &none(), &DetachedEffects).unwrap();
        assert!(!dry.applied);
        assert_eq!(
            dry.stale_cells
                .iter()
                .map(|cell| cell.item.as_str())
                .collect::<Vec<_>>(),
            [stale_item]
        );
        assert_eq!(
            dry.stale_receipts
                .iter()
                .map(|receipt| receipt.item_text.as_str())
                .collect::<Vec<_>>(),
            [stale_item]
        );
        assert_eq!(dry.kept_live_cells, 1);
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "dry run writes nothing"
        );
        assert_eq!(receipts().len(), 2, "dry run keeps every receipt");

        let applied = repair(&doc, true, &none(), &DetachedEffects).unwrap();
        assert!(applied.applied);
        assert_eq!(applied.stale_cells.len(), 1);
        assert_eq!(applied.stale_receipts.len(), 1);
        assert_eq!(applied.snapshot_cells_retired, 1);
        let result = fs::read_to_string(&doc).unwrap();
        assert!(!result.contains(&stale_cell), "{result}");
        assert!(result.contains(&live_cell), "{result}");
        assert!(
            result.contains(&operator),
            "operator text byte for byte:\n{result}"
        );
        assert!(result.contains(&format!("- ~~{stale_item}~~\n")));
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            result,
            "the snapshot retires the same cell"
        );
        assert_eq!(receipts(), [live_item]);

        // A later operator row with the same text inherits nothing.
        let readded = result.replace(
            &format!("- ~~{stale_item}~~\n"),
            &format!("- ~~{stale_item}~~\n- {stale_item}\n"),
        );
        assert_eq!(
            crate::queue_claim::completion_evidence_head_texts(&readded),
            [live_item]
        );

        let rerun = repair(&doc, true, &none(), &DetachedEffects).unwrap();
        assert!(rerun.is_clean(), "{rerun:?}");
        assert!(!rerun.applied);
        assert_eq!(fs::read_to_string(&doc).unwrap(), result);
        assert_eq!(receipts(), [live_item]);
    }

    /// The live 2026-10-26 example: a 52-hour-old `release_publish_patch`
    /// cell paints `🔄` onto a NEW `release + publish` row. A live row keeps
    /// its cell by default (it could be the row's only completion evidence),
    /// so the dry run lists it as untracked and `--apply` alone writes
    /// nothing; named with `--key`, it is retired and the row is plain again.
    #[test]
    fn explicit_key_retires_the_old_attempt_cell_painting_a_new_row() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let item = "release + publish";
        let old = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:release_publish_patch",
            item,
            "old-attempt-key",
            "Released 0.35.500.",
        )
        .unwrap();
        let content = shell(&format!("{old}\n"), &format!("- 🔄 {item}\n"));
        fs::write(&doc, &content).unwrap();
        assert_eq!(
            crate::queue_claim::completion_evidence_head_texts(&content),
            [item]
        );

        let dry = repair(&doc, false, &none(), &DetachedEffects).unwrap();
        assert!(dry.is_clean());
        assert_eq!(dry.untracked_live_cells.len(), 1);
        assert_eq!(dry.untracked_live_cells[0].key, "old-attempt-key");
        let unnamed = repair(&doc, true, &none(), &DetachedEffects).unwrap();
        assert!(!unnamed.applied);
        assert_eq!(fs::read_to_string(&doc).unwrap(), content);

        let named = HashSet::from(["old-attempt-key".to_string()]);
        let applied = repair(&doc, true, &named, &DetachedEffects).unwrap();
        assert!(applied.applied);
        let result = fs::read_to_string(&doc).unwrap();
        assert!(!result.contains(&old), "{result}");
        assert!(
            result.contains(&format!("- 🔄 {item}\n")),
            "the row itself is untouched"
        );
        assert!(crate::queue_claim::completion_evidence_head_texts(&result).is_empty());

        let rerun = repair(&doc, true, &named, &DetachedEffects).unwrap();
        assert!(!rerun.applied && rerun.is_clean());
        assert_eq!(rerun.missing_keys, ["old-attempt-key"]);
        assert_eq!(fs::read_to_string(&doc).unwrap(), result);
    }

    /// A document whose queue cannot be judged is refused, untouched.
    #[test]
    fn refuses_a_document_without_a_queue() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let cell = agent_doc_document::subagent_completion::cell("subagent:a", "x", "k");
        let content = format!(
            "---\nagent_doc_session: x\n---\n\n<!-- agent:exchange -->\n{cell}\n<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, &content).unwrap();
        let err = repair(&doc, true, &none(), &DetachedEffects).unwrap_err();
        assert!(format!("{err:#}").contains("no queue component"), "{err:#}");
        assert_eq!(fs::read_to_string(&doc).unwrap(), content);
    }

    /// The project scan reports only documents carrying stale evidence and
    /// never writes.
    #[test]
    fn project_scan_reports_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let stale = dir.path().join("stale.md");
        let clean = dir.path().join("clean.md");
        let cell = agent_doc_document::subagent_completion::cell("subagent:a", "gone row", "k");
        let stale_content = shell(&format!("{cell}\n"), "- ~~gone row~~\n");
        fs::write(&stale, &stale_content).unwrap();
        fs::write(&clean, shell("", "- open row\n")).unwrap();

        let scan = scan_project(dir.path());
        assert_eq!(scan.len(), 1, "{scan:?}");
        let (file, report) = &scan[0];
        assert_eq!(file, &stale);
        assert_eq!(report.as_ref().unwrap().stale_cells.len(), 1);
        assert_eq!(fs::read_to_string(&stale).unwrap(), stale_content);
    }
}
