//! `#q-e9483473162d`: the document reconcile passes the admission hook no longer
//! runs inline.
//!
//! The operator direction: hooks are quick operations; document reconciliation
//! happens in the background. Preflight admission used to read the document
//! authority and write it back once per pass (todo-id backfill, answer-ledger
//! closures and stamps, moot reported-closeout decisions, external blocker
//! settlement with one `gh` lookup per unsettled upstream), all inside the
//! `UserPromptSubmit` hook budget. On a contended controller each authority
//! read is a full document materialization and each write a new revision, so
//! every pass cost seconds there (observed on a 6.5k-line session document:
//! single admission phases of 10-25s).
//!
//! Admission now derives only what the cycle contract carries (the answered
//! facts, read-only) and leaves the writes to the per-document detached
//! maintenance worker, which already coalesces requests (one latest request
//! row per document), fences them by generation and by the admitted authority
//! hash, and logs every exit. This module is that worker's first step: each
//! pass reads nothing on its own, runs on the admitted cut the fence expects,
//! and persists with a compare-and-swap fenced to that cut, receipting the
//! produced cut so the pending and queue maintenance after it fence against
//! it. An operator edit after admission supersedes the whole request — newer
//! operator edits win — and the next admission re-derives every pass.
//!
//! Every pass is idempotent: a second run over its own output finds nothing to
//! close, stamp, coin, prune, or settle.

use std::path::Path;

use anyhow::Result;

use crate::{FreeTextCoinGate, PendingMaintenanceEffectFence};

/// Which deferred passes changed the document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmissionReconcileReport {
    pub changed: Vec<&'static str>,
    pub failed: Vec<&'static str>,
}

type Pass = fn(&Path, &str, &dyn PendingMaintenanceEffectFence) -> Result<Option<String>>;

fn todo_ids(
    file: &Path,
    content: &str,
    fence: &dyn PendingMaintenanceEffectFence,
) -> Result<Option<String>> {
    // The admitted cut already passed admission's typing debounce, and an edit
    // after it supersedes this request, so only the structural coin rule
    // applies here (the same gate the detached queue maintenance coins with).
    crate::run_todo_id_maintenance_on_admitted_cut(
        file,
        content,
        &FreeTextCoinGate::structural_only(),
        fence,
    )
}

/// The passes in the order admission ran them: todo ids are coined before the
/// answer ledger reads todo rows, and moot decisions are pruned before the
/// queue maintenance after this step rebuilds the waiting tree.
const PASSES: [(&str, Pass); 4] = [
    ("todo_id_backfill", todo_ids),
    (
        "answer_ledger",
        crate::answer_ledger::run_answer_ledger_maintenance_on_admitted_cut,
    ),
    (
        "moot_decision",
        crate::moot_decision::run_moot_decision_maintenance_on_admitted_cut,
    ),
    (
        "blocker_settlement",
        crate::blocker_settlement::run_blocker_settlement_maintenance_on_admitted_cut,
    ),
];

fn is_superseded(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<crate::QueueMaintenanceEffectSuperseded>()
            .is_some()
            || cause.to_string().contains("was superseded")
    })
}

/// Run the deferred admission passes over `admitted`, the cut `fence`
/// expects. A superseded admission returns an error whose message contains
/// `authority was superseded`, which the worker completes as superseded.
/// Any other pass failure is logged and skipped, as it was on the admission
/// path: these passes are best-effort and never block maintenance.
pub fn run_deferred_admission_reconcile(
    file: &Path,
    admitted: &str,
    fence: &dyn PendingMaintenanceEffectFence,
) -> Result<AdmissionReconcileReport> {
    let mut report = AdmissionReconcileReport::default();
    let started = std::time::Instant::now();
    fence.check(file)?;
    if let Some(expected) = fence.expected_authority_hash() {
        let observed = agent_doc_hash::content_hash(admitted);
        if observed != expected {
            return Err(crate::QueueMaintenanceEffectSuperseded::admitted_authority(
                expected, observed,
            )
            .into());
        }
    }
    let mut current = admitted.to_string();
    for (name, pass) in PASSES {
        fence.check(file).map_err(|error| superseded(name, error))?;
        match pass(file, &current, fence) {
            Ok(Some(persisted)) => {
                report.changed.push(name);
                current = persisted;
            }
            Ok(None) => {}
            Err(error) if is_superseded(&error) => return Err(superseded(name, error)),
            Err(error) => {
                report.failed.push(name);
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "admission_reconcile_pass_failed file={} pass={name} error={} (#q-e9483473162d)",
                        file.display(),
                        format!("{error:#}").replace('\n', " | "),
                    ),
                );
                // A failed write may still have landed; the next fence check
                // supersedes the request if the authority moved.
                fence.check(file).map_err(|error| superseded(name, error))?;
            }
        }
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "admission_reconcile_completed file={} changed={} failed={} elapsed_ms={} (#q-e9483473162d)",
            file.display(),
            if report.changed.is_empty() {
                "-".to_string()
            } else {
                report.changed.join(",")
            },
            if report.failed.is_empty() {
                "-".to_string()
            } else {
                report.failed.join(",")
            },
            started.elapsed().as_millis(),
        ),
    );
    Ok(report)
}

fn superseded(pass: &str, error: anyhow::Error) -> anyhow::Error {
    if is_superseded(&error) {
        anyhow::anyhow!(
            "admission reconcile admitted authority was superseded at the {pass} pass: {error:#}"
        )
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueueMaintenanceEffectStep;
    use std::cell::RefCell;

    /// A fence expecting `expected`; it records each document cut the passes
    /// receipt and refuses every crossing of `refuse`.
    struct CutFence {
        expected: RefCell<String>,
        receipts: RefCell<Vec<String>>,
        refuse: Option<QueueMaintenanceEffectStep>,
    }

    impl PendingMaintenanceEffectFence for CutFence {
        fn check(&self, _file: &Path) -> Result<()> {
            Ok(())
        }

        fn apply_step(&self, _file: &Path, step: QueueMaintenanceEffectStep) -> Result<()> {
            if self.refuse == Some(step) {
                return Err(crate::QueueMaintenanceEffectSuperseded::admitted_authority(
                    self.expected.borrow().clone(),
                    "operator-newer".to_string(),
                )
                .into());
            }
            Ok(())
        }

        fn record_applied_content(&self, _file: &Path, content: &str) -> Result<()> {
            *self.expected.borrow_mut() = agent_doc_hash::content_hash(content);
            self.receipts.borrow_mut().push(content.to_string());
            Ok(())
        }

        fn expected_authority_hash(&self) -> Option<String> {
            Some(self.expected.borrow().clone())
        }
    }

    const QUIET_DOC: &str = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:todo -->\n- [ ] [#tdowned] Already has its id\n<!-- /agent:todo -->\n";

    fn fixture(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    /// An admitted cut the authority no longer equals is superseded before any
    /// pass runs: a newer operator edit wins and nothing is written.
    #[test]
    fn a_moved_authority_supersedes_the_reconcile_before_any_pass() {
        let (_dir, doc) = fixture(QUIET_DOC);
        let fence = CutFence {
            expected: RefCell::new("operator-newer-cut".to_string()),
            receipts: RefCell::new(Vec::new()),
            refuse: None,
        };
        let error = run_deferred_admission_reconcile(&doc, QUIET_DOC, &fence).unwrap_err();
        assert!(
            error.to_string().contains("authority was superseded"),
            "the worker completes a superseded request by this phrase: {error:#}"
        );
        assert!(fence.receipts.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), QUIET_DOC);
    }

    /// A settled document is a no-op: the reconcile is idempotent and writes
    /// nothing when no pass has work.
    #[test]
    fn a_settled_document_reconciles_to_itself() {
        let (_dir, doc) = fixture(QUIET_DOC);
        let fence = CutFence {
            expected: RefCell::new(agent_doc_hash::content_hash(QUIET_DOC)),
            receipts: RefCell::new(Vec::new()),
            refuse: None,
        };
        let report = run_deferred_admission_reconcile(&doc, QUIET_DOC, &fence).unwrap();
        assert!(report.changed.is_empty(), "{report:?}");
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(fence.receipts.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), QUIET_DOC);
    }

    /// A refusal at a pass's own fenced write is reported as superseded, not
    /// swallowed as a best-effort pass failure.
    #[test]
    fn a_refused_document_write_supersedes_instead_of_failing_the_pass() {
        let content = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:todo -->\n- [ ] Delete the clean inspection worktrees.\n<!-- /agent:todo -->\n";
        let (_dir, doc) = fixture(content);
        let fence = CutFence {
            expected: RefCell::new(agent_doc_hash::content_hash(content)),
            receipts: RefCell::new(Vec::new()),
            refuse: Some(QueueMaintenanceEffectStep::Document),
        };
        let error = run_deferred_admission_reconcile(&doc, content, &fence).unwrap_err();
        assert!(
            error.to_string().contains("authority was superseded")
                && error.to_string().contains("todo_id_backfill"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }
}
