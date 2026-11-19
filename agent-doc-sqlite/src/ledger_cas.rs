//! Lazily-owned controller/ledger SQLite CAS (`#implementlazilyowned`).
//!
//! The controller's typed ledger lives in `state.db`, and one
//! `BEGIN IMMEDIATE` transaction is that backend's atomic comparator.
//! [`TerminalTurnLedgerAppend`] is the narrow `lazily::CompareAndApply`
//! adapter for the terminal closeout append: `observe` reads the ledger's
//! terminal-turn authority ([`TerminalTurnAuthority`]: the latest checkpointed
//! cycle, the bound actor, and whether that actor's turn is still live)
//! without side effects, and `compare_and_apply` re-reads it inside one
//! IMMEDIATE transaction and appends the closeout facts only when it equals
//! the attempt's expected authority. A mismatch rolls the transaction back,
//! writes zero rows, and is reported to Lazily as `Superseded { observed }`; a
//! SQLite failure is `Rejected`. The adapter never retries and never chooses a
//! newer intent.
//!
//! `lazily::EffectCasCore` owns the append's identity, expected authority,
//! single-flight claim, and supersession: [`append_if_terminal_turn`] runs one
//! desired epoch. A terminal closeout whose turn authority moved does not want
//! its facts any more, so a supersession is final; nothing re-desires it.
//!
//! The actor-generation fenced writes share the shape. [`ActorGenerationWrite`]
//! compares a document's actor authority ([`ActorGenerationAuthority`]: the
//! generation a write supersedes and the exact committed write,
//! `ActorRevision`) and stores or deletes the record in one IMMEDIATE
//! transaction; [`write_if_actor_generation`] runs one epoch and reports a loss
//! as the typed [`ActorGenerationSuperseded`]. A writer that re-decides after a
//! loss runs through [`drive_actor_generation_decisions`], where each decision
//! is the next Lazily epoch; no caller keeps its own retry loop.

use std::cell::RefCell;

use anyhow::Result;
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasFailure, EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
};
use rusqlite::{Connection, OptionalExtension, params};

use agent_doc_controller::actor::{ActorRecord, ActorRevision, ActorStoreWrite};
pub use agent_doc_supervisor::actor_generation::{
    ACTOR_GENERATION_SUPERSEDED_MARKER, actor_generation_superseded_detail,
};

use crate::state_store::{
    FencedStateEventInsert, StateEventInsert, TerminalTurnFenceCheck,
    apply_actor_record_write_in_tx, delete_actor_document_rows_in_tx, insert_actor_transition,
    insert_state_event_in_db, load_actor_record_with_revision_from_db,
};

/// The single actor generation of these in-process cores: every append owns
/// its own core, so they never reconnect.
const GENERATION: u64 = 0;

/// Logical clock for claims: a terminal append never schedules a timed retry.
const NOW: u64 = 0;

/// The actor a terminal closeout is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTurnActor {
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
}

/// Opaque authority of the terminal-turn ledger for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTurnAuthority {
    /// Cycle id of the latest `turn_intent_checkpointed` fact.
    pub cycle_id: Option<String>,
    /// The document's bound actor (`documents` row).
    pub actor: Option<TerminalTurnActor>,
    /// Whether the fence pane still holds a live `turn_active` lease.
    pub live_turn: bool,
}

impl TerminalTurnFenceCheck<'_> {
    /// The authority a terminal append expects: the fence's cycle and actor,
    /// with no live turn. `None` when the fence names a partial actor, which
    /// no ledger state can equal.
    pub fn expected_authority(&self) -> Option<TerminalTurnAuthority> {
        let actor = match (self.session_id, self.pane_id, self.generation) {
            (None, None, None) => None,
            (Some(session_id), Some(pane_id), Some(generation)) => Some(TerminalTurnActor {
                session_id: session_id.to_string(),
                pane_id: pane_id.to_string(),
                generation,
            }),
            _ => return None,
        };
        Some(TerminalTurnAuthority {
            cycle_id: Some(self.expected_cycle_id.to_string()),
            actor,
            live_turn: false,
        })
    }
}

/// Effect-free read of the terminal-turn authority. Inside an IMMEDIATE
/// transaction it is the comparator's observation; on a plain connection it is
/// Lazily's `observe`.
pub fn read_terminal_turn_authority(
    conn: &Connection,
    fence: &TerminalTurnFenceCheck<'_>,
) -> Result<TerminalTurnAuthority> {
    let checkpoint_payload: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM state_events \
             WHERE document_hash = ?1 AND fact_type = 'turn_intent_checkpointed' \
             ORDER BY document_version DESC, id DESC LIMIT 1",
            [fence.document_hash],
            |row| row.get(0),
        )
        .optional()?;
    let cycle_id = checkpoint_payload
        .as_deref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|event| {
            event
                .get("fact")?
                .get("cycle_id")?
                .as_str()
                .map(str::to_string)
        });
    let actor = conn
        .query_row(
            "SELECT session_id, pane_id, generation FROM documents WHERE document_id = ?1",
            [fence.document_id],
            |row| {
                let generation: i64 = row.get(2)?;
                Ok(TerminalTurnActor {
                    session_id: row.get(0)?,
                    pane_id: row.get(1)?,
                    generation: u64::try_from(generation).unwrap_or_default(),
                })
            },
        )
        .optional()?;
    let live_turn = match fence.pane_id {
        None => false,
        Some(pane) => {
            let live_turn_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM coordination_leases \
                 WHERE scope_kind = 'turn_active' \
                   AND heartbeat_secs > ?1 \
                   AND ((scope_id = ?2 AND holder = ?2) \
                        OR (scope_id = 'project' AND holder = ?2))",
                params![fence.active_after_secs as i64, pane],
                |row| row.get(0),
            )?;
            live_turn_count > 0
        }
    };
    Ok(TerminalTurnAuthority {
        cycle_id,
        actor,
        live_turn,
    })
}

/// Which document's terminal ledger an append targets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalTurnKey {
    pub document_hash: String,
}

/// Payload of one terminal append: the event ids it writes (identity only;
/// the rows themselves are borrowed by the adapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTurnAppend {
    pub event_ids: Vec<String>,
}

/// Receipt of an applied append: per event, whether it inserted a new row
/// (`false` = the event id already existed, an idempotent repeat).
pub type TerminalTurnReceipt = Vec<bool>;

type TerminalTurnCore = EffectCasCore<
    TerminalTurnKey,
    TerminalTurnAuthority,
    TerminalTurnAppend,
    TerminalTurnReceipt,
    String,
>;

/// `lazily::CompareAndApply` adapter: compare the terminal-turn authority and
/// append the closeout facts in one IMMEDIATE transaction.
pub struct TerminalTurnLedgerAppend<'c, 'f, 'e> {
    conn: RefCell<&'c mut Connection>,
    fence: &'f TerminalTurnFenceCheck<'f>,
    events: &'e [StateEventInsert<'e>],
    error: RefCell<Option<anyhow::Error>>,
}

impl<'c, 'f, 'e> TerminalTurnLedgerAppend<'c, 'f, 'e> {
    pub fn new(
        conn: &'c mut Connection,
        fence: &'f TerminalTurnFenceCheck<'f>,
        events: &'e [StateEventInsert<'e>],
    ) -> Self {
        Self {
            conn: RefCell::new(conn),
            fence,
            events,
            error: RefCell::new(None),
        }
    }

    fn take_error(&self) -> Option<anyhow::Error> {
        self.error.borrow_mut().take()
    }

    fn transact(
        &self,
        expected: &TerminalTurnAuthority,
    ) -> Result<EffectApply<TerminalTurnReceipt, TerminalTurnAuthority, String>> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let observed = read_terminal_turn_authority(&tx, self.fence)?;
        if &observed != expected {
            // Dropping the transaction rolls it back: zero rows written.
            return Ok(EffectApply::Superseded { observed });
        }
        let inserted = self
            .events
            .iter()
            .map(|event| insert_state_event_in_db(&tx, event))
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(EffectApply::Applied {
            receipt: inserted,
            authority: observed,
        })
    }
}

impl
    CompareAndApply<TerminalTurnKey, TerminalTurnAuthority, TerminalTurnAppend, TerminalTurnReceipt>
    for TerminalTurnLedgerAppend<'_, '_, '_>
{
    type Error = String;

    fn observe(&self, _key: &TerminalTurnKey) -> Result<TerminalTurnAuthority, String> {
        let conn = self.conn.borrow();
        read_terminal_turn_authority(&conn, self.fence).map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<
            TerminalTurnKey,
            TerminalTurnAuthority,
            TerminalTurnAppend,
            TerminalTurnReceipt,
        >,
    ) -> EffectApply<TerminalTurnReceipt, TerminalTurnAuthority, String> {
        match self.transact(&attempt.expected) {
            Ok(outcome) => outcome,
            Err(error) => {
                let rendered = format!("{error:#}");
                *self.error.borrow_mut() = Some(error);
                EffectApply::Rejected(rendered)
            }
        }
    }
}

/// Append terminal closeout facts as one Lazily-owned effect epoch whose
/// expected authority is `fence`'s cycle and actor with no live turn. A moved
/// authority is `FenceChanged` (Lazily `Superseded`) and writes nothing.
pub fn append_if_terminal_turn(
    conn: &mut Connection,
    fence: &TerminalTurnFenceCheck<'_>,
    events: &[StateEventInsert<'_>],
) -> Result<FencedStateEventInsert> {
    let Some(expected) = fence.expected_authority() else {
        return Ok(FencedStateEventInsert::FenceChanged);
    };
    let key = TerminalTurnKey {
        document_hash: fence.document_hash.to_string(),
    };
    let event_ids = events
        .iter()
        .map(|event| event.event_id.to_string())
        .collect::<Vec<_>>();
    let identity = EffectIdentity::new(format!(
        "terminal-turn-ledger-append:{}:{}:{}#1",
        fence.document_hash,
        fence.expected_cycle_id,
        event_ids.join(",")
    ))
    .map_err(|error| anyhow::anyhow!("invalid terminal ledger effect identity: {error:?}"))?;
    let mut core = TerminalTurnCore::new(GENERATION);
    match core
        .upsert_desired(EffectIntent {
            identity,
            epoch: 1,
            key: key.clone(),
            expected,
            plan: EffectPlan::single(TerminalTurnAppend { event_ids }),
        })
        .1
    {
        EffectCasUpsert::Accepted => {}
        other => anyhow::bail!("terminal ledger append was not accepted: {other:?}"),
    }
    let attempt = match core.claim(&key, GENERATION, NOW).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => anyhow::bail!("terminal ledger append was not claimed: {other:?}"),
    };
    let adapter = TerminalTurnLedgerAppend::new(conn, fence, events);
    let outcome = adapter.compare_and_apply(&attempt);
    let applied = match &outcome {
        EffectApply::Applied { receipt, .. } => Some(receipt.clone()),
        _ => None,
    };
    match (
        core.record_outcome(&key, &attempt.identity, outcome, NOW).1,
        applied,
    ) {
        (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Some(receipt)) => {
            Ok(FencedStateEventInsert::Applied(receipt))
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Superseded), _) => {
            Ok(FencedStateEventInsert::FenceChanged)
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Rejected), _) => Err(adapter
            .take_error()
            .unwrap_or_else(|| anyhow::anyhow!("terminal ledger append was rejected"))),
        (recorded, _) => {
            anyhow::bail!("terminal ledger append outcome diverged from Lazily: {recorded:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// Actor-generation fenced writes.
// ---------------------------------------------------------------------------

/// Opaque authority of one document's actor row: the generation a write
/// supersedes and the exact committed write ([`ActorRevision`]) it was decided
/// against. A row that does not exist has [`ActorRevision::ABSENT`], and its
/// generation is the one the write itself names as its prior
/// (`last_transition.prior_generation`; `0` for a delete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorGenerationAuthority {
    pub generation: u64,
    pub revision: ActorRevision,
}

/// Which document's actor row a fenced write targets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorGenerationKey {
    pub document_id: String,
}

/// An actor record write: the record plus the `documents` bootstrap columns,
/// and, for a lifecycle transition, the time used to settle dispatch receipts
/// when entering `Ready` (`#queuedclearflake`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorRecordStore {
    pub record: ActorRecord,
    pub launch_mode: Option<String>,
    pub controller_epoch: Option<i64>,
    pub lifecycle_as_of: Option<i64>,
}

/// Payload of one actor-generation epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorGenerationMutation {
    /// Write a new actor record.
    Store(Box<ActorRecordStore>),
    /// Compare the binding and correct its physical window without changing lifecycle evidence.
    CorrectWindow {
        window: String,
        caller: String,
        reason: String,
        timestamp: u64,
    },
    /// Hard-delete the record with its history and lease rows (`#actorprune`).
    Delete,
}

/// Receipt of an applied actor-generation epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorGenerationReceipt {
    Stored {
        write: Box<ActorStoreWrite>,
        released_dispatches: usize,
    },
    WindowCorrected,
    Deleted {
        removed: usize,
    },
}

/// A fenced actor write lost to a commit after its decision: Lazily
/// `Superseded`. Its rendering keeps the `generation compare-and-swap failed`
/// text ([`ACTOR_GENERATION_SUPERSEDED_MARKER`]) every caller and the
/// controller IPC error channel already recognize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorGenerationSuperseded {
    pub document_id: String,
    pub delete: bool,
    pub expected: ActorGenerationAuthority,
    pub observed: ActorGenerationAuthority,
}

impl std::fmt::Display for ActorGenerationSuperseded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.delete {
            write!(
                f,
                "controller actor {ACTOR_GENERATION_SUPERSEDED_MARKER} for {}: the record was rewritten after the prune decision (expected revision {:?}, found {:?})",
                self.document_id,
                self.expected.revision.last_transition_id,
                self.observed.revision.last_transition_id
            )
        } else if self.expected.generation != self.observed.generation {
            write!(
                f,
                "controller actor {ACTOR_GENERATION_SUPERSEDED_MARKER} for {}: expected {}, found {}",
                self.document_id, self.expected.generation, self.observed.generation
            )
        } else {
            write!(
                f,
                "controller actor {ACTOR_GENERATION_SUPERSEDED_MARKER} for {}: generation {} was rewritten after the decision (expected revision {:?}, found {:?})",
                self.document_id,
                self.observed.generation,
                self.expected.revision.last_transition_id,
                self.observed.revision.last_transition_id
            )
        }
    }
}

impl std::error::Error for ActorGenerationSuperseded {}

/// Whether `err` is a fenced actor write that lost to a commit after its
/// decision: the typed [`ActorGenerationSuperseded`], or its rendering carried
/// across the controller IPC error channel.
pub fn actor_generation_superseded(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<ActorGenerationSuperseded>().is_some()
            || actor_generation_superseded_detail(&cause.to_string())
    })
}

/// Effect-free read of a document's actor authority. `absent_prior_generation`
/// is the generation a write names as its prior when no row exists.
pub fn read_actor_generation_authority(
    conn: &Connection,
    document_id: &str,
    absent_prior_generation: u64,
) -> Result<(Option<ActorRecord>, ActorGenerationAuthority)> {
    let (record, revision) = load_actor_record_with_revision_from_db(conn, document_id)?;
    let generation = record
        .as_ref()
        .map_or(absent_prior_generation, |record| record.generation);
    Ok((
        record,
        ActorGenerationAuthority {
            generation,
            revision,
        },
    ))
}

type ActorGenerationCore = EffectCasCore<
    ActorGenerationKey,
    ActorGenerationAuthority,
    ActorGenerationMutation,
    ActorGenerationReceipt,
    String,
>;

/// `lazily::CompareAndApply` adapter for one actor-generation fenced write:
/// compare the document's actor authority and apply the mutation in one
/// IMMEDIATE transaction. A mismatch rolls back, writes zero rows, and is
/// `Superseded { observed }`; a SQLite failure or a generation regression is
/// `Rejected`. It never retries and never re-decides.
pub struct ActorGenerationWrite<'c> {
    conn: RefCell<&'c mut Connection>,
    error: RefCell<Option<anyhow::Error>>,
}

impl<'c> ActorGenerationWrite<'c> {
    pub fn new(conn: &'c mut Connection) -> Self {
        Self {
            conn: RefCell::new(conn),
            error: RefCell::new(None),
        }
    }

    fn take_error(&self) -> Option<anyhow::Error> {
        self.error.borrow_mut().take()
    }

    fn transact(
        &self,
        key: &ActorGenerationKey,
        expected: &ActorGenerationAuthority,
        mutation: &ActorGenerationMutation,
    ) -> Result<EffectApply<ActorGenerationReceipt, ActorGenerationAuthority, String>> {
        let mut conn = self.conn.borrow_mut();
        // Reserve the writer slot before reading: a deferred transaction can
        // fail its read-to-write upgrade with SQLITE_BUSY immediately.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (previous, observed) = read_actor_generation_authority(
            &tx,
            &key.document_id,
            absent_prior_generation(mutation),
        )?;
        if &observed != expected {
            // Dropping the transaction rolls it back: zero rows written.
            return Ok(EffectApply::Superseded { observed });
        }
        let receipt = match mutation {
            ActorGenerationMutation::Store(store) => {
                let (write, released_dispatches) = apply_actor_record_write_in_tx(
                    &tx,
                    previous.as_ref(),
                    observed.generation,
                    &store.record,
                    store.launch_mode.clone(),
                    store.controller_epoch,
                    store.lifecycle_as_of,
                )?;
                ActorGenerationReceipt::Stored {
                    write: Box::new(write),
                    released_dispatches,
                }
            }
            ActorGenerationMutation::CorrectWindow {
                window,
                caller,
                reason,
                timestamp,
            } => {
                let previous = previous.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot correct the window of an absent actor {}",
                        key.document_id
                    )
                })?;
                if !window.is_empty() && *window != previous.window_id {
                    let mut corrected = previous.clone();
                    corrected.window_id.clone_from(window);
                    corrected.last_transition = agent_doc_controller::actor::ActorLastTransition {
                        caller: caller.clone(),
                        reason: reason.clone(),
                        timestamp: *timestamp,
                        prior_generation: previous.generation,
                        new_generation: previous.generation,
                    };
                    let transition_id = insert_actor_transition(&tx, Some(previous), &corrected)?;
                    tx.execute(
                        "UPDATE documents SET window_id = ?2, window_transition_id = ?3 WHERE document_id = ?1",
                        params![key.document_id, window, transition_id],
                    )?;
                }
                ActorGenerationReceipt::WindowCorrected
            }
            ActorGenerationMutation::Delete => ActorGenerationReceipt::Deleted {
                removed: delete_actor_document_rows_in_tx(&tx, &key.document_id)?,
            },
        };
        tx.commit()?;
        Ok(EffectApply::Applied {
            receipt,
            authority: observed,
        })
    }
}

fn absent_prior_generation(mutation: &ActorGenerationMutation) -> u64 {
    match mutation {
        ActorGenerationMutation::Store(store) => store.record.last_transition.prior_generation,
        ActorGenerationMutation::CorrectWindow { .. } | ActorGenerationMutation::Delete => 0,
    }
}

impl
    CompareAndApply<
        ActorGenerationKey,
        ActorGenerationAuthority,
        ActorGenerationMutation,
        ActorGenerationReceipt,
    > for ActorGenerationWrite<'_>
{
    type Error = String;

    fn observe(&self, key: &ActorGenerationKey) -> Result<ActorGenerationAuthority, String> {
        let conn = self.conn.borrow();
        read_actor_generation_authority(&conn, &key.document_id, 0)
            .map(|(_, authority)| authority)
            .map_err(|error| format!("{error:#}"))
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<
            ActorGenerationKey,
            ActorGenerationAuthority,
            ActorGenerationMutation,
            ActorGenerationReceipt,
        >,
    ) -> EffectApply<ActorGenerationReceipt, ActorGenerationAuthority, String> {
        match self.transact(&attempt.key, &attempt.expected, &attempt.payload) {
            Ok(outcome) => outcome,
            Err(error) => {
                let rendered = format!("{error:#}");
                *self.error.borrow_mut() = Some(error);
                EffectApply::Rejected(rendered)
            }
        }
    }
}

/// Run one actor-generation fenced write as a Lazily-owned effect epoch whose
/// expected authority is the decision's `expected`. A moved authority is
/// Lazily `Superseded`, writes nothing, and returns [`ActorGenerationSuperseded`].
pub fn write_if_actor_generation(
    conn: &mut Connection,
    document_id: &str,
    expected: ActorGenerationAuthority,
    mutation: ActorGenerationMutation,
) -> Result<ActorGenerationReceipt> {
    let key = ActorGenerationKey {
        document_id: document_id.to_string(),
    };
    let operation = match mutation {
        ActorGenerationMutation::Store(_) => "store",
        ActorGenerationMutation::CorrectWindow { .. } => "window",
        ActorGenerationMutation::Delete => "delete",
    };
    let delete = matches!(mutation, ActorGenerationMutation::Delete);
    let identity = EffectIdentity::new(format!(
        "actor-generation-write:{document_id}:{}:{:?}:{operation}#1",
        expected.generation, expected.revision.last_transition_id,
    ))
    .map_err(|error| anyhow::anyhow!("invalid actor generation effect identity: {error:?}"))?;
    let mut core = ActorGenerationCore::new(GENERATION);
    match core
        .upsert_desired(EffectIntent {
            identity,
            epoch: 1,
            key: key.clone(),
            expected,
            plan: EffectPlan::single(mutation),
        })
        .1
    {
        EffectCasUpsert::Accepted => {}
        other => anyhow::bail!("actor generation write was not accepted: {other:?}"),
    }
    let attempt = match core.claim(&key, GENERATION, NOW).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => anyhow::bail!("actor generation write was not claimed: {other:?}"),
    };
    let adapter = ActorGenerationWrite::new(conn);
    let outcome = adapter.compare_and_apply(&attempt);
    let (applied, observed) = match &outcome {
        EffectApply::Applied { receipt, .. } => (Some(receipt.clone()), None),
        EffectApply::Superseded { observed } => (None, Some(*observed)),
        _ => (None, None),
    };
    match (
        core.record_outcome(&key, &attempt.identity, outcome, NOW).1,
        applied,
        observed,
    ) {
        (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Some(receipt), _) => {
            Ok(receipt)
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Superseded), _, Some(observed)) => {
            Err(anyhow::Error::new(ActorGenerationSuperseded {
                document_id: document_id.to_string(),
                delete,
                expected,
                observed,
            }))
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Rejected), _, _) => Err(adapter
            .take_error()
            .unwrap_or_else(|| anyhow::anyhow!("actor generation write was rejected"))),
        (recorded, _, _) => {
            anyhow::bail!("actor generation write outcome diverged from Lazily: {recorded:?}")
        }
    }
}

/// [`write_if_actor_generation`] for an actor record write.
pub fn store_actor_record_if_generation(
    conn: &mut Connection,
    expected: ActorGenerationAuthority,
    store: ActorRecordStore,
) -> Result<(ActorStoreWrite, usize)> {
    let document_id = store.record.document_id.clone();
    match write_if_actor_generation(
        conn,
        &document_id,
        expected,
        ActorGenerationMutation::Store(Box::new(store)),
    )? {
        ActorGenerationReceipt::Stored {
            write,
            released_dispatches,
        } => Ok((*write, released_dispatches)),
        other => anyhow::bail!("actor record store returned {other:?}"),
    }
}

/// [`write_if_actor_generation`] for an `#actorprune` delete.
pub fn delete_actor_document_if_generation(
    conn: &mut Connection,
    document_id: &str,
    expected: ActorGenerationAuthority,
) -> Result<usize> {
    match write_if_actor_generation(conn, document_id, expected, ActorGenerationMutation::Delete)? {
        ActorGenerationReceipt::Deleted { removed } => Ok(removed),
        other => anyhow::bail!("actor record delete returned {other:?}"),
    }
}

/// One decision of a re-deciding actor-generation producer.
#[derive(Debug)]
pub enum ActorGenerationDecision<S, W> {
    /// Nothing to write (the observed record no longer needs it).
    Settled(S),
    /// Write `write`, fenced on the decision's observed `authority`.
    Write {
        authority: ActorGenerationAuthority,
        write: W,
    },
}

/// Terminal state of [`drive_actor_generation_decisions`], carrying the
/// decided write back to the producer.
#[derive(Debug)]
pub enum ActorGenerationDrive<S, W, R> {
    Settled(S),
    Applied {
        write: W,
        receipt: R,
    },
    /// Every decision lost to a newer commit; `error` is the last supersession.
    Exhausted {
        decisions: u32,
        write: W,
        error: anyhow::Error,
    },
}

/// Payload of one re-decision epoch: the decision number (the write itself is
/// held by the producer, which rebuilds it from each fresh observation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorGenerationDecisionStep {
    pub decision: u32,
}

type ActorGenerationDecisionCore = EffectCasCore<
    ActorGenerationKey,
    ActorGenerationAuthority,
    ActorGenerationDecisionStep,
    (),
    String,
>;

/// Drive a re-deciding actor-generation producer through Lazily.
///
/// Each decision is one epoch: `decide` reads the record and returns the write
/// with the authority it observed, and `write` performs that fenced write (the
/// local [`write_if_actor_generation`] or a controller request carrying the
/// same expectation). A lost fence ([`actor_generation_superseded`]) is Lazily
/// `Superseded`; while decisions remain, the producer re-decides from a fresh
/// read as the next epoch, and `on_superseded(decision, redecide, error)` is
/// called for every loss. Any other write error is a Lazily rejection and is
/// returned. The producer never loops on its own.
#[allow(clippy::type_complexity)]
pub fn drive_actor_generation_decisions<S, W, R>(
    document_id: &str,
    max_decisions: u32,
    decide: &mut dyn FnMut(u32) -> Result<ActorGenerationDecision<S, W>>,
    write: &mut dyn FnMut(&ActorGenerationAuthority, &W) -> Result<R>,
    on_superseded: &mut dyn FnMut(u32, bool, &W, &anyhow::Error),
) -> Result<ActorGenerationDrive<S, W, R>> {
    let key = ActorGenerationKey {
        document_id: document_id.to_string(),
    };
    let max_decisions = max_decisions.max(1);
    let mut core = ActorGenerationDecisionCore::new(GENERATION);
    for decision in 1..=max_decisions {
        let (authority, pending) = match decide(decision)? {
            ActorGenerationDecision::Settled(settled) => {
                return Ok(ActorGenerationDrive::Settled(settled));
            }
            ActorGenerationDecision::Write { authority, write } => (authority, write),
        };
        let identity = EffectIdentity::new(format!(
            "actor-generation-decision:{document_id}#{decision}"
        ))
        .map_err(|error| anyhow::anyhow!("invalid actor generation effect identity: {error:?}"))?;
        match core
            .upsert_desired(EffectIntent {
                identity,
                epoch: u64::from(decision),
                key: key.clone(),
                expected: authority,
                plan: EffectPlan::single(ActorGenerationDecisionStep { decision }),
            })
            .1
        {
            EffectCasUpsert::Accepted => {}
            other => anyhow::bail!("actor generation decision was not accepted: {other:?}"),
        }
        let attempt = match core.claim(&key, GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => attempt,
            other => anyhow::bail!("actor generation decision was not claimed: {other:?}"),
        };
        let (outcome, result) = match write(&attempt.expected, &pending) {
            Ok(receipt) => (
                EffectApply::Applied {
                    receipt: (),
                    authority: attempt.expected,
                },
                Ok(receipt),
            ),
            Err(error) if actor_generation_superseded(&error) => {
                // A local loss carries the observed authority; a loss rendered
                // across the controller IPC channel does not, and the next
                // decision re-reads it.
                let observed = error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<ActorGenerationSuperseded>())
                    .map_or(attempt.expected, |lost| lost.observed);
                (EffectApply::Superseded { observed }, Err(error))
            }
            Err(error) => (EffectApply::Rejected(format!("{error:#}")), Err(error)),
        };
        match (
            core.record_outcome(&key, &attempt.identity, outcome, NOW).1,
            result,
        ) {
            (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Ok(receipt)) => {
                return Ok(ActorGenerationDrive::Applied {
                    write: pending,
                    receipt,
                });
            }
            (EffectCasOutcome::Failure(EffectCasFailure::Superseded), Err(error)) => {
                let redecide = decision < max_decisions;
                on_superseded(decision, redecide, &pending, &error);
                if !redecide {
                    return Ok(ActorGenerationDrive::Exhausted {
                        decisions: max_decisions,
                        write: pending,
                        error,
                    });
                }
            }
            (EffectCasOutcome::Failure(EffectCasFailure::Rejected), Err(error)) => {
                return Err(error);
            }
            (recorded, _) => {
                anyhow::bail!("actor generation decision diverged from Lazily: {recorded:?}")
            }
        }
    }
    unreachable!("the final decision always returns")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::{
        CoordinationLeaseRecord, insert_actor_transition, load_state_events_from_db, open_state_db,
        upsert_actor_document, upsert_coordination_lease_in_db,
    };
    use agent_doc_controller::actor::{ActorLastTransition, ActorState};

    const DOCUMENT_ID: &str = "tasks/ledger-cas.md";
    const DOCUMENT_HASH: &str = "ledger-cas-document-hash";

    fn checkpoint(conn: &Connection, event_id: &str, cycle_id: &str) -> Result<()> {
        let payload = serde_json::json!({
            "event_id": event_id,
            "fact": {
                "type": "turn_intent_checkpointed",
                "document_hash": DOCUMENT_HASH,
                "cycle_id": cycle_id,
                "checkpoint_sequence": 1,
                "state_sha256": "state",
                "state_json": "{}"
            }
        })
        .to_string();
        insert_state_event_in_db(
            conn,
            &StateEventInsert {
                event_id,
                document_hash: DOCUMENT_HASH,
                domain: "closeout",
                fact_type: "turn_intent_checkpointed",
                payload_json: &payload,
            },
        )?;
        Ok(())
    }

    fn bind_actor(conn: &Connection, generation: u64) -> Result<()> {
        let actor = ActorRecord {
            document_id: DOCUMENT_ID.to_string(),
            session_id: "session-ledger".to_string(),
            generation,
            pane_id: "%88".to_string(),
            window_id: "@8".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: ActorLastTransition {
                caller: "test".to_string(),
                reason: "initial_binding".to_string(),
                timestamp: 100,
                prior_generation: generation.saturating_sub(1),
                new_generation: generation,
            },
        };
        let transition = insert_actor_transition(conn, None, &actor)?;
        upsert_actor_document(conn, &actor, transition, None, None)?;
        Ok(())
    }

    fn fence(generation: Option<u64>) -> TerminalTurnFenceCheck<'static> {
        TerminalTurnFenceCheck {
            document_id: DOCUMENT_ID,
            document_hash: DOCUMENT_HASH,
            expected_cycle_id: "cycle-1",
            session_id: generation.map(|_| "session-ledger"),
            pane_id: generation.map(|_| "%88"),
            generation,
            active_after_secs: 150,
        }
    }

    fn terminal_payload() -> String {
        serde_json::json!({
            "event_id": "terminal-commit",
            "fact": {
                "type": "commit_observed",
                "document_hash": DOCUMENT_HASH,
                "cycle_id": "cycle-1",
                "commit": "deadbeef"
            }
        })
        .to_string()
    }

    fn terminal(payload: &str) -> [StateEventInsert<'_>; 1] {
        [StateEventInsert {
            event_id: "terminal-commit",
            document_hash: DOCUMENT_HASH,
            domain: "closeout",
            fact_type: "commit_observed",
            payload_json: payload,
        }]
    }

    fn event_ids(conn: &Connection) -> Result<Vec<String>> {
        Ok(load_state_events_from_db(conn, Some(DOCUMENT_HASH))?
            .into_iter()
            .map(|event| event.event_id)
            .collect())
    }

    #[test]
    fn matching_authority_applies_and_a_repeat_is_idempotent() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut conn = open_state_db(dir.path())?;
        checkpoint(&conn, "checkpoint-1", "cycle-1")?;
        bind_actor(&conn, 7)?;
        let payload = terminal_payload();
        assert_eq!(
            append_if_terminal_turn(&mut conn, &fence(Some(7)), &terminal(&payload))?,
            FencedStateEventInsert::Applied(vec![true])
        );
        assert_eq!(
            append_if_terminal_turn(&mut conn, &fence(Some(7)), &terminal(&payload))?,
            FencedStateEventInsert::Applied(vec![false]),
            "the same event id is an idempotent repeat"
        );
        assert_eq!(event_ids(&conn)?, vec!["checkpoint-1", "terminal-commit"]);
        Ok(())
    }

    /// Superseded contract: a moved cycle, a rebound actor generation, or a
    /// live turn is Lazily `Superseded` and the ledger is unchanged.
    #[test]
    fn changed_authority_is_superseded_and_leaves_the_ledger_unchanged() -> Result<()> {
        type Drift = fn(&Connection) -> Result<()>;
        let drifts: [(&str, Drift); 3] = [
            ("cycle", |conn| checkpoint(conn, "checkpoint-2", "cycle-2")),
            ("actor", |conn| bind_actor(conn, 8)),
            ("live_turn", |conn| {
                upsert_coordination_lease_in_db(
                    conn,
                    &CoordinationLeaseRecord {
                        scope_kind: "turn_active".to_string(),
                        scope_id: "%88".to_string(),
                        holder: "%88".to_string(),
                        holder_pid: Some(std::process::id()),
                        heartbeat_secs: 200,
                    },
                )
            }),
        ];
        for (label, drift) in drifts {
            let dir = tempfile::tempdir()?;
            let mut conn = open_state_db(dir.path())?;
            checkpoint(&conn, "checkpoint-1", "cycle-1")?;
            bind_actor(&conn, 7)?;
            let expected = fence(Some(7)).expected_authority().unwrap();
            assert_eq!(
                read_terminal_turn_authority(&conn, &fence(Some(7)))?,
                expected
            );
            drift(&conn)?;
            let observed = read_terminal_turn_authority(&conn, &fence(Some(7)))?;
            assert_ne!(observed, expected, "{label}: the drift moved the authority");
            let before = event_ids(&conn)?;
            let payload = terminal_payload();
            assert_eq!(
                append_if_terminal_turn(&mut conn, &fence(Some(7)), &terminal(&payload))?,
                FencedStateEventInsert::FenceChanged,
                "{label}"
            );
            assert_eq!(event_ids(&conn)?, before, "{label}: zero rows written");
        }
        Ok(())
    }

    #[test]
    fn adapter_reports_superseded_with_the_observed_authority() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut conn = open_state_db(dir.path())?;
        checkpoint(&conn, "checkpoint-1", "cycle-1")?;
        checkpoint(&conn, "checkpoint-2", "cycle-2")?;
        let payload = terminal_payload();
        let events = terminal(&payload);
        let fence = fence(None);
        let expected = fence.expected_authority().unwrap();
        let adapter = TerminalTurnLedgerAppend::new(&mut conn, &fence, &events);
        let key = TerminalTurnKey {
            document_hash: DOCUMENT_HASH.to_string(),
        };
        let observed = adapter.observe(&key).unwrap();
        assert_eq!(observed.cycle_id.as_deref(), Some("cycle-2"));
        match adapter.transact(&expected)? {
            EffectApply::Superseded { observed: reported } => assert_eq!(reported, observed),
            other => panic!("expected Superseded, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn a_partial_actor_fence_never_matches_and_writes_nothing() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut conn = open_state_db(dir.path())?;
        checkpoint(&conn, "checkpoint-1", "cycle-1")?;
        let mut partial = fence(None);
        partial.pane_id = Some("%88");
        assert_eq!(partial.expected_authority(), None);
        let payload = terminal_payload();
        assert_eq!(
            append_if_terminal_turn(&mut conn, &partial, &terminal(&payload))?,
            FencedStateEventInsert::FenceChanged
        );
        assert_eq!(event_ids(&conn)?, vec!["checkpoint-1"]);
        Ok(())
    }

    fn actor_rows(conn: &Connection) -> Result<Vec<(String, i64, Option<i64>)>> {
        let mut statement = conn.prepare(
            "SELECT document_id, generation, last_transition_id FROM documents ORDER BY document_id",
        )?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn actor_store(generation: u64, state: ActorState) -> ActorRecordStore {
        ActorRecordStore {
            record: ActorRecord {
                document_id: DOCUMENT_ID.to_string(),
                session_id: "session-ledger".to_string(),
                generation,
                pane_id: "%88".to_string(),
                window_id: "@8".to_string(),
                harness: "codex".to_string(),
                state,
                last_transition: ActorLastTransition {
                    caller: "test".to_string(),
                    reason: "fenced".to_string(),
                    timestamp: 100,
                    prior_generation: generation.saturating_sub(1),
                    new_generation: generation,
                },
            },
            launch_mode: None,
            controller_epoch: None,
            lifecycle_as_of: None,
        }
    }

    fn observed_actor(conn: &Connection) -> Result<ActorGenerationAuthority> {
        Ok(read_actor_generation_authority(conn, DOCUMENT_ID, 0)?.1)
    }

    /// Superseded contract (`#implementlazilyowned` actor-generation family): a
    /// moved generation, a same-generation rewrite, or a re-opened record is
    /// Lazily `Superseded`, leaves every actor row unchanged, and renders the
    /// `generation compare-and-swap failed` text callers re-decide on.
    #[test]
    fn changed_actor_authority_is_superseded_and_leaves_the_actor_rows_unchanged() -> Result<()> {
        type Drift = fn(&Connection) -> Result<()>;
        let drifts: [(&str, Drift, bool); 3] = [
            ("generation", |conn| bind_actor(conn, 8), false),
            (
                "same-generation rewrite",
                |conn| {
                    let mut conn_store = actor_store(7, ActorState::Busy);
                    conn_store.record.last_transition.prior_generation = 7;
                    let transition = insert_actor_transition(conn, None, &conn_store.record)?;
                    upsert_actor_document(conn, &conn_store.record, transition, None, None)?;
                    Ok(())
                },
                false,
            ),
            ("re-opened before prune", |conn| bind_actor(conn, 7), true),
        ];
        for (label, drift, delete) in drifts {
            let dir = tempfile::tempdir()?;
            let mut conn = open_state_db(dir.path())?;
            bind_actor(&conn, 7)?;
            let expected = observed_actor(&conn)?;
            drift(&conn)?;
            let observed = observed_actor(&conn)?;
            assert_ne!(observed, expected, "{label}: the drift moved the authority");
            let before = actor_rows(&conn)?;
            let err = if delete {
                delete_actor_document_if_generation(&mut conn, DOCUMENT_ID, expected)
                    .expect_err("a superseded delete fails")
            } else {
                store_actor_record_if_generation(
                    &mut conn,
                    expected,
                    actor_store(9, ActorState::Ready),
                )
                .expect_err("a superseded store fails")
            };
            let lost = err
                .downcast_ref::<ActorGenerationSuperseded>()
                .unwrap_or_else(|| panic!("{label}: typed supersession, got {err:#}"));
            assert_eq!(
                (lost.expected, lost.observed),
                (expected, observed),
                "{label}"
            );
            assert!(actor_generation_superseded(&err), "{label}");
            assert!(
                err.to_string().contains(ACTOR_GENERATION_SUPERSEDED_MARKER),
                "{label}: {err}"
            );
            assert_eq!(actor_rows(&conn)?, before, "{label}: zero rows written");
        }
        Ok(())
    }

    #[test]
    fn matching_actor_authority_applies_the_store_and_the_delete() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut conn = open_state_db(dir.path())?;
        let absent = ActorGenerationAuthority {
            generation: 6,
            revision: ActorRevision::ABSENT,
        };
        let (write, released) =
            store_actor_record_if_generation(&mut conn, absent, actor_store(7, ActorState::Ready))?;
        assert_eq!((write.record.generation, released), (7, 0));
        let current = observed_actor(&conn)?;
        assert_eq!(current.generation, 7);
        assert_eq!(
            delete_actor_document_if_generation(&mut conn, DOCUMENT_ID, current)?,
            1
        );
        assert!(actor_rows(&conn)?.is_empty());
        Ok(())
    }

    #[test]
    fn generation_regression_is_rejected_not_superseded() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut conn = open_state_db(dir.path())?;
        bind_actor(&conn, 7)?;
        let current = observed_actor(&conn)?;
        let before = actor_rows(&conn)?;
        let err =
            store_actor_record_if_generation(&mut conn, current, actor_store(6, ActorState::Ready))
                .expect_err("a regression is rejected");
        assert!(!actor_generation_superseded(&err), "{err:#}");
        assert!(err.to_string().contains("generation regression"), "{err:#}");
        assert_eq!(actor_rows(&conn)?, before);
        Ok(())
    }

    /// The re-decision driver: a lost fence is Lazily `Superseded` and the
    /// producer re-decides from a fresh read as the next epoch; exhaustion
    /// returns the last loss and leaves the winner in place.
    #[test]
    fn drive_redecides_a_superseded_write_as_the_next_epoch() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let conn = RefCell::new(open_state_db(dir.path())?);
        bind_actor(&conn.borrow(), 7)?;
        let mut decisions = Vec::new();
        let mut losses = Vec::new();
        let drive = drive_actor_generation_decisions(
            DOCUMENT_ID,
            3,
            &mut |decision| {
                let authority = observed_actor(&conn.borrow())?;
                decisions.push((decision, authority.generation));
                if decision == 1 {
                    // A newer commit lands between the decision and the write.
                    bind_actor(&conn.borrow(), 8)?;
                }
                Ok(ActorGenerationDecision::<(), _>::Write {
                    authority,
                    write: actor_store(authority.generation + 1, ActorState::Ready),
                })
            },
            &mut |authority, store| {
                store_actor_record_if_generation(&mut conn.borrow_mut(), *authority, store.clone())
            },
            &mut |decision, redecide, _, _| losses.push((decision, redecide)),
        )?;
        let ActorGenerationDrive::Applied {
            receipt: (write, _),
            ..
        } = drive
        else {
            panic!("expected Applied, got {drive:?}");
        };
        assert_eq!(write.record.generation, 9);
        assert_eq!(decisions, vec![(1, 7), (2, 8)]);
        assert_eq!(losses, vec![(1, true)]);

        let mut losses = Vec::new();
        let drive = drive_actor_generation_decisions(
            DOCUMENT_ID,
            2,
            &mut |_| {
                let authority = observed_actor(&conn.borrow())?;
                bind_actor(&conn.borrow(), authority.generation + 1)?;
                Ok(ActorGenerationDecision::<(), _>::Write {
                    authority,
                    write: actor_store(20, ActorState::Ready),
                })
            },
            &mut |authority, store| {
                store_actor_record_if_generation(&mut conn.borrow_mut(), *authority, store.clone())
            },
            &mut |decision, redecide, _, _| losses.push((decision, redecide)),
        )?;
        let ActorGenerationDrive::Exhausted {
            decisions, error, ..
        } = drive
        else {
            panic!("expected Exhausted, got {drive:?}");
        };
        assert_eq!(decisions, 2);
        assert!(actor_generation_superseded(&error));
        assert_eq!(losses, vec![(1, true), (2, false)]);
        assert_eq!(
            observed_actor(&conn.borrow())?.generation,
            11,
            "the winner stays"
        );
        Ok(())
    }

    #[test]
    fn drive_settles_without_writing_and_returns_a_rejection_without_redeciding() -> Result<()> {
        let drive = drive_actor_generation_decisions::<&str, (), ()>(
            DOCUMENT_ID,
            3,
            &mut |_| Ok(ActorGenerationDecision::Settled("kept")),
            &mut |_, _| unreachable!("a settled decision never writes"),
            &mut |_, _, _, _| unreachable!(),
        )?;
        assert!(matches!(drive, ActorGenerationDrive::Settled("kept")));

        let mut decided = 0;
        let err = drive_actor_generation_decisions::<(), (), ()>(
            DOCUMENT_ID,
            3,
            &mut |_| {
                decided += 1;
                Ok(ActorGenerationDecision::Write {
                    authority: ActorGenerationAuthority {
                        generation: 1,
                        revision: ActorRevision::ABSENT,
                    },
                    write: (),
                })
            },
            &mut |_, _| anyhow::bail!("controller unavailable"),
            &mut |_, _, _, _| unreachable!("a rejection is not a supersession"),
        )
        .expect_err("a rejection is returned");
        assert_eq!(decided, 1);
        assert!(err.to_string().contains("controller unavailable"));
        Ok(())
    }

    #[test]
    fn a_supersession_rendered_across_ipc_is_still_classified() {
        let remote = anyhow::anyhow!(
            "controller request failed: controller actor generation compare-and-swap failed for doc: expected 1, found 2"
        );
        assert!(actor_generation_superseded(&remote));
        assert!(!actor_generation_superseded(&anyhow::anyhow!("io error")));
    }
}
