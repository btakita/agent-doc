//! Durable Lazily claim for detached queue maintenance (`#implementlazilyowned`).
//!
//! A detached maintenance worker owns one admitted maintenance request at a
//! time. That ownership is a `lazily::EffectCasCore` claim, not an
//! application-local lease: Lazily owns the effect identity of each request
//! epoch, the single in-flight attempt per document, the actor generation that
//! fences a crashed or replaced worker, supersession by a newer admission, and
//! the monotone applied frontier.
//!
//! The core is persisted across processes as a full-fidelity
//! `EffectCasDurableState` in the project `state.db`. Every transition runs as
//! load → hydrate → operate → store inside one IMMEDIATE SQLite transaction, so
//! the SQLite write lock serializes contenders in different processes and a
//! successor resumes the exact retained intent a crashed worker held.
//!
//! The application contributes only the narrow backend adapter
//! ([`ConsumeRequestAndLease`]): observing the request row's version and
//! atomically consuming the exact request together with its coordination lease.
//! A request that changed underneath the worker is reported to Lazily as
//! `Superseded { observed }`; the adapter never retries or picks a newer request.
//!
//! The OS apply flock and the SQLite coordination-lease row remain the backend
//! lock behind this claim (`QueueEditGuard`): they make ownership observable to
//! session-check and exclude direct mutations, but they no longer decide which
//! worker's effects are current.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasDurableState, EffectCasFailure, EffectCasOutcome, EffectCasReconnect, EffectCasUpsert,
    EffectIdentity, EffectIntent, EffectPlan, EffectTerminal,
};
use serde::{Deserialize, Serialize};

use crate::queue_edit_owner::{
    QUEUE_EDIT_OWNER_SCOPE, QueueEditGuard, QueueMaintenanceRequest, maintenance_request_location,
};

const QUEUE_MAINTENANCE_CLAIM_PREFIX: &str = "queue_maintenance_claim:";

/// Logical clock for claims. A refused comparison is supersession, never a
/// timed retry, so no claim waits on a retry time.
const NOW: u64 = 0;

/// The backend version of one admitted maintenance request: the latest-wins
/// request row's generation and exact payload. Any newer admission changes it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct QueueMaintenanceRequestVersion {
    pub generation: u64,
    pub payload: String,
}

impl QueueMaintenanceRequestVersion {
    fn of(request: &QueueMaintenanceRequest) -> Self {
        Self {
            generation: request.generation,
            payload: request.payload().to_string(),
        }
    }

    /// The version observed when no request row exists.
    fn absent() -> Self {
        Self {
            generation: 0,
            payload: String::new(),
        }
    }
}

/// The single plan step of a request epoch: consume the exact request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueMaintenanceClaimStep {
    Complete,
}

type ClaimCore = EffectCasCore<
    String,
    QueueMaintenanceRequestVersion,
    QueueMaintenanceClaimStep,
    QueueMaintenanceRequestVersion,
    String,
>;
type ClaimImage = EffectCasDurableState<
    String,
    QueueMaintenanceRequestVersion,
    QueueMaintenanceClaimStep,
    QueueMaintenanceRequestVersion,
    String,
>;
type ClaimAttempt = EffectAttempt<
    String,
    QueueMaintenanceRequestVersion,
    QueueMaintenanceClaimStep,
    QueueMaintenanceRequestVersion,
>;

/// Typed supersession of a queue-maintenance claim. The display text keeps the
/// historical wording that operator diagnostics and ops-log greps rely on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueMaintenanceClaimSuperseded {
    message: String,
}

impl QueueMaintenanceClaimSuperseded {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for QueueMaintenanceClaimSuperseded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for QueueMaintenanceClaimSuperseded {}

/// How [`QueueMaintenanceClaim::claim_request`] obtained the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMaintenanceClaimed {
    /// A new epoch was desired and claimed for this request.
    Desired { epoch: u64 },
    /// Retained work for this exact request (a crashed predecessor's intent)
    /// was resumed under its original effect identity.
    Resumed { epoch: u64 },
    /// This actor already holds the request in flight.
    Held { epoch: u64 },
}

/// One worker's actor handle on the durable per-document claim. Cloning it
/// shares the same actor generation (for example with a heartbeat thread).
#[derive(Debug, Clone)]
pub struct QueueMaintenanceClaim {
    file: PathBuf,
    root: PathBuf,
    document_hash: String,
    claim_key: String,
    request_key: String,
    generation: u64,
}

impl QueueMaintenanceClaim {
    /// Become the claim's current actor: hydrate the durable core and reconnect
    /// at a strictly newer generation. A crashed predecessor's in-flight attempt
    /// is requeued with its effect identity and every later receipt from that
    /// predecessor is fenced as a stale generation.
    pub fn reconnect(file: &Path) -> Result<Self> {
        let (root, document_hash, request_key) = maintenance_request_location(file)?;
        let mut claim = Self {
            file: file.to_path_buf(),
            root,
            claim_key: format!("{QUEUE_MAINTENANCE_CLAIM_PREFIX}{document_hash}"),
            document_hash,
            request_key,
            generation: 0,
        };
        claim.generation = claim.transact(|core, _| {
            let next = core.generation().saturating_add(1);
            match core.reconnect(next).1 {
                EffectCasReconnect::Advanced { generation, .. } => Ok(generation),
                other => anyhow::bail!("queue maintenance claim did not reconnect: {other:?}"),
            }
        })?;
        Ok(claim)
    }

    /// This handle's actor generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn superseded(&self, message: impl Into<String>) -> anyhow::Error {
        QueueMaintenanceClaimSuperseded::new(message).into()
    }

    /// Desire `request` as the latest epoch and claim it for this actor.
    pub fn claim_request(
        &self,
        request: &QueueMaintenanceRequest,
    ) -> Result<QueueMaintenanceClaimed> {
        let version = QueueMaintenanceRequestVersion::of(request);
        let identity_prefix = format!("queue-maintenance:{}", self.document_hash);
        self.transact(|core, _| {
            self.ensure_current_generation(core)?;
            let key = self.document_hash.clone();
            let state = core.state(&key);
            if let Some(attempt) = state.as_ref().and_then(|state| state.inflight.clone()) {
                if attempt.expected == version {
                    return Ok(QueueMaintenanceClaimed::Held {
                        epoch: attempt.identity.epoch,
                    });
                }
                // This actor moved on to a newer request: its older in-flight
                // attempt is superseded by the observed version.
                core.record_superseded(&key, &attempt.identity, version.clone());
            }
            let state = core.state(&key);
            let resumable = state
                .as_ref()
                .and_then(|state| state.pending.as_ref())
                .is_some_and(|pending| pending.next_step == 0 && pending.expected == version);
            let desired = if resumable {
                None
            } else {
                let epoch = state.as_ref().map(last_epoch).unwrap_or(0) + 1;
                let intent = EffectIntent {
                    identity: EffectIdentity::new(format!("{identity_prefix}#{epoch}"))
                        .map_err(|error| anyhow::anyhow!("invalid claim identity: {error:?}"))?,
                    epoch,
                    key: key.clone(),
                    expected: version.clone(),
                    plan: EffectPlan::single(QueueMaintenanceClaimStep::Complete),
                };
                match core.upsert_desired(intent).1 {
                    EffectCasUpsert::Accepted => Some(epoch),
                    other => anyhow::bail!(
                        "queue maintenance claim epoch {epoch} was not accepted: {other:?}"
                    ),
                }
            };
            match core.claim(&key, self.generation, NOW).1 {
                EffectCasClaim::Claimed(attempt) => Ok(match desired {
                    Some(epoch) => QueueMaintenanceClaimed::Desired { epoch },
                    None => QueueMaintenanceClaimed::Resumed {
                        epoch: attempt.identity.epoch,
                    },
                }),
                EffectCasClaim::StaleGeneration { current } => Err(self.superseded(format!(
                    "background queue maintenance reservation was superseded: actor generation {} replaced by {current}",
                    self.generation
                ))),
                other => anyhow::bail!("queue maintenance claim was not claimed: {other:?}"),
            }
        })
    }

    /// Verify that this actor still holds `request` in flight and that the
    /// request row still names it. A changed request is recorded as Lazily
    /// supersession before it is reported, so the claim fails closed from then
    /// on without any application-owned flag.
    pub fn verify(&self, request: &QueueMaintenanceRequest) -> Result<()> {
        let version = QueueMaintenanceRequestVersion::of(request);
        self.transact(|core, transaction| {
            let attempt = self.owned_attempt(core)?;
            if attempt.expected != version {
                return Err(self.superseded(
                    "background queue maintenance request was superseded: the claim holds another request",
                ));
            }
            let observed = observe_request(transaction, &self.request_key)?;
            if observed == attempt.expected {
                return Ok(());
            }
            core.record_superseded(&self.document_hash, &attempt.identity, observed);
            Err(self.superseded("background queue maintenance request was superseded"))
        })
    }

    /// Record that the backend lock behind this claim was lost (the
    /// coordination lease names another holder or could not be refreshed).
    /// The in-flight attempt becomes terminally superseded.
    pub fn record_backend_lock_lost(&self) -> Result<()> {
        self.transact(|core, transaction| {
            let Ok(attempt) = self.owned_attempt(core) else {
                return Ok(());
            };
            let observed = observe_request(transaction, &self.request_key)?;
            core.record_superseded(&self.document_hash, &attempt.identity, observed);
            Ok(())
        })
    }

    /// Complete the in-flight request: atomically consume the exact request row
    /// and this worker's coordination lease, and receipt the epoch in the same
    /// transaction. Returns `false` when a newer admission superseded it (the
    /// request row and lease are untouched) or the claim no longer holds it.
    pub fn complete(
        &self,
        guard: &mut QueueEditGuard,
        request: &QueueMaintenanceRequest,
    ) -> Result<bool> {
        let version = QueueMaintenanceRequestVersion::of(request);
        let holder = guard.holder().to_string();
        let consumed = self.transact(|core, transaction| {
            self.ensure_current_generation(core)?;
            let key = self.document_hash.clone();
            let Some(attempt) = core.state(&key).and_then(|state| state.inflight) else {
                return Ok(false);
            };
            if attempt.identity.generation != self.generation || attempt.expected != version {
                return Ok(false);
            }
            let adapter = ConsumeRequestAndLease {
                transaction,
                request_key: &self.request_key,
                document_hash: &self.document_hash,
                holder: &holder,
            };
            let outcome = adapter.compare_and_apply(&attempt);
            let error = match &outcome {
                EffectApply::Rejected(error) => Some(error.clone()),
                _ => None,
            };
            match core.record_outcome(&key, &attempt.identity, outcome, NOW).1 {
                EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }) => Ok(true),
                // A newer desired epoch already stopped this one; the request
                // row was consumed, but the newer epoch remains desired.
                EffectCasOutcome::Applied(EffectCasApplied::Superseded) => Ok(true),
                EffectCasOutcome::Failure(EffectCasFailure::Superseded) => Ok(false),
                EffectCasOutcome::Failure(EffectCasFailure::Rejected) => Err(anyhow::anyhow!(
                    "queue maintenance claim completion was rejected: {}",
                    error.unwrap_or_default()
                )),
                recorded => {
                    anyhow::bail!("queue maintenance claim completion diverged: {recorded:?}")
                }
            }
        })?;
        if consumed {
            guard.release_consumed_apply();
        }
        Ok(consumed)
    }

    /// Whether this handle is still the claim's current actor generation.
    pub fn is_current_actor(&self) -> Result<bool> {
        Ok(self.load()?.generation() == self.generation)
    }

    /// Whether the durable claim still retains unfinished work owned by this
    /// actor: an in-flight attempt at this generation or a retained pending
    /// intent. A worker that exits while this holds leaves its coordination
    /// lease durable so the attempt stays visible and recoverable.
    pub fn retains_work(&self) -> Result<bool> {
        let core = self.load()?;
        Ok(core.state(&self.document_hash).is_some_and(|state| {
            state
                .inflight
                .as_ref()
                .is_some_and(|attempt| attempt.identity.generation == self.generation)
                || state.pending.is_some()
        }))
    }

    /// The durable claim image's terminal record for this document, if any.
    pub fn terminal(
        &self,
    ) -> Result<
        Option<
            EffectTerminal<QueueMaintenanceRequestVersion, QueueMaintenanceRequestVersion, String>,
        >,
    > {
        Ok(self
            .load()?
            .state(&self.document_hash)
            .and_then(|state| state.terminal))
    }

    /// The durable applied frontier (highest completed request epoch).
    pub fn applied_through(&self) -> Result<Option<u64>> {
        Ok(self
            .load()?
            .state(&self.document_hash)
            .and_then(|state| state.applied_through))
    }

    fn ensure_current_generation(&self, core: &ClaimCore) -> Result<()> {
        if core.generation() != self.generation {
            return Err(self.superseded(format!(
                "background queue maintenance reservation was superseded: actor generation {} replaced by {}",
                self.generation,
                core.generation()
            )));
        }
        Ok(())
    }

    fn owned_attempt(&self, core: &ClaimCore) -> Result<ClaimAttempt> {
        self.ensure_current_generation(core)?;
        core.state(&self.document_hash)
            .and_then(|state| state.inflight)
            .filter(|attempt| attempt.identity.generation == self.generation)
            .ok_or_else(|| {
                self.superseded(
                    "background queue maintenance request was superseded: the claim holds no request",
                )
            })
    }

    fn load(&self) -> Result<ClaimCore> {
        let conn = agent_doc_sqlite::state_store::open_state_db(&self.root)?;
        let payload = agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
            &conn,
            &self.claim_key,
        )?;
        hydrate(payload)
    }

    /// Run one claim transition inside an IMMEDIATE transaction on the project
    /// state database. The core image is stored only when the transition
    /// succeeds; a superseded verification still stores the supersession it
    /// recorded before reporting it.
    fn transact<T>(
        &self,
        transition: impl FnOnce(
            &mut ClaimCore,
            &agent_doc_sqlite::state_store::Transaction<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let mut conn = agent_doc_sqlite::state_store::open_state_db(&self.root)
            .with_context(|| format!("open claim state for {}", self.file.display()))?;
        agent_doc_sqlite::state_store::update_project_runtime_state_in_transaction(
            &mut conn,
            &self.claim_key,
            |transaction, payload| {
                let mut core = hydrate(payload)?;
                let result = transition(&mut core, transaction);
                let image = serde_json::to_string(&core.durable_state())?;
                Ok((Some(image), result))
            },
        )?
    }
}

fn hydrate(payload: Option<String>) -> Result<ClaimCore> {
    match payload {
        None => Ok(ClaimCore::new(0)),
        Some(payload) => {
            let image: ClaimImage = serde_json::from_str(&payload)
                .context("queue maintenance claim image is unreadable")?;
            ClaimCore::hydrate(image).map_err(|error| {
                anyhow::anyhow!("queue maintenance claim image is corrupt: {error}")
            })
        }
    }
}

fn last_epoch(
    state: &lazily::EffectCasKeyState<
        String,
        QueueMaintenanceRequestVersion,
        QueueMaintenanceClaimStep,
        QueueMaintenanceRequestVersion,
        String,
    >,
) -> u64 {
    let terminal = state.terminal.as_ref().map(|terminal| match terminal {
        EffectTerminal::Applied { epoch, .. }
        | EffectTerminal::Superseded { epoch, .. }
        | EffectTerminal::Rejected { epoch, .. } => *epoch,
    });
    [
        state.applied_through,
        terminal,
        state.pending.as_ref().map(|pending| pending.intent.epoch),
        state
            .inflight
            .as_ref()
            .map(|attempt| attempt.identity.epoch),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0)
}

fn observe_request(
    connection: &agent_doc_sqlite::state_store::Connection,
    request_key: &str,
) -> Result<QueueMaintenanceRequestVersion> {
    Ok(
        agent_doc_sqlite::state_store::load_project_runtime_state_with_generation_from_db(
            connection,
            request_key,
        )?
        .map(|(payload, generation)| QueueMaintenanceRequestVersion {
            generation,
            payload,
        })
        .unwrap_or_else(QueueMaintenanceRequestVersion::absent),
    )
}

/// Backend adapter: the request row and the worker's coordination lease are
/// consumed together only while the row still holds the attempt's exact
/// version. A mismatch mutates nothing and reports the observed version.
struct ConsumeRequestAndLease<'t, 'c> {
    transaction: &'t agent_doc_sqlite::state_store::Transaction<'c>,
    request_key: &'t str,
    document_hash: &'t str,
    holder: &'t str,
}

impl
    CompareAndApply<
        String,
        QueueMaintenanceRequestVersion,
        QueueMaintenanceClaimStep,
        QueueMaintenanceRequestVersion,
    > for ConsumeRequestAndLease<'_, '_>
{
    type Error = String;

    fn observe(&self, _key: &String) -> Result<QueueMaintenanceRequestVersion, String> {
        observe_request(self.transaction, self.request_key).map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &ClaimAttempt,
    ) -> EffectApply<QueueMaintenanceRequestVersion, QueueMaintenanceRequestVersion, String> {
        let consumed = agent_doc_sqlite::state_store::consume_runtime_request_and_coordination_lease_in_transaction(
            self.transaction,
            self.request_key,
            &attempt.expected.payload,
            attempt.expected.generation,
            QUEUE_EDIT_OWNER_SCOPE,
            self.document_hash,
            self.holder,
        );
        match consumed {
            Ok(true) => EffectApply::Applied {
                receipt: attempt.expected.clone(),
                authority: QueueMaintenanceRequestVersion::absent(),
            },
            Ok(false) => match self.observe(&attempt.key) {
                Ok(observed) => EffectApply::Superseded { observed },
                Err(error) => EffectApply::Rejected(error),
            },
            Err(error) => EffectApply::Rejected(format!("{error:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_edit_owner::{
        BACKGROUND_QUEUE_EDIT_OWNER_PREFIX, load_queue_maintenance_request,
        publish_queue_maintenance_request, read_queue_edit_owner_lease,
        try_reserve_queue_edit_owner,
    };

    struct Fixture {
        _dir: tempfile::TempDir,
        file: PathBuf,
        token: String,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let file = dir.path().join("plan.md");
            std::fs::write(&file, "body").unwrap();
            let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}{name}");
            assert!(
                try_reserve_queue_edit_owner(&file.to_string_lossy(), &token, std::process::id())
                    .unwrap()
            );
            Self {
                _dir: dir,
                file,
                token,
            }
        }

        fn guard(&self) -> QueueEditGuard {
            QueueEditGuard::adopt_reserved(&self.file, &self.token)
                .unwrap()
                .expect("reserved worker adopts")
        }

        fn holder(&self) -> Option<String> {
            read_queue_edit_owner_lease(&self.file.to_string_lossy()).map(|lease| lease.holder)
        }
    }

    fn superseded(error: &anyhow::Error) -> bool {
        error
            .downcast_ref::<QueueMaintenanceClaimSuperseded>()
            .is_some()
    }

    /// Two actors contend for the same document's claim. The newer actor's
    /// reconnect fences the incumbent: every incumbent transition is a typed
    /// supersession and the backend request/lease are left untouched for the
    /// current actor to consume exactly once.
    #[test]
    fn second_contender_supersedes_the_incumbent_through_lazily() {
        let fixture = Fixture::new("contention");
        let request = publish_queue_maintenance_request(&fixture.file, "hash-a", false).unwrap();
        let mut guard = fixture.guard();

        let incumbent = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        assert_eq!(
            incumbent.claim_request(&request).unwrap(),
            QueueMaintenanceClaimed::Desired { epoch: 1 }
        );
        incumbent.verify(&request).unwrap();

        let contender = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        assert!(contender.generation() > incumbent.generation());
        for error in [
            incumbent.verify(&request).unwrap_err(),
            incumbent.claim_request(&request).unwrap_err(),
            incumbent.complete(&mut guard, &request).unwrap_err(),
        ] {
            assert!(superseded(&error), "{error:#}");
            assert!(
                error.to_string().contains("reservation was superseded"),
                "{error:#}"
            );
        }
        assert_eq!(
            load_queue_maintenance_request(&fixture.file).unwrap(),
            Some(request.clone()),
            "a superseded contender mutates nothing"
        );
        assert_eq!(fixture.holder().as_deref(), Some(fixture.token.as_str()));

        // The contender resumes the retained request epoch, not a new identity.
        assert_eq!(
            contender.claim_request(&request).unwrap(),
            QueueMaintenanceClaimed::Resumed { epoch: 1 }
        );
        assert!(contender.complete(&mut guard, &request).unwrap());
        assert_eq!(contender.applied_through().unwrap(), Some(1));
        assert!(
            load_queue_maintenance_request(&fixture.file)
                .unwrap()
                .is_none()
        );
        assert!(fixture.holder().is_none());
    }

    /// A newer admission supersedes the in-flight request: verification and
    /// completion both report `Superseded` without consuming the newer row, and
    /// the newer epoch completes exactly once.
    #[test]
    fn newer_admission_supersedes_the_in_flight_request_epoch() {
        let fixture = Fixture::new("newer-admission");
        let first = publish_queue_maintenance_request(&fixture.file, "hash-a", false).unwrap();
        let mut guard = fixture.guard();
        let claim = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        claim.claim_request(&first).unwrap();

        let successor = publish_queue_maintenance_request(&fixture.file, "hash-b", true).unwrap();
        let error = claim.verify(&first).unwrap_err();
        assert!(superseded(&error), "{error:#}");
        assert!(
            error.to_string().contains("request was superseded"),
            "{error:#}"
        );
        assert!(
            matches!(
                claim.terminal().unwrap(),
                Some(EffectTerminal::Superseded {
                    epoch: 1,
                    observed: Some(ref observed),
                    ..
                }) if observed.generation == successor.generation
            ),
            "{:?}",
            claim.terminal()
        );
        // Supersession is sticky: the old request can no longer be verified.
        assert!(claim.verify(&first).is_err());
        assert!(!claim.complete(&mut guard, &first).unwrap());
        assert_eq!(
            load_queue_maintenance_request(&fixture.file).unwrap(),
            Some(successor.clone())
        );
        assert_eq!(fixture.holder().as_deref(), Some(fixture.token.as_str()));

        assert_eq!(
            claim.claim_request(&successor).unwrap(),
            QueueMaintenanceClaimed::Desired { epoch: 2 }
        );
        assert_eq!(
            claim.claim_request(&successor).unwrap(),
            QueueMaintenanceClaimed::Held { epoch: 2 }
        );
        claim.verify(&successor).unwrap();
        assert!(claim.complete(&mut guard, &successor).unwrap());
        assert_eq!(claim.applied_through().unwrap(), Some(2));
        assert!(!claim.retains_work().unwrap());
        assert!(
            load_queue_maintenance_request(&fixture.file)
                .unwrap()
                .is_none()
        );
        assert!(fixture.holder().is_none());
    }

    /// A worker that dies mid-request leaves its in-flight epoch durable; a
    /// successor process hydrates it and resumes the same effect identity.
    #[test]
    fn crashed_actor_retains_work_and_successor_resumes_it() {
        let fixture = Fixture::new("crash");
        let request = publish_queue_maintenance_request(&fixture.file, "hash-a", false).unwrap();
        let crashed = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        assert_eq!(
            crashed.claim_request(&request).unwrap(),
            QueueMaintenanceClaimed::Desired { epoch: 1 }
        );
        assert!(crashed.retains_work().unwrap());

        let successor = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        assert!(
            successor.retains_work().unwrap(),
            "requeued retained intent"
        );
        assert_eq!(
            successor.claim_request(&request).unwrap(),
            QueueMaintenanceClaimed::Resumed { epoch: 1 }
        );
        let mut guard = fixture.guard();
        assert!(superseded(
            &crashed.complete(&mut guard, &request).unwrap_err()
        ));
        assert!(successor.complete(&mut guard, &request).unwrap());
        assert!(!successor.retains_work().unwrap());
    }

    /// `#answerheadmaintrace` (tmux-ci `answer_head`, 9a0eb19): completion
    /// consumes the coordination lease in the claim transaction. A worker that
    /// still held the apply flock afterwards left "flock held, no lease" for as
    /// long as its remaining teardown took: `retains_work` opens `state.db`
    /// (30 s busy timeout plus open-time maintenance) and the heartbeat join,
    /// both before the guard drops. Foreground acquirers bound that window by
    /// `QUEUE_APPLY_HANDOFF_GRACE`, so under `state.db` contention an admitted
    /// `write --commit` failed with `queue mutation deferred` while the 30 s
    /// response wait would still have admitted it. The consumed guard must
    /// release the flock at completion, so no teardown runs inside the window.
    /// The worker's guard stays alive across the whole response acquisition
    /// (an arbitrarily slow teardown), and the handoff grace is shortened to
    /// make outliving it deterministic.
    #[test]
    fn a_completed_claim_releases_the_apply_flock_before_worker_teardown() {
        let fixture = Fixture::new("completion-handoff");
        let request = publish_queue_maintenance_request(&fixture.file, "hash-a", true).unwrap();
        let mut guard = fixture.guard();
        let claim = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        claim.claim_request(&request).unwrap();
        assert!(claim.complete(&mut guard, &request).unwrap());
        assert!(fixture.holder().is_none(), "completion consumed the lease");

        let mut observations = Vec::new();
        let response = QueueEditGuard::acquire_with_background_wait_observed(
            &fixture.file,
            crate::queue_edit_owner::RESPONSE_BACKGROUND_WAIT,
            std::time::Duration::from_millis(30),
            |lease| observations.push(lease.cloned()),
        )
        .expect("a response after claim completion must not wait on the worker's teardown");
        assert!(
            observations.is_empty(),
            "the completed worker still held the apply flock with no lease: {observations:?}"
        );
        assert!(
            fixture
                .holder()
                .is_some_and(|holder| !holder.starts_with(BACKGROUND_QUEUE_EDIT_OWNER_PREFIX)),
            "the response owns the apply capability"
        );
        drop(response);

        // The worker's teardown runs after the response, and it neither
        // retains work nor disturbs the released capability.
        assert!(!claim.retains_work().unwrap());
        drop(guard);
        assert!(fixture.holder().is_none());
    }

    #[test]
    fn corrupt_claim_image_fails_closed() {
        let fixture = Fixture::new("corrupt");
        let claim = QueueMaintenanceClaim::reconnect(&fixture.file).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&claim.root).unwrap();
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &claim.claim_key,
            "{\"generation\":1,\"keys\":\"not-a-list\"}",
            0,
        )
        .unwrap();
        let request = publish_queue_maintenance_request(&fixture.file, "hash-a", false).unwrap();
        assert!(claim.claim_request(&request).is_err());
        assert!(claim.retains_work().is_err());
    }
}
