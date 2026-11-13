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
//! - [`AdmittedAuthorityLineage`] replaces the detached worker's bespoke
//!   `Mutex<String>` "latest own authority cut" state. Every authority cut the
//!   worker owns is one Lazily effect epoch; a receipted document cut completes
//!   that epoch and the next epoch's expected authority is the receipt, never
//!   the stale admission hash.
//! - [`persist_document_cut`] replaces the hand-rolled
//!   `for attempt in 1..=QUEUE_MAINTENANCE_CAS_ATTEMPTS` refusal/re-observe loop.
//!   A retryable CAS refusal is `Superseded { observed }`; the maintenance
//!   producer re-desires its delta at the observed authority as the next epoch.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasFailure, EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
};

/// The single actor generation of an in-process effect core. Cross-process
/// fencing stays with the queue-edit lease (a backend lock, not effect
/// orchestration), so the in-process core never reconnects.
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

/// Payload of one admitted authority epoch: the worker owns this cut until a
/// receipted document mutation or a supersession ends the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmittedAuthorityStep {
    OwnAuthorityCut,
}

type LineageCore = EffectCasCore<PathBuf, String, AdmittedAuthorityStep, String, String>;

/// Lazily-owned authority lineage for one detached queue-maintenance request.
///
/// The expected authority hash is read from the Lazily in-flight attempt; a
/// mismatch is recorded as Lazily supersession before it is reported, so every
/// later comparison fails closed without any application-owned flag.
#[derive(Debug)]
pub struct AdmittedAuthorityLineage {
    key: PathBuf,
    identity: String,
    core: Mutex<LineageCore>,
}

impl AdmittedAuthorityLineage {
    /// Admit `admitted_authority_hash` as epoch 1 and claim it.
    pub fn admit(file: &Path, admitted_authority_hash: &str) -> Result<Self> {
        let lineage = Self {
            key: file.to_path_buf(),
            identity: format!("queue-maintenance-authority:{admitted_authority_hash}"),
            core: Mutex::new(LineageCore::new(GENERATION)),
        };
        {
            let mut core = lineage.core.lock().unwrap();
            lineage.desire_and_claim(&mut core, 1, admitted_authority_hash.to_string())?;
        }
        Ok(lineage)
    }

    fn desire_and_claim(&self, core: &mut LineageCore, epoch: u64, expected: String) -> Result<()> {
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("{}#{epoch}", self.identity))
                .map_err(|error| anyhow::anyhow!("invalid effect identity: {error:?}"))?,
            epoch,
            key: self.key.clone(),
            expected,
            plan: EffectPlan::single(AdmittedAuthorityStep::OwnAuthorityCut),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => anyhow::bail!(
                "queue maintenance authority epoch {epoch} was not accepted: {other:?}"
            ),
        }
        match core.claim(&self.key, GENERATION, NOW).1 {
            EffectCasClaim::Claimed(_) => Ok(()),
            other => anyhow::bail!(
                "queue maintenance authority epoch {epoch} was not claimed: {other:?}"
            ),
        }
    }

    fn inflight(
        core: &LineageCore,
        key: &PathBuf,
    ) -> Option<EffectAttempt<PathBuf, String, AdmittedAuthorityStep, String>> {
        core.state(key).and_then(|state| state.inflight)
    }

    /// The authority this owner currently holds, or `None` once superseded.
    pub fn expected_authority_hash(&self) -> Option<String> {
        let core = self.core.lock().unwrap();
        Self::inflight(&core, &self.key).map(|attempt| attempt.expected)
    }

    /// Highest receipted epoch (Lazily's monotone applied frontier).
    pub fn applied_through(&self) -> Option<u64> {
        let core = self.core.lock().unwrap();
        core.state(&self.key)
            .and_then(|state| state.applied_through)
    }

    /// Compare an observed authority against the Lazily-owned expectation.
    pub fn compare_observed(&self, observed_hash: &str) -> Result<()> {
        let mut core = self.core.lock().unwrap();
        let Some(attempt) = Self::inflight(&core, &self.key) else {
            return Err(QueueMaintenanceEffectSuperseded::new(
                "background queue maintenance admitted authority was superseded: no owned authority remains",
            )
            .into());
        };
        if attempt.expected == observed_hash {
            return Ok(());
        }
        let _ = core.record_superseded(&self.key, &attempt.identity, observed_hash.to_string());
        Err(QueueMaintenanceEffectSuperseded::new(format!(
            "background queue maintenance admitted authority was superseded: expected={} observed={observed_hash}",
            attempt.expected
        ))
        .into())
    }

    /// Receipt a document cut produced by this owner. `adapter` performs the
    /// atomic comparison; on success the next epoch expects the receipted cut.
    pub fn record_applied_cut(&self, adapter: &DocumentCutReceipt<'_>) -> Result<()> {
        let mut core = self.core.lock().unwrap();
        let Some(attempt) = Self::inflight(&core, &self.key) else {
            return Err(QueueMaintenanceEffectSuperseded::new(
                "background queue maintenance applied cut was superseded before receipt",
            )
            .into());
        };
        let outcome = adapter.compare_and_apply(&attempt);
        let receipt = match &outcome {
            EffectApply::Applied { receipt, .. } => Some(receipt.clone()),
            _ => None,
        };
        let (_, recorded) = core.record_outcome(&self.key, &attempt.identity, outcome, NOW);
        match (receipt, recorded) {
            (
                Some(receipt),
                EffectCasOutcome::Applied(EffectCasApplied::Completed { applied_through }),
            ) => self.desire_and_claim(&mut core, applied_through + 1, receipt),
            (_, EffectCasOutcome::Failure(EffectCasFailure::Superseded)) => {
                Err(QueueMaintenanceEffectSuperseded::new(
                    "background queue maintenance applied cut was superseded before receipt",
                )
                .into())
            }
            (_, recorded) => Err(adapter.take_error().unwrap_or_else(|| {
                anyhow::anyhow!("queue maintenance applied cut receipt failed: {recorded:?}")
            })),
        }
    }
}

/// Adapter: the canonical document authority must equal the cut this owner
/// just applied. Observation is effect-free.
pub struct DocumentCutReceipt<'a> {
    target_hash: String,
    observe: &'a dyn Fn() -> Result<String>,
    error: RefCell<Option<anyhow::Error>>,
}

impl<'a> DocumentCutReceipt<'a> {
    pub fn new(target_hash: String, observe: &'a dyn Fn() -> Result<String>) -> Self {
        Self {
            target_hash,
            observe,
            error: RefCell::new(None),
        }
    }

    fn take_error(&self) -> Option<anyhow::Error> {
        self.error.borrow_mut().take()
    }
}

impl CompareAndApply<PathBuf, String, AdmittedAuthorityStep, String> for DocumentCutReceipt<'_> {
    type Error = String;

    fn observe(&self, _key: &PathBuf) -> Result<String, String> {
        (self.observe)().map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        _attempt: &EffectAttempt<PathBuf, String, AdmittedAuthorityStep, String>,
    ) -> EffectApply<String, String, String> {
        match (self.observe)() {
            Ok(observed) if observed == self.target_hash => EffectApply::Applied {
                receipt: observed.clone(),
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

    #[test]
    fn lineage_expects_admission_then_receipted_cut() {
        let lineage = AdmittedAuthorityLineage::admit(Path::new("doc.md"), "h-admitted").unwrap();
        assert_eq!(
            lineage.expected_authority_hash().as_deref(),
            Some("h-admitted")
        );
        lineage.compare_observed("h-admitted").unwrap();
        let observe = || Ok("h-applied".to_string());
        lineage
            .record_applied_cut(&DocumentCutReceipt::new("h-applied".into(), &observe))
            .unwrap();
        assert_eq!(
            lineage.expected_authority_hash().as_deref(),
            Some("h-applied")
        );
        assert_eq!(lineage.applied_through(), Some(1));
        lineage.compare_observed("h-applied").unwrap();
        // The stale admission is no longer the owned authority.
        assert!(lineage.compare_observed("h-admitted").is_err());
    }

    #[test]
    fn lineage_supersession_is_sticky_and_typed() {
        let lineage = AdmittedAuthorityLineage::admit(Path::new("doc.md"), "h-admitted").unwrap();
        let error = lineage.compare_observed("h-operator").unwrap_err();
        assert!(
            error
                .downcast_ref::<QueueMaintenanceEffectSuperseded>()
                .is_some()
        );
        assert!(
            error.to_string().contains("authority was superseded"),
            "{error}"
        );
        assert_eq!(lineage.expected_authority_hash(), None);
        // Even the original admission no longer passes after supersession.
        assert!(lineage.compare_observed("h-admitted").is_err());
        let observe = || Ok("h-admitted".to_string());
        assert!(
            lineage
                .record_applied_cut(&DocumentCutReceipt::new("h-admitted".into(), &observe))
                .is_err()
        );
        assert_eq!(lineage.applied_through(), None);
    }

    #[test]
    fn lineage_receipt_mismatch_supersedes_without_advancing_frontier() {
        let lineage = AdmittedAuthorityLineage::admit(Path::new("doc.md"), "h-admitted").unwrap();
        let observe = || Ok("h-operator".to_string());
        let error = lineage
            .record_applied_cut(&DocumentCutReceipt::new("h-applied".into(), &observe))
            .unwrap_err();
        assert!(
            error.to_string().contains("superseded before receipt"),
            "{error}"
        );
        assert_eq!(lineage.applied_through(), None);
        assert_eq!(lineage.expected_authority_hash(), None);
    }
}
