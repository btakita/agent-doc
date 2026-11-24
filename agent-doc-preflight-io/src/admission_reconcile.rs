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

/// Test-only: also run [`agent_doc_queue::reconcile_incremental::reconcile_full`]
/// beside every chain pass and log whether the two results are byte-identical
/// (`full_equal=`). Read by the process running the pass (the controller for
/// controller-hosted maintenance).
const VERIFY_FULL_CHAIN_ENV: &str = "AGENT_DOC_TEST_RECONCILE_CHAIN_VERIFY_FULL";

/// One document's chain state (`#incrementalreconcile`): its
/// [`ReconcileCache`] and the controller canonical's text revision the
/// cached revision was, when it was the canonical text.
///
/// [`ReconcileCache`]: agent_doc_queue::reconcile_incremental::ReconcileCache
#[derive(Default)]
struct ChainState {
    cache: agent_doc_queue::reconcile_incremental::ReconcileCache,
    canonical_revision: Option<u64>,
}

/// One [`ChainState`] per document (`#incrementalreconcile`). The
/// controller's background reconciler is long-lived, so a document's cache
/// survives between its maintenance runs there; a detached worker process
/// starts empty, which is the same chain without reuse.
fn chain_caches()
-> &'static std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, ChainState>> {
    static CACHES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, ChainState>>,
    > = std::sync::OnceLock::new();
    CACHES.get_or_init(Default::default)
}

fn chain_cache_key(file: &Path) -> std::path::PathBuf {
    file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
}

/// How the chain found this revision's changeset (`crdt=` on
/// `queue_reconcile_chain`).
fn crdt_changeset_state(
    canonical: Option<&agent_doc_crdt_relay_io::CanonicalChangeset>,
    is_canonical: bool,
    previous: Option<u64>,
) -> &'static str {
    match (canonical, is_canonical, previous) {
        (None, _, _) => "no_hub",
        (Some(_), false, _) => "not_canonical",
        (Some(_), true, None) => "anchored",
        (Some(changeset), true, Some(_)) if changeset.splices.is_none() => "journal_gap",
        (Some(_), true, Some(_)) => "journal",
    }
}

/// `#incrementalreconcile`: the queue/closeout chain (moved follow-up states,
/// self-repair, response-tree sync, marker legend, open counts, loose ends)
/// through [`agent_doc_queue::reconcile_incremental::reconcile_incremental`],
/// keeping one cache per document. The queue maintenance after this step runs
/// the same passes again inside its own pass order; on the chain's output they
/// find nothing left to change.
///
/// In the controller the revision's changeset comes from the canonical's CRDT
/// text journal: when `content` is the canonical text (the fence's admitted
/// hash is the canonical snapshot's memoized hash) and the cached revision
/// was the canonical text at an earlier revision, the canonical's splices
/// since then are the changeset, so the chain never compares the texts
/// through `agent:exchange` to find what changed. Otherwise (no hub here, an
/// edit after the cut, a journal gap) the chain compares the texts.
fn queue_reconcile_chain(
    file: &Path,
    content: &str,
    fence: &dyn PendingMaintenanceEffectFence,
) -> Result<Option<String>> {
    let key = chain_cache_key(file);
    let mut state = chain_caches()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .remove(&key)
        .unwrap_or_default();
    let started = std::time::Instant::now();
    let canonical =
        agent_doc_crdt_relay_io::canonical_changeset_for_file(file, state.canonical_revision);
    let is_canonical = canonical.as_ref().is_some_and(|canonical| {
        canonical.snapshot.len() == content.len()
            && fence
                .expected_authority_hash()
                .is_some_and(|admitted| admitted == canonical.snapshot.content_hash())
    });
    let crdt = crdt_changeset_state(canonical.as_ref(), is_canonical, state.canonical_revision);
    let splices = canonical
        .as_ref()
        .filter(|_| is_canonical && state.canonical_revision.is_some())
        .and_then(|canonical| canonical.splices.as_ref())
        .map(|splices| {
            splices
                .iter()
                .map(
                    |splice| agent_doc_queue::reconcile_incremental::TextSplice {
                        start: splice.start,
                        removed_len: splice.removed_len,
                        inserted: splice.inserted.clone(),
                    },
                )
                .collect::<Vec<_>>()
        });
    let report = match &splices {
        Some(splices) => agent_doc_queue_io::self_repair::reconcile_chain_changeset_for(
            file,
            splices,
            content,
            &mut state.cache,
        ),
        None => {
            agent_doc_queue_io::self_repair::reconcile_chain_for(file, content, &mut state.cache)
        }
    };
    state.canonical_revision = canonical
        .as_ref()
        .filter(|_| is_canonical)
        .map(|canonical| canonical.snapshot.revision());
    drop(canonical);
    let elapsed = started.elapsed();
    chain_caches()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(key, state);
    let full_equal = std::env::var_os(VERIFY_FULL_CHAIN_ENV).map(|_| {
        agent_doc_queue_io::self_repair::reconcile_chain_full_for(file, content)
            == report.reconciled
    });
    let reconciled = &report.reconciled;
    let changed = report.changed;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_reconcile_chain file={} changed={changed} reused={} mode={} crdt={crdt} changeset={} changeset_fallback={} exchange_bytes_compared={} chain_bytes={} exchange_bytes_indexed={} rebuilt={} dirty={} slice_parses={} document_parses={} moved={} repairs={} response_trees={} legend={} open_counts={} loose_ends={} elapsed_us={}{} (#incrementalreconcile)",
            file.display(),
            report.reused,
            report.mode.as_str(),
            report.changeset.as_str(),
            report.changeset_fallback.unwrap_or("-"),
            report.exchange_bytes_compared,
            report.chain_bytes,
            report.exchange_bytes_indexed,
            report.rebuilt.unwrap_or("-"),
            if report.dirty_components.is_empty() {
                "-".to_string()
            } else {
                report.dirty_components.len().to_string()
            },
            report.slice_parses,
            report.document_parses,
            reconciled.moved_followup_rows,
            reconciled.repairs.len(),
            reconciled.response_trees.is_some(),
            reconciled.legend_refreshed,
            reconciled.open_counts_changed,
            reconciled.loose_ends_changed,
            elapsed.as_micros(),
            match full_equal {
                Some(equal) => format!(" full_equal={equal}"),
                None => String::new(),
            },
        ),
    );
    if !changed {
        return Ok(None);
    }
    for repair in &reconciled.repairs {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_self_repair file={} source=background_reconcile {} (#selfrepaircleanup)",
                file.display(),
                repair.log_fields()
            ),
        );
    }
    agent_doc_queue_io::self_repair::carry_self_repair_rewrites(file, &reconciled.repairs);
    let root = agent_doc_project_root_io::project_root_containing(file);
    let admitted = agent_doc_hash::content_hash(content);
    let persisted = crate::persist_queue_maintenance_doc_with_admitted_authority(
        file,
        &reconciled.document,
        content,
        root.as_deref(),
        "background_queue_reconcile_chain",
        Some(&admitted),
        Some(fence),
        None,
    )?;
    // The open counts and the loose-ends digest are generated projections the
    // recovery baseline carries too (queue maintenance re-renders them into
    // the snapshot whenever it writes); keep it aligned when this pass is the
    // one that changed them.
    if (reconciled.open_counts_changed || reconciled.loose_ends_changed)
        && let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
    {
        let mut next = snapshot.clone();
        if let Ok(Some(counted)) = agent_doc_queue::open_count::reconcile_open_counts(&next) {
            next = counted;
        }
        if let Ok(Some(digest)) = agent_doc_queue::loose_ends::reconcile_loose_ends_ensuring(
            &next,
            agent_doc_queue::loose_ends::has_loose_ends_component(&persisted),
        ) {
            next = digest;
        }
        if next != snapshot {
            crate::queue_maintenance_checkpoint_snapshot(
                file,
                Some(fence),
                &next,
                "queue_reconcile_chain",
            )?;
        }
    }
    Ok(Some(persisted))
}

/// The passes in the order admission ran them: todo ids are coined before the
/// answer ledger reads todo rows, and moot decisions are pruned before the
/// queue maintenance after this step rebuilds the waiting tree. The queue
/// reconcile chain runs last, on the cut those passes produced.
const PASSES: [(&str, Pass); 5] = [
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
    ("queue_reconcile_chain", queue_reconcile_chain),
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

    /// `#incrementalreconcile`: the chain pass persists exactly
    /// `reconcile_full`'s document, and the per-document cache it keeps makes
    /// the next revision of the same document reuse it.
    #[test]
    fn the_chain_pass_persists_the_full_chain_result_with_a_per_document_cache() {
        let content = format!(
            "{}\n## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            include_str!("../../agent-doc-queue/tests/fixtures/open_count_session.md")
        );
        let (_dir, doc) = fixture(&content);
        let full = agent_doc_queue_io::self_repair::reconcile_chain_full_for(&doc, &content);
        assert_ne!(full.document, content, "the fixture must have chain work");
        let fence = CutFence {
            expected: RefCell::new(agent_doc_hash::content_hash(&content)),
            receipts: RefCell::new(Vec::new()),
            refuse: None,
        };
        let report = run_deferred_admission_reconcile(&doc, &content, &fence).unwrap();
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(
            report.changed.contains(&"queue_reconcile_chain"),
            "{report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            full.document,
            "the persisted chain cut must be byte-identical to reconcile_full"
        );
        assert!(
            chain_caches()
                .lock()
                .unwrap()
                .contains_key(&chain_cache_key(&doc)),
            "the pass keeps one cache per document"
        );

        // The reconciled revision is a fixed point: the next run writes nothing.
        let settled = full.document.clone();
        let fence = CutFence {
            expected: RefCell::new(agent_doc_hash::content_hash(&settled)),
            receipts: RefCell::new(Vec::new()),
            refuse: None,
        };
        let report = run_deferred_admission_reconcile(&doc, &settled, &fence).unwrap();
        assert!(
            !report.changed.contains(&"queue_reconcile_chain"),
            "{report:?}"
        );
        let mut cache = chain_caches()
            .lock()
            .unwrap()
            .remove(&chain_cache_key(&doc))
            .unwrap();
        let replay =
            agent_doc_queue_io::self_repair::reconcile_chain_for(&doc, &settled, &mut cache.cache);
        assert!(
            replay.reused,
            "an unchanged revision replays from the cache"
        );
        assert_eq!(replay.reconciled.document, settled);
    }

    /// `#incrementalreconcile`: with this process hosting the document's live
    /// hub (an attached editor), the chain anchors on the canonical's text
    /// revision, and the next run reconciles from the canonical's CRDT text
    /// journal: no byte of `agent:exchange` is compared.
    #[test]
    fn an_attached_document_reconciles_from_the_canonical_journal() {
        let settled = QUIET_DOC.replace(
            "<!-- agent:exchange -->\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n",
        );
        let (dir, doc) = fixture(&settled);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: agent_doc_hash::document_id_for_path(&doc),
                pid: std::process::id().into(),
                tag: format!("admission-reconcile-journal:{}", doc.display()),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "editor:journal")
            .unwrap()
            .expect("an attached document allocates a hub");
        let canonical =
            agent_doc_crdt_relay_io::existing_hub_canonical_text_for_file(&doc).unwrap();
        assert_eq!(canonical, settled);
        let fence_for = |text: &str| CutFence {
            expected: RefCell::new(agent_doc_hash::content_hash(text)),
            receipts: RefCell::new(Vec::new()),
            refuse: None,
        };
        let chain_lines = || -> Vec<String> {
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.contains("queue_reconcile_chain file="))
                .map(str::to_string)
                .collect()
        };

        assert_eq!(
            queue_reconcile_chain(&doc, &settled, &fence_for(&settled)).unwrap(),
            None
        );
        let first = chain_lines();
        assert!(
            first.last().unwrap().contains(" crdt=anchored "),
            "{first:?}"
        );

        // An agent turn appends a note through the canonical.
        let edited = settled.replace(
            "<!-- /agent:exchange -->",
            "A plain note.\n<!-- /agent:exchange -->",
        );
        agent_doc_crdt_relay_io::apply_cp_write_for_file(&doc, &settled, &edited, "test")
            .unwrap()
            .expect("the canonical takes the write");
        assert_eq!(
            queue_reconcile_chain(&doc, &edited, &fence_for(&edited)).unwrap(),
            None
        );
        let second = chain_lines();
        let line = second.last().unwrap();
        assert!(
            line.contains(" crdt=journal ")
                && line.contains(" changeset=splices ")
                && line.contains(" exchange_bytes_compared=0 ")
                && line.contains(" mode=exchange_only "),
            "{line}"
        );

        // A cut that is not the canonical text compares the texts.
        let elsewhere = edited.replace("A plain note.", "A plain notE.");
        queue_reconcile_chain(&doc, &elsewhere, &fence_for(&elsewhere)).unwrap();
        let third = chain_lines();
        let line = third.last().unwrap();
        assert!(
            line.contains(" crdt=not_canonical ") && line.contains(" changeset=byte_compare "),
            "{line}"
        );
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
