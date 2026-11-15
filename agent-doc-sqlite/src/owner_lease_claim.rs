//! Durable Lazily owner-lease claims (`#implementlazilyowned`, family 2).
//!
//! A cross-process owner lease names the one process that currently owns an
//! effect: the `/loop` or explicit owner draining a queue, the predecessor
//! controller still draining its accepted work, the controller's orphan-drain
//! dispatch slot. Every ownership transition is a `lazily::EffectCasCore`
//! epoch persisted as a full-fidelity `EffectCasDurableState` in the project
//! `state.db`, exactly as `queue_maintenance_claim` persists its claim: load,
//! hydrate, operate, and store run inside one IMMEDIATE SQLite transaction, so
//! the write lock serializes contenders in different processes.
//!
//! One grant is one epoch with the two-step plan `[Grant, Release]`. Lazily
//! owns:
//! - the holder's effect identity: the retained `Release` step of the latest
//!   granted epoch *is* the ownership;
//! - supersession: a newer grant supersedes the retained epoch of the previous
//!   holder (`EffectTerminal::Superseded`), with no application-owned flag;
//! - receipt matching: a release completes the holder's own epoch, so a holder
//!   that was superseded can never clear its successor's grant;
//! - the monotone applied frontier of released epochs.
//!
//! The backend row (a coordination-lease row, or a controller-bootstrap slot)
//! stays the observable backend lock behind the claim, as the coordination
//! lease stays behind `queue_maintenance_claim`. [`OwnerLeaseWrite`] is the
//! narrow `CompareAndApply` adapter: it compares the backend's current holder
//! with the epoch's expected holder and writes or clears the row only on a
//! match. A mismatch mutates nothing and is `Superseded { observed }`; a
//! SQLite failure is `Rejected`. The adapter never retries and never chooses a
//! newer holder. Refreshing a live grant's heartbeat by the same holder is the
//! backend lock's own renewal, not a new effect.

use std::fmt::Debug;

use anyhow::{Context, Result};
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasDurableState, EffectCasFailure, EffectCasOutcome, EffectCasUpsert, EffectIdentity,
    EffectIntent, EffectPlan, EffectTerminal,
};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::state_store::{
    CoordinationLeaseRecord, Transaction, clear_coordination_lease_in_db,
    load_controller_bootstrap_json_from_db, load_coordination_lease_from_db,
    load_project_runtime_state_from_db, store_controller_bootstrap_json_in_db,
    update_project_runtime_state_in_transaction, upsert_coordination_lease_in_db,
};

const OWNER_CLAIM_STATE_PREFIX: &str = "owner_lease_claim:";

/// The single actor generation of owner claims: every transition completes
/// inside one transaction, so no attempt is ever in flight across processes
/// and the core never reconnects.
const GENERATION: u64 = 0;

/// Logical clock: an ownership transition never schedules a timed retry.
const NOW: u64 = 0;

/// The holder identity an owner claim compares: who owns the effect, not
/// when its heartbeat last moved.
pub type OwnerHolder = Option<String>;

/// A backend grant value (the row the backend lock stores).
pub trait OwnerGrant: Clone + PartialEq + Debug + Serialize + DeserializeOwned {
    /// The stable identity of the owner this grant names.
    fn holder(&self) -> String;
}

impl OwnerGrant for CoordinationLeaseRecord {
    fn holder(&self) -> String {
        self.holder.clone()
    }
}

impl OwnerGrant for agent_doc_controller::recycle::PredecessorDrainLease {
    fn holder(&self) -> String {
        format!("controller:{}:{}", self.pid, self.generation)
    }
}

/// The backend lock behind one owner claim: an atomic observe/write/clear of
/// one row, always called inside the claim's IMMEDIATE transaction.
pub trait OwnerLeaseBackend {
    type Grant: OwnerGrant;

    /// Stable scope of this claim (one durable Lazily core per scope).
    fn claim_scope(&self) -> String;
    fn observe(&self, conn: &Connection) -> Result<Option<Self::Grant>>;
    fn write(&self, conn: &Connection, grant: &Self::Grant) -> Result<()>;
    fn clear(&self, conn: &Connection) -> Result<()>;
}

/// A coordination-lease row (`coordination_leases`) as the backend lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationLeaseBackend {
    pub scope_kind: String,
    pub scope_id: String,
}

impl CoordinationLeaseBackend {
    pub fn new(scope_kind: impl Into<String>, scope_id: impl Into<String>) -> Self {
        Self {
            scope_kind: scope_kind.into(),
            scope_id: scope_id.into(),
        }
    }
}

impl OwnerLeaseBackend for CoordinationLeaseBackend {
    type Grant = CoordinationLeaseRecord;

    fn claim_scope(&self) -> String {
        format!("coordination:{}:{}", self.scope_kind, self.scope_id)
    }

    fn observe(&self, conn: &Connection) -> Result<Option<CoordinationLeaseRecord>> {
        load_coordination_lease_from_db(conn, &self.scope_kind, &self.scope_id)
    }

    fn write(&self, conn: &Connection, grant: &CoordinationLeaseRecord) -> Result<()> {
        anyhow::ensure!(
            grant.scope_kind == self.scope_kind && grant.scope_id == self.scope_id,
            "owner claim grant targets {}:{}, not this claim's {}:{}",
            grant.scope_kind,
            grant.scope_id,
            self.scope_kind,
            self.scope_id
        );
        upsert_coordination_lease_in_db(conn, grant)
    }

    fn clear(&self, conn: &Connection) -> Result<()> {
        clear_coordination_lease_in_db(conn, &self.scope_kind, &self.scope_id).map(|_| ())
    }
}

/// A single-slot controller-bootstrap JSON row as the backend lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapSlotBackend<G> {
    pub scope: String,
    grant: std::marker::PhantomData<G>,
}

impl<G> BootstrapSlotBackend<G> {
    pub fn new(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            grant: std::marker::PhantomData,
        }
    }
}

impl<G: OwnerGrant> OwnerLeaseBackend for BootstrapSlotBackend<G> {
    type Grant = G;

    fn claim_scope(&self) -> String {
        format!("bootstrap:{}", self.scope)
    }

    fn observe(&self, conn: &Connection) -> Result<Option<G>> {
        // An unreadable slot is no grant: the next grant replaces it.
        Ok(load_controller_bootstrap_json_from_db(conn, &self.scope)?
            .and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn write(&self, conn: &Connection, grant: &G) -> Result<()> {
        store_controller_bootstrap_json_in_db(conn, &self.scope, &serde_json::to_string(grant)?)
    }

    fn clear(&self, conn: &Connection) -> Result<()> {
        conn.execute(
            "DELETE FROM controller_bootstrap WHERE scope = ?1",
            [&self.scope],
        )?;
        Ok(())
    }
}

/// One step of an owner epoch: grant the lease, then (later) release it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "G: Serialize + DeserializeOwned")]
pub enum OwnerLeaseStep<G> {
    Grant(G),
    Release,
}

type OwnerCore<G> = EffectCasCore<String, OwnerHolder, OwnerLeaseStep<G>, OwnerHolder, String>;
type OwnerImage<G> =
    EffectCasDurableState<String, OwnerHolder, OwnerLeaseStep<G>, OwnerHolder, String>;
type OwnerAttempt<G> = EffectAttempt<String, OwnerHolder, OwnerLeaseStep<G>, OwnerHolder>;

/// How [`acquire_owner_lease`] resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerLeaseAcquired<G> {
    /// A new epoch granted the lease; `superseded` is the holder it replaced.
    Granted { epoch: u64, superseded: OwnerHolder },
    /// The holder of the current epoch refreshed its own backend grant.
    Renewed { epoch: u64 },
    /// Admission refused: the observed grant keeps the lease, nothing changed.
    Held(G),
}

/// `lazily::CompareAndApply` adapter for one owner-claim step: compare the
/// backend's holder with the attempt's expected holder and write (`Grant`) or
/// clear (`Release`) the row in the claim's open IMMEDIATE transaction.
pub struct OwnerLeaseWrite<'t, 'c, B> {
    transaction: &'t Transaction<'c>,
    backend: &'t B,
}

impl<B: OwnerLeaseBackend> OwnerLeaseWrite<'_, '_, B> {
    fn observe_holder(&self) -> Result<OwnerHolder> {
        Ok(self
            .backend
            .observe(self.transaction)?
            .map(|grant| grant.holder()))
    }

    fn apply(
        &self,
        attempt: &OwnerAttempt<B::Grant>,
    ) -> Result<EffectApply<OwnerHolder, OwnerHolder, String>> {
        let observed = self.observe_holder()?;
        if observed != attempt.expected {
            return Ok(EffectApply::Superseded { observed });
        }
        Ok(match &attempt.payload {
            OwnerLeaseStep::Grant(grant) => {
                self.backend.write(self.transaction, grant)?;
                let holder = Some(grant.holder());
                EffectApply::Applied {
                    receipt: holder.clone(),
                    authority: holder,
                }
            }
            OwnerLeaseStep::Release => {
                self.backend.clear(self.transaction)?;
                EffectApply::Applied {
                    receipt: None,
                    authority: None,
                }
            }
        })
    }
}

impl<B: OwnerLeaseBackend>
    CompareAndApply<String, OwnerHolder, OwnerLeaseStep<B::Grant>, OwnerHolder>
    for OwnerLeaseWrite<'_, '_, B>
{
    type Error = String;

    fn observe(&self, _key: &String) -> Result<OwnerHolder, String> {
        self.observe_holder().map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &OwnerAttempt<B::Grant>,
    ) -> EffectApply<OwnerHolder, OwnerHolder, String> {
        self.apply(attempt)
            .unwrap_or_else(|error| EffectApply::Rejected(format!("{error:#}")))
    }
}

/// Effect-free read of the backend grant behind a claim.
pub fn observe_owner_lease<B: OwnerLeaseBackend>(
    conn: &Connection,
    backend: &B,
) -> Result<Option<B::Grant>> {
    backend.observe(conn)
}

/// Desire `grant` as the owner of `backend`'s scope.
///
/// `admit` sees the backend's current grant and decides whether this
/// acquisition may proceed at all (a latest-wins owner always admits; a
/// backoff slot admits only an absent or expired grant). An admitted grant by
/// the current epoch's own holder renews the backend row; any other holder is
/// a new Lazily epoch that supersedes the previous holder's retained epoch.
pub fn acquire_owner_lease<B: OwnerLeaseBackend>(
    conn: &mut Connection,
    backend: &B,
    grant: B::Grant,
    admit: impl FnOnce(Option<&B::Grant>) -> bool,
) -> Result<OwnerLeaseAcquired<B::Grant>> {
    let scope = backend.claim_scope();
    transact(conn, &scope, |core, transaction| {
        let observed = backend.observe(transaction)?;
        if !admit(observed.as_ref()) {
            return observed.map(OwnerLeaseAcquired::Held).context(
                "owner claim admission refused an absent grant; admit must accept a free slot",
            );
        }
        let holder = grant.holder();
        let observed_holder = observed.as_ref().map(OwnerGrant::holder);
        if let Some(epoch) = retained_holder_epoch(core, &scope, &holder)
            && observed_holder.as_deref() == Some(holder.as_str())
        {
            backend.write(transaction, &grant)?;
            return Ok(OwnerLeaseAcquired::Renewed { epoch });
        }
        let epoch = desire(
            core,
            &scope,
            observed_holder.clone(),
            vec![OwnerLeaseStep::Grant(grant), OwnerLeaseStep::Release],
        )?;
        match run_claimed_step(core, &scope, transaction, backend)? {
            StepOutcome::Applied => Ok(OwnerLeaseAcquired::Granted {
                epoch,
                superseded: observed_holder,
            }),
            StepOutcome::Superseded(observed) => anyhow::bail!(
                "owner claim {scope} grant epoch {epoch} was superseded by {observed:?} inside its own transaction"
            ),
        }
    })
}

/// Release `backend`'s scope when `holder` (or, for `None`, whoever) owns it.
///
/// The current holder's release completes its own retained Lazily epoch. A
/// row that no epoch owns (written by an older binary) is released through a
/// release-only epoch over the observed holder. A different holder, or no
/// grant at all, mutates nothing and returns `false`.
pub fn release_owner_lease<B: OwnerLeaseBackend>(
    conn: &mut Connection,
    backend: &B,
    holder: Option<&str>,
) -> Result<bool> {
    let scope = backend.claim_scope();
    transact(conn, &scope, |core, transaction| {
        let observed = backend
            .observe(transaction)?
            .map(|grant| OwnerGrant::holder(&grant));
        let Some(observed) = observed else {
            return Ok(false);
        };
        if holder.is_some_and(|holder| holder != observed) {
            return Ok(false);
        }
        if retained_holder_epoch(core, &scope, &observed).is_none() {
            desire(
                core,
                &scope,
                Some(observed.clone()),
                vec![OwnerLeaseStep::Release],
            )?;
        }
        Ok(matches!(
            run_claimed_step(core, &scope, transaction, backend)?,
            StepOutcome::Applied
        ))
    })
}

/// The holder named by the claim's retained (granted, not yet released) epoch.
pub fn owner_lease_holder<B: OwnerLeaseBackend>(
    conn: &Connection,
    backend: &B,
) -> Result<OwnerHolder> {
    let scope = backend.claim_scope();
    Ok(retained_holder(&load::<B::Grant>(conn, &scope)?, &scope))
}

/// The durable claim's terminal record for `backend`'s scope, if any.
pub fn owner_lease_terminal<B: OwnerLeaseBackend>(
    conn: &Connection,
    backend: &B,
) -> Result<Option<EffectTerminal<OwnerHolder, OwnerHolder, String>>> {
    let scope = backend.claim_scope();
    Ok(load::<B::Grant>(conn, &scope)?
        .state(&scope)
        .and_then(|state| state.terminal))
}

enum StepOutcome {
    Applied,
    Superseded(OwnerHolder),
}

fn retained_holder<G: OwnerGrant>(core: &OwnerCore<G>, scope: &String) -> OwnerHolder {
    let pending = core.state(scope)?.pending?;
    if pending.next_step != 1 {
        return None;
    }
    match pending.intent.plan.steps().first() {
        Some(OwnerLeaseStep::Grant(grant)) => Some(grant.holder()),
        _ => None,
    }
}

fn retained_holder_epoch<G: OwnerGrant>(
    core: &OwnerCore<G>,
    scope: &String,
    holder: &str,
) -> Option<u64> {
    let pending = core.state(scope)?.pending?;
    (retained_holder(core, scope).as_deref() == Some(holder)).then_some(pending.intent.epoch)
}

fn desire<G: OwnerGrant>(
    core: &mut OwnerCore<G>,
    scope: &String,
    expected: OwnerHolder,
    steps: Vec<OwnerLeaseStep<G>>,
) -> Result<u64> {
    let epoch = core.state(scope).as_ref().map(last_epoch).unwrap_or(0) + 1;
    let intent = EffectIntent {
        identity: EffectIdentity::new(format!("owner-claim:{scope}#{epoch}"))
            .map_err(|error| anyhow::anyhow!("invalid owner claim identity: {error:?}"))?,
        epoch,
        key: scope.clone(),
        expected,
        plan: EffectPlan::new(steps)
            .map_err(|error| anyhow::anyhow!("invalid owner claim plan: {error:?}"))?,
    };
    match core.upsert_desired(intent).1 {
        EffectCasUpsert::Accepted => Ok(epoch),
        other => anyhow::bail!("owner claim {scope} epoch {epoch} was not accepted: {other:?}"),
    }
}

/// Claim the scope's next pending step and record the adapter's outcome.
fn run_claimed_step<B: OwnerLeaseBackend>(
    core: &mut OwnerCore<B::Grant>,
    scope: &String,
    transaction: &Transaction<'_>,
    backend: &B,
) -> Result<StepOutcome> {
    let attempt = match core.claim(scope, GENERATION, NOW).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => anyhow::bail!("owner claim {scope} was not claimed: {other:?}"),
    };
    let adapter = OwnerLeaseWrite {
        transaction,
        backend,
    };
    let outcome = adapter.compare_and_apply(&attempt);
    let rejected = match &outcome {
        EffectApply::Rejected(error) => Some(error.clone()),
        _ => None,
    };
    let observed = match &outcome {
        EffectApply::Superseded { observed } => Some(observed.clone()),
        _ => None,
    };
    match core
        .record_outcome(scope, &attempt.identity, outcome, NOW)
        .1
    {
        EffectCasOutcome::Applied(
            EffectCasApplied::StepApplied { .. } | EffectCasApplied::Completed { .. },
        ) => Ok(StepOutcome::Applied),
        EffectCasOutcome::Failure(EffectCasFailure::Superseded) => {
            Ok(StepOutcome::Superseded(observed.flatten()))
        }
        EffectCasOutcome::Failure(EffectCasFailure::Rejected) => Err(anyhow::anyhow!(
            "owner claim {scope} step was rejected: {}",
            rejected.unwrap_or_default()
        )),
        recorded => anyhow::bail!("owner claim {scope} outcome diverged from Lazily: {recorded:?}"),
    }
}

fn last_epoch<G>(
    state: &lazily::EffectCasKeyState<String, OwnerHolder, OwnerLeaseStep<G>, OwnerHolder, String>,
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

fn hydrate<G: OwnerGrant>(payload: Option<String>) -> Result<OwnerCore<G>> {
    match payload {
        None => Ok(OwnerCore::new(GENERATION)),
        Some(payload) => {
            let image: OwnerImage<G> =
                serde_json::from_str(&payload).context("owner claim image is unreadable")?;
            OwnerCore::hydrate(image)
                .map_err(|error| anyhow::anyhow!("owner claim image is corrupt: {error:?}"))
        }
    }
}

fn state_key(scope: &str) -> String {
    format!("{OWNER_CLAIM_STATE_PREFIX}{scope}")
}

fn load<G: OwnerGrant>(conn: &Connection, scope: &str) -> Result<OwnerCore<G>> {
    hydrate(load_project_runtime_state_from_db(conn, &state_key(scope))?)
}

/// Run one claim transition inside an IMMEDIATE transaction. The core image
/// and the backend row commit together; an error rolls back both.
fn transact<G: OwnerGrant, T>(
    conn: &mut Connection,
    scope: &str,
    transition: impl FnOnce(&mut OwnerCore<G>, &Transaction<'_>) -> Result<T>,
) -> Result<T> {
    update_project_runtime_state_in_transaction(conn, &state_key(scope), |transaction, payload| {
        let mut core = hydrate::<G>(payload)?;
        let value = transition(&mut core, transaction)?;
        Ok((Some(serde_json::to_string(&core.durable_state())?), value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::open_state_db;
    use agent_doc_controller::recycle::PredecessorDrainLease;

    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let conn = open_state_db(dir.path()).unwrap();
        (dir, conn)
    }

    fn backend() -> CoordinationLeaseBackend {
        CoordinationLeaseBackend::new("queue_drain", "doc-hash")
    }

    fn grant(holder: &str, heartbeat_secs: u64) -> CoordinationLeaseRecord {
        CoordinationLeaseRecord {
            scope_kind: "queue_drain".to_string(),
            scope_id: "doc-hash".to_string(),
            holder: holder.to_string(),
            holder_pid: Some(7),
            heartbeat_secs,
        }
    }

    #[test]
    fn a_newer_holder_supersedes_the_retained_epoch_and_the_stale_holder_cannot_release_it() {
        let (_dir, mut conn) = db();
        let backend = backend();
        assert_eq!(
            acquire_owner_lease(&mut conn, &backend, grant("loop", 10), |_| true).unwrap(),
            OwnerLeaseAcquired::Granted {
                epoch: 1,
                superseded: None
            }
        );
        assert_eq!(
            acquire_owner_lease(&mut conn, &backend, grant("operator", 11), |_| true).unwrap(),
            OwnerLeaseAcquired::Granted {
                epoch: 2,
                superseded: Some("loop".to_string())
            }
        );
        assert!(matches!(
            owner_lease_terminal(&conn, &backend).unwrap(),
            Some(EffectTerminal::Superseded { epoch: 1, .. })
        ));

        // The superseded holder's release matches no retained epoch and
        // leaves the successor's backend row byte-for-byte unchanged.
        assert!(!release_owner_lease(&mut conn, &backend, Some("loop")).unwrap());
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(grant("operator", 11))
        );
        assert_eq!(
            owner_lease_holder(&conn, &backend).unwrap(),
            Some("operator".to_string())
        );
    }

    #[test]
    fn the_holder_renews_its_epoch_and_its_release_completes_it() {
        let (_dir, mut conn) = db();
        let backend = backend();
        acquire_owner_lease(&mut conn, &backend, grant("loop", 10), |_| true).unwrap();
        assert_eq!(
            acquire_owner_lease(&mut conn, &backend, grant("loop", 40), |_| true).unwrap(),
            OwnerLeaseAcquired::Renewed { epoch: 1 }
        );
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(grant("loop", 40))
        );
        assert!(release_owner_lease(&mut conn, &backend, Some("loop")).unwrap());
        assert_eq!(observe_owner_lease(&conn, &backend).unwrap(), None);
        assert!(matches!(
            owner_lease_terminal(&conn, &backend).unwrap(),
            Some(EffectTerminal::Applied { epoch: 1, .. })
        ));
        assert_eq!(owner_lease_holder(&conn, &backend).unwrap(), None);
        assert!(!release_owner_lease(&mut conn, &backend, None).unwrap());
    }

    #[test]
    fn a_refused_admission_mutates_nothing_and_keeps_the_holder_epoch() {
        let (_dir, mut conn) = db();
        let backend = backend();
        acquire_owner_lease(&mut conn, &backend, grant("attempt-a", 100), |_| true).unwrap();
        let fresh = |observed: Option<&CoordinationLeaseRecord>| {
            observed.is_none_or(|lease| lease.heartbeat_secs <= 50)
        };
        assert_eq!(
            acquire_owner_lease(&mut conn, &backend, grant("attempt-b", 120), fresh).unwrap(),
            OwnerLeaseAcquired::Held(grant("attempt-a", 100))
        );
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(grant("attempt-a", 100))
        );
        assert_eq!(
            owner_lease_holder(&conn, &backend).unwrap(),
            Some("attempt-a".to_string())
        );
    }

    #[test]
    fn a_row_no_epoch_owns_is_released_through_a_release_only_epoch() {
        let (_dir, mut conn) = db();
        let backend = backend();
        upsert_coordination_lease_in_db(&conn, &grant("legacy", 5)).unwrap();
        assert!(!release_owner_lease(&mut conn, &backend, Some("loop")).unwrap());
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(grant("legacy", 5))
        );
        assert!(release_owner_lease(&mut conn, &backend, None).unwrap());
        assert_eq!(observe_owner_lease(&conn, &backend).unwrap(), None);
    }

    #[test]
    fn a_backend_moved_under_the_retained_epoch_is_superseded_without_mutation() {
        let (_dir, mut conn) = db();
        let backend = backend();
        acquire_owner_lease(&mut conn, &backend, grant("loop", 10), |_| true).unwrap();
        // An expiry sweep removed the row outside the claim.
        clear_coordination_lease_in_db(&conn, "queue_drain", "doc-hash").unwrap();
        upsert_coordination_lease_in_db(&conn, &grant("other", 12)).unwrap();
        // The retained epoch names `loop`; the release-only epoch for `other`
        // supersedes it, and only `other` is cleared.
        assert!(!release_owner_lease(&mut conn, &backend, Some("loop")).unwrap());
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(grant("other", 12))
        );
        assert!(release_owner_lease(&mut conn, &backend, Some("other")).unwrap());
        assert_eq!(observe_owner_lease(&conn, &backend).unwrap(), None);
    }

    #[test]
    fn a_bootstrap_slot_grant_is_latest_wins_across_controller_generations() {
        let (_dir, mut conn) = db();
        let backend = BootstrapSlotBackend::<PredecessorDrainLease>::new("predecessor_drain_lease");
        let lease = |pid, generation| PredecessorDrainLease {
            pid,
            generation,
            deadline_secs: 500,
        };
        acquire_owner_lease(&mut conn, &backend, lease(11, 3), |_| true).unwrap();
        assert_eq!(
            acquire_owner_lease(&mut conn, &backend, lease(12, 4), |_| true).unwrap(),
            OwnerLeaseAcquired::Granted {
                epoch: 2,
                superseded: Some("controller:11:3".to_string())
            }
        );
        assert_eq!(
            observe_owner_lease(&conn, &backend).unwrap(),
            Some(lease(12, 4))
        );
    }
}
