//! Lazily-owned effect CAS for deferred queue maintenance (`#implementlazilyowned`).
//!
//! `lazily::EffectCasCore` owns effect identity, the expected authority of each
//! attempt, single-flight claims, supersession, and the monotone applied
//! frontier. This module contributes only the narrow agent-doc backend adapters
//! (`lazily::CompareAndApply`): observing the canonical document authority and
//! executing one atomic compare-and-apply against it. Adapters never retry and
//! never choose a newer intent; a failed comparison is reported to Lazily as
//! `Superseded { observed }`.
//!
//! Two seams are migrated here:
//!
//! - [`QueueMaintenanceEffectPlan`] replaces the bespoke per-family barrier
//!   (`QueueMaintenanceEffectFamily` + `fence_queue_maintenance_effect`) and the
//!   detached worker's "latest own authority cut" state with an ordered Lazily
//!   effect plan. Every barrier is one receipted step whose expected authority
//!   is the previous step's receipt; a receipted document cut makes every later
//!   step expect that cut, never the stale admission hash.
//! - [`persist_document_cut`] replaces the hand-rolled
//!   `for attempt in 1..=QUEUE_MAINTENANCE_CAS_ATTEMPTS` refusal/re-observe loop.
//!   A retryable CAS refusal is `Superseded { observed }`; the maintenance
//!   producer re-desires its delta at the observed authority as the next epoch.
//!
//! Which detached worker owns a request is the cross-process durable claim in
//! `agent_doc_queue_io::queue_maintenance_claim`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasFailure, EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
    EffectTerminal,
};

/// The single actor generation of an in-process effect core. Cross-process
/// ownership is the durable queue-maintenance claim, so these per-request cores
/// never reconnect.
const GENERATION: u64 = 0;

/// Logical clock for claims. These cores never schedule a timed retry: a
/// refused comparison is supersession, not a backoff.
const NOW: u64 = 0;

/// Typed supersession of a Lazily-owned queue-maintenance effect. Its display
/// text keeps the historical wording so operator diagnostics and existing
/// ops-log greps are unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueMaintenanceEffectSuperseded {
    message: String,
}

impl QueueMaintenanceEffectSuperseded {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for QueueMaintenanceEffectSuperseded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for QueueMaintenanceEffectSuperseded {}

/// One step of the ordered queue-maintenance effect plan.
///
/// The detached maintenance owner crosses one step immediately before each
/// durable effect it performs. The step names the effect family the barrier
/// protects; its compare-and-apply proves the canonical document authority
/// still equals the plan's expected authority (the admitted cut, or the last
/// cut this owner receipted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMaintenanceEffectStep {
    /// A plain authority verification between effects.
    Verify,
    /// Ops-log evidence describing an effect.
    Ops,
    /// Controller state or recovery.
    Controller,
    /// A canonical document mutation.
    Document,
    /// Claim/lineage/answer ledger writes.
    Ledger,
    /// Snapshot writes.
    Snapshot,
    /// Worktree or archive cleanup.
    Worktree,
    /// Receipt of a document cut this owner produced: the backend must now
    /// equal the produced cut, which every later step then expects.
    DocumentCut,
}

impl QueueMaintenanceEffectStep {
    /// Stable label used by diagnostics and the test delay barrier.
    pub fn label(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::Ops => "ops",
            Self::Controller => "controller",
            Self::Document => "document",
            Self::Ledger => "ledger",
            Self::Snapshot => "snapshot",
            Self::Worktree => "worktree",
            Self::DocumentCut => "document_cut",
        }
    }
}

/// Receipt of one crossed plan step: which step, at which authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueMaintenanceStepReceipt {
    pub step: QueueMaintenanceEffectStep,
    pub authority: String,
}

type PlanCore =
    EffectCasCore<PathBuf, String, QueueMaintenanceEffectStep, QueueMaintenanceStepReceipt, String>;

/// Lazily-owned ordered effect plan for one admitted queue-maintenance request.
///
/// Each crossed step is one Lazily effect epoch keyed by the document: Lazily
/// owns the step identity, its expected authority (derived from the previous
/// step's receipt, never from the stale admission once a cut is receipted),
/// the single in-flight attempt, the monotone applied frontier (the plan
/// cursor), and supersession. A mismatch is recorded as Lazily supersession
/// before it is reported, and a superseded plan refuses every later step
/// without any application-owned flag. Steps are ordered and receipted, not
/// atomic across backends: each later effect is idempotent and the plan stops
/// at the first supersession.
#[derive(Debug)]
pub struct QueueMaintenanceEffectPlan {
    key: PathBuf,
    identity: String,
    admitted: String,
    core: Mutex<PlanCore>,
}

impl QueueMaintenanceEffectPlan {
    /// Start the plan for one admitted authority cut.
    pub fn admit(file: &Path, admitted_authority_hash: &str) -> Self {
        Self {
            key: file.to_path_buf(),
            identity: format!("queue-maintenance-plan:{admitted_authority_hash}"),
            admitted: admitted_authority_hash.to_string(),
            core: Mutex::new(PlanCore::new(GENERATION)),
        }
    }

    /// The authority the next step expects, or `None` once superseded.
    fn expected(&self, core: &PlanCore) -> Option<String> {
        match core.state(&self.key).and_then(|state| state.terminal) {
            None => Some(self.admitted.clone()),
            Some(EffectTerminal::Applied { receipts, .. }) => receipts
                .last()
                .map(|receipt| receipt.authority.clone())
                .or_else(|| Some(self.admitted.clone())),
            Some(EffectTerminal::Superseded { .. } | EffectTerminal::Rejected { .. }) => None,
        }
    }

    /// The authority this owner currently holds, or `None` once superseded.
    pub fn expected_authority_hash(&self) -> Option<String> {
        let core = self.core.lock().unwrap();
        self.expected(&core)
    }

    /// Lazily's monotone applied frontier: the number of crossed steps.
    pub fn applied_through(&self) -> Option<u64> {
        let core = self.core.lock().unwrap();
        core.state(&self.key)
            .and_then(|state| state.applied_through)
    }

    /// Cross `step` against the observed canonical authority. `observe` is an
    /// effect-free read of the document authority hash.
    pub fn cross(
        &self,
        step: QueueMaintenanceEffectStep,
        observe: &dyn Fn() -> Result<String>,
    ) -> Result<()> {
        let adapter = StepAuthority {
            observe,
            target: None,
            error: RefCell::new(None),
        };
        self.apply(step, &adapter, || {
            "background queue maintenance admitted authority was superseded".to_string()
        })
    }

    /// Receipt a document cut this owner just produced. The backend must now
    /// equal `cut_hash`; every later step expects it instead of the admission.
    pub fn record_document_cut(
        &self,
        cut_hash: String,
        observe: &dyn Fn() -> Result<String>,
    ) -> Result<()> {
        let adapter = StepAuthority {
            observe,
            target: Some(cut_hash),
            error: RefCell::new(None),
        };
        self.apply(QueueMaintenanceEffectStep::DocumentCut, &adapter, || {
            "background queue maintenance applied cut was superseded before receipt".to_string()
        })
    }

    fn apply(
        &self,
        step: QueueMaintenanceEffectStep,
        adapter: &StepAuthority<'_>,
        superseded: impl Fn() -> String,
    ) -> Result<()> {
        let mut core = self.core.lock().unwrap();
        let Some(expected) = self.expected(&core) else {
            return Err(QueueMaintenanceEffectSuperseded::new(format!(
                "{}: the plan was already superseded",
                superseded()
            ))
            .into());
        };
        let epoch = core
            .state(&self.key)
            .and_then(|state| state.applied_through)
            .unwrap_or(0)
            + 1;
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("{}#{epoch}", self.identity))
                .map_err(|error| anyhow::anyhow!("invalid effect identity: {error:?}"))?,
            epoch,
            key: self.key.clone(),
            expected: expected.clone(),
            plan: EffectPlan::single(step),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => anyhow::bail!(
                "queue maintenance plan step {epoch} ({}) was not accepted: {other:?}",
                step.label()
            ),
        }
        let attempt = match core.claim(&self.key, GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => attempt,
            other => anyhow::bail!(
                "queue maintenance plan step {epoch} ({}) was not claimed: {other:?}",
                step.label()
            ),
        };
        let outcome = adapter.compare_and_apply(&attempt);
        let observed = match &outcome {
            EffectApply::Superseded { observed } => Some(observed.clone()),
            _ => None,
        };
        match core
            .record_outcome(&self.key, &attempt.identity, outcome, NOW)
            .1
        {
            EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }) => Ok(()),
            EffectCasOutcome::Failure(EffectCasFailure::Superseded) => {
                Err(QueueMaintenanceEffectSuperseded::new(format!(
                    "{}: expected={expected} observed={} step={}",
                    superseded(),
                    observed.unwrap_or_default(),
                    step.label()
                ))
                .into())
            }
            recorded => Err(adapter.take_error().unwrap_or_else(|| {
                anyhow::anyhow!(
                    "queue maintenance plan step {} failed: {recorded:?}",
                    step.label()
                )
            })),
        }
    }
}

/// Adapter: compare the observed canonical document authority with the step's
/// expected authority (or, for a produced cut, with that cut). Observation is
/// effect-free; a mismatch mutates nothing and reports the observed authority.
struct StepAuthority<'a> {
    observe: &'a dyn Fn() -> Result<String>,
    target: Option<String>,
    error: RefCell<Option<anyhow::Error>>,
}

impl StepAuthority<'_> {
    fn take_error(&self) -> Option<anyhow::Error> {
        self.error.borrow_mut().take()
    }
}

impl CompareAndApply<PathBuf, String, QueueMaintenanceEffectStep, QueueMaintenanceStepReceipt>
    for StepAuthority<'_>
{
    type Error = String;

    fn observe(&self, _key: &PathBuf) -> Result<String, String> {
        (self.observe)().map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<
            PathBuf,
            String,
            QueueMaintenanceEffectStep,
            QueueMaintenanceStepReceipt,
        >,
    ) -> EffectApply<QueueMaintenanceStepReceipt, String, String> {
        let wanted = self.target.as_ref().unwrap_or(&attempt.expected);
        match (self.observe)() {
            Ok(observed) if &observed == wanted => EffectApply::Applied {
                receipt: QueueMaintenanceStepReceipt {
                    step: attempt.payload,
                    authority: observed.clone(),
                },
                authority: observed,
            },
            Ok(observed) => EffectApply::Superseded { observed },
            Err(error) => {
                let rendered = format!("{error:#}");
                *self.error.borrow_mut() = Some(error);
                EffectApply::Rejected(rendered)
            }
        }
    }
}

/// Payload of one document-cut CAS effect: the maintenance successor computed
/// against `base`. Each epoch rebases it over its Lazily-owned expected cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentCutTarget {
    pub base: String,
    pub content: String,
}

type DocumentCasCore = EffectCasCore<PathBuf, String, DocumentCutTarget, String, String>;

/// Backend operations for one attached-editor document CAS. Each callback is a
/// single backend action; none may retry.
pub(crate) trait DocumentCasBackend {
    /// Rebase the maintenance successor over the expected authority cut.
    fn rebase(&mut self, target: &DocumentCutTarget, expected: &str) -> Result<String>;
    /// Atomically replace `expected` with `rebased`; `Ok(false)` = not applied.
    fn compare_and_swap(&mut self, expected: &str, rebased: &str) -> Result<bool>;
    /// Whether a CAS error is an authority mismatch (the backend advanced).
    fn is_authority_mismatch(&self, error: &anyhow::Error) -> bool;
    /// Fenced diagnostic before re-observing a refused comparison.
    fn record_refusal(
        &mut self,
        epoch: u64,
        max_epochs: u64,
        expected: &str,
        rebased: &str,
    ) -> Result<()>;
    /// Effect-free observation of the current authority cut.
    fn observe(&mut self) -> Result<String>;
    fn not_applied_error(&self) -> anyhow::Error;
}

/// `lazily::CompareAndApply` adapter over a [`DocumentCasBackend`].
struct DocumentCas<'b, B: DocumentCasBackend> {
    backend: RefCell<&'b mut B>,
    max_epochs: u64,
    error: RefCell<Option<anyhow::Error>>,
}

impl<B: DocumentCasBackend> DocumentCas<'_, B> {
    fn reject(&self, error: anyhow::Error) -> EffectApply<String, String, String> {
        let rendered = format!("{error:#}");
        *self.error.borrow_mut() = Some(error);
        EffectApply::Rejected(rendered)
    }
}

impl<B: DocumentCasBackend> CompareAndApply<PathBuf, String, DocumentCutTarget, String>
    for DocumentCas<'_, B>
{
    type Error = String;

    fn observe(&self, _key: &PathBuf) -> Result<String, String> {
        self.backend
            .borrow_mut()
            .observe()
            .map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<PathBuf, String, DocumentCutTarget, String>,
    ) -> EffectApply<String, String, String> {
        let mut backend = self.backend.borrow_mut();
        let rebased = match backend.rebase(&attempt.payload, &attempt.expected) {
            Ok(rebased) => rebased,
            Err(error) => return self.reject(error),
        };
        match backend.compare_and_swap(&attempt.expected, &rebased) {
            Ok(true) => EffectApply::Applied {
                receipt: rebased.clone(),
                authority: rebased,
            },
            Ok(false) => {
                let error = backend.not_applied_error();
                self.reject(error)
            }
            // The final permitted epoch reports the refusal itself: no
            // diagnostic and no re-observation for a desire that cannot recur.
            Err(error)
                if attempt.identity.epoch < self.max_epochs
                    && backend.is_authority_mismatch(&error) =>
            {
                if let Err(fenced) = backend.record_refusal(
                    attempt.identity.epoch,
                    self.max_epochs,
                    &attempt.expected,
                    &rebased,
                ) {
                    return self.reject(fenced);
                }
                match backend.observe() {
                    Ok(observed) => EffectApply::Superseded { observed },
                    Err(observe_error) => self.reject(observe_error),
                }
            }
            Err(error) => self.reject(error),
        }
    }
}

/// Persist one queue-maintenance document cut through Lazily effect CAS.
///
/// Lazily owns each attempt's identity, expected authority, single-flight claim,
/// and supersession. The maintenance producer still wants its delta after a
/// supersession, so it re-desires that delta at the observed authority as the
/// next epoch, bounded by `max_epochs`. Returns the receipted cut.
pub(crate) fn persist_document_cut<B: DocumentCasBackend>(
    file: &Path,
    target: DocumentCutTarget,
    initial_current: String,
    max_epochs: u64,
    backend: &mut B,
) -> Result<String> {
    let key = file.to_path_buf();
    let identity = format!(
        "queue-maintenance-document-cas:{}",
        agent_doc_hash::content_hash(&target.content)
    );
    let adapter = DocumentCas {
        backend: RefCell::new(backend),
        max_epochs: max_epochs.max(1),
        error: RefCell::new(None),
    };
    let mut core = DocumentCasCore::new(GENERATION);
    let mut epoch = 1;
    let mut expected = initial_current;
    loop {
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("{identity}#{epoch}"))
                .map_err(|error| anyhow::anyhow!("invalid effect identity: {error:?}"))?,
            epoch,
            key: key.clone(),
            expected,
            plan: EffectPlan::single(target.clone()),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => anyhow::bail!(
                "queue maintenance document CAS epoch {epoch} was not accepted: {other:?}"
            ),
        }
        let attempt = match core.claim(&key, GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => attempt,
            other => anyhow::bail!(
                "queue maintenance document CAS epoch {epoch} was not claimed: {other:?}"
            ),
        };
        let outcome = adapter.compare_and_apply(&attempt);
        let applied = match &outcome {
            EffectApply::Applied { receipt, .. } => Some(receipt.clone()),
            _ => None,
        };
        let observed = match &outcome {
            EffectApply::Superseded { observed } => Some(observed.clone()),
            _ => None,
        };
        let (_, recorded) = core.record_outcome(&key, &attempt.identity, outcome, NOW);
        match (applied, observed, recorded) {
            (Some(receipt), _, EffectCasOutcome::Applied(EffectCasApplied::Completed { .. })) => {
                return Ok(receipt);
            }
            (_, Some(observed), EffectCasOutcome::Failure(EffectCasFailure::Superseded)) => {
                epoch += 1;
                expected = observed;
            }
            (_, _, EffectCasOutcome::Failure(EffectCasFailure::Rejected)) => {
                return Err(adapter.error.borrow_mut().take().unwrap_or_else(|| {
                    anyhow::anyhow!("queue maintenance document CAS was rejected")
                }));
            }
            (_, _, recorded) => {
                anyhow::bail!(
                    "queue maintenance document CAS outcome diverged from Lazily: {recorded:?}"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScriptedBackend {
        refusals: usize,
        observed: Vec<String>,
        swaps: Vec<(String, String)>,
        refusal_log: Vec<u64>,
        fence_fails: bool,
    }

    impl DocumentCasBackend for ScriptedBackend {
        fn rebase(&mut self, target: &DocumentCutTarget, expected: &str) -> Result<String> {
            Ok(format!("{}|on:{expected}", target.content))
        }
        fn compare_and_swap(&mut self, expected: &str, rebased: &str) -> Result<bool> {
            self.swaps.push((expected.to_string(), rebased.to_string()));
            if self.refusals > 0 {
                self.refusals -= 1;
                anyhow::bail!("CAS refused recovery=retry_crdt_merge");
            }
            Ok(true)
        }
        fn is_authority_mismatch(&self, error: &anyhow::Error) -> bool {
            format!("{error:#}").contains("recovery=retry_crdt_merge")
        }
        fn record_refusal(&mut self, epoch: u64, _: u64, _: &str, _: &str) -> Result<()> {
            anyhow::ensure!(!self.fence_fails, "injected authority superseded");
            self.refusal_log.push(epoch);
            Ok(())
        }
        fn observe(&mut self) -> Result<String> {
            Ok(self.observed.remove(0))
        }
        fn not_applied_error(&self) -> anyhow::Error {
            anyhow::anyhow!("not applied")
        }
    }

    fn backend(refusals: usize) -> ScriptedBackend {
        ScriptedBackend {
            refusals,
            observed: vec!["cut-2".into(), "cut-3".into(), "cut-4".into()],
            swaps: Vec::new(),
            refusal_log: Vec::new(),
            fence_fails: false,
        }
    }

    fn target() -> DocumentCutTarget {
        DocumentCutTarget {
            base: "cut-1".into(),
            content: "delta".into(),
        }
    }

    #[test]
    fn refusal_is_lazily_supersession_and_next_epoch_expects_observed_cut() {
        let mut backend = backend(1);
        let written = persist_document_cut(
            Path::new("doc.md"),
            target(),
            "cut-1".into(),
            3,
            &mut backend,
        )
        .unwrap();
        assert_eq!(written, "delta|on:cut-2");
        assert_eq!(
            backend.swaps,
            vec![
                ("cut-1".to_string(), "delta|on:cut-1".to_string()),
                ("cut-2".to_string(), "delta|on:cut-2".to_string()),
            ]
        );
        assert_eq!(backend.refusal_log, vec![1]);
    }

    #[test]
    fn final_epoch_reports_refusal_without_diagnostic_or_reobserve() {
        let mut backend = backend(5);
        let error = persist_document_cut(
            Path::new("doc.md"),
            target(),
            "cut-1".into(),
            3,
            &mut backend,
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("retry_crdt_merge"),
            "{error:#}"
        );
        assert_eq!(backend.swaps.len(), 3);
        assert_eq!(backend.refusal_log, vec![1, 2]);
        assert_eq!(backend.observed, vec!["cut-4".to_string()]);
    }

    #[test]
    fn fenced_refusal_diagnostic_rejects_without_reobserve() {
        let mut backend = backend(1);
        backend.fence_fails = true;
        let error = persist_document_cut(
            Path::new("doc.md"),
            target(),
            "cut-1".into(),
            3,
            &mut backend,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("superseded"), "{error:#}");
        assert_eq!(
            backend.observed.len(),
            3,
            "a stale owner must not reobserve"
        );
        assert_eq!(backend.swaps.len(), 1);
    }

    use std::cell::Cell;

    fn authority(values: &[&str]) -> impl Fn() -> Result<String> + use<> {
        let values: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        let index = Cell::new(0usize);
        move || {
            let at = index.get().min(values.len() - 1);
            index.set(index.get() + 1);
            Ok(values[at].clone())
        }
    }

    const FAMILIES: [QueueMaintenanceEffectStep; 6] = [
        QueueMaintenanceEffectStep::Ops,
        QueueMaintenanceEffectStep::Controller,
        QueueMaintenanceEffectStep::Document,
        QueueMaintenanceEffectStep::Ledger,
        QueueMaintenanceEffectStep::Snapshot,
        QueueMaintenanceEffectStep::Worktree,
    ];

    #[test]
    fn plan_steps_are_ordered_receipted_epochs_against_the_admission() {
        let plan = QueueMaintenanceEffectPlan::admit(Path::new("doc.md"), "h-admitted");
        let observe = authority(&["h-admitted"]);
        for (index, step) in FAMILIES.into_iter().enumerate() {
            plan.cross(step, &observe).unwrap();
            assert_eq!(plan.applied_through(), Some(index as u64 + 1));
        }
        assert_eq!(
            plan.expected_authority_hash().as_deref(),
            Some("h-admitted")
        );
        let core = plan.core.lock().unwrap();
        let Some(EffectTerminal::Applied {
            identity, receipts, ..
        }) = core.state(&plan.key).and_then(|state| state.terminal)
        else {
            panic!("the last crossed step is an applied terminal");
        };
        assert_eq!(identity.as_str(), "queue-maintenance-plan:h-admitted#6");
        assert_eq!(receipts.len(), 1);
        assert_eq!(
            receipts[0].receipt.step,
            QueueMaintenanceEffectStep::Worktree
        );
    }

    /// Authority drift at each of the six effect-family barriers supersedes the
    /// plan at that step: no later step (of any family) can be crossed and the
    /// cursor stops at the last receipted step.
    #[test]
    fn drift_at_every_family_barrier_supersedes_and_stops_the_plan() {
        for (drift_at, family) in FAMILIES.into_iter().enumerate() {
            let plan = QueueMaintenanceEffectPlan::admit(Path::new("doc.md"), "h-admitted");
            let admitted = authority(&["h-admitted"]);
            for step in FAMILIES.into_iter().take(drift_at) {
                plan.cross(step, &admitted).unwrap();
            }
            let drifted = authority(&["h-operator"]);
            let error = plan.cross(family, &drifted).unwrap_err();
            assert!(
                error
                    .downcast_ref::<QueueMaintenanceEffectSuperseded>()
                    .is_some(),
                "{family:?}: {error:#}"
            );
            let message = error.to_string();
            assert!(
                message.contains("authority was superseded")
                    && message.contains("expected=h-admitted observed=h-operator")
                    && message.contains(family.label()),
                "{message}"
            );
            assert_eq!(
                plan.applied_through(),
                (drift_at > 0).then_some(drift_at as u64)
            );
            assert_eq!(plan.expected_authority_hash(), None);
            for later in FAMILIES {
                assert!(
                    plan.cross(later, &admitted).is_err(),
                    "{later:?} crossed after supersession at {family:?}"
                );
            }
            assert_eq!(
                plan.applied_through(),
                (drift_at > 0).then_some(drift_at as u64)
            );
        }
    }

    #[test]
    fn receipted_document_cut_becomes_the_expected_authority() {
        let plan = QueueMaintenanceEffectPlan::admit(Path::new("doc.md"), "h-admitted");
        plan.cross(
            QueueMaintenanceEffectStep::Ledger,
            &authority(&["h-admitted"]),
        )
        .unwrap();
        plan.record_document_cut("h-applied".into(), &authority(&["h-applied"]))
            .unwrap();
        assert_eq!(plan.expected_authority_hash().as_deref(), Some("h-applied"));
        assert_eq!(plan.applied_through(), Some(2));
        plan.cross(
            QueueMaintenanceEffectStep::Snapshot,
            &authority(&["h-applied"]),
        )
        .unwrap();
        // The stale admission no longer satisfies a later step.
        assert!(
            plan.cross(QueueMaintenanceEffectStep::Ops, &authority(&["h-admitted"]))
                .is_err()
        );
    }

    #[test]
    fn document_cut_receipt_mismatch_supersedes_without_advancing_frontier() {
        let plan = QueueMaintenanceEffectPlan::admit(Path::new("doc.md"), "h-admitted");
        let error = plan
            .record_document_cut("h-applied".into(), &authority(&["h-operator"]))
            .unwrap_err();
        assert!(
            error.to_string().contains("superseded before receipt"),
            "{error}"
        );
        assert_eq!(plan.applied_through(), None);
        assert_eq!(plan.expected_authority_hash(), None);
    }

    #[test]
    fn observation_failure_is_rejected_not_superseded() {
        let plan = QueueMaintenanceEffectPlan::admit(Path::new("doc.md"), "h-admitted");
        let failing = || -> Result<String> { anyhow::bail!("editor authority unavailable") };
        let error = plan
            .cross(QueueMaintenanceEffectStep::Document, &failing)
            .unwrap_err();
        assert!(
            error
                .downcast_ref::<QueueMaintenanceEffectSuperseded>()
                .is_none()
        );
        assert!(error.to_string().contains("editor authority unavailable"));
        assert_eq!(
            plan.expected_authority_hash(),
            None,
            "rejection is terminal"
        );
    }
}
