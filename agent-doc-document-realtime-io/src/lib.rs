//! # Module: realtime_model
//!
//! ## Spec (`#rtwatch` — realtime editor-buffer ↔ disk read authority)
//! The agent-doc cycle (`preflight` / `write` / `finalize` / `session-check`)
//! resolves the "current document" through the active editor authority first.
//! When an editor (IDEA / VS Code) owns the document, the CRDT relay canonical
//! text is authoritative and disk is only a non-authoritative replica. Disk is
//! read as the fallback replica only after the relay reports that no editor is
//! attached.
//!
//! `agent-doc-document-realtime` still owns the deterministic pure policy for
//! reconciling a trusted editor buffer against disk. This crate owns the live
//! relay/disk adapter and ops-log/CP effects. Cycle read sites (`preflight.rs` / `write.rs` /
//! `session_check.rs`) source current-doc through
//! [`try_resolve_current_doc_from_file`].
//!
//! ## Evals
//! - `durable_buffer_state_none_when_buffer_in_sync_with_disk`
//! - `durable_buffer_state_wins_when_unsaved_buffer_ahead_of_disk`
//! - `durable_buffer_state_none_when_no_editor_feed`
//! - `repair_cas_projects_retained_target_when_editor_owner_has_zero_replicas`

use parking_lot::Mutex;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use lazily::{DeadlineCore, TimelineSource};
use std::path::{Path, PathBuf};

use agent_doc_document_realtime::{
    BufferState, Reconciliation, reconcile_current_doc,
    write_policy::{self, VisibleWriteReconcile},
};
pub use agent_doc_document_realtime::{CurrentDocument, DocumentKey};
pub use agent_doc_state_backbone::DocumentWriteDeferredReason;
use agent_doc_turn::op_log::OpsLogEvent;

mod current_document_projection;
pub use current_document_projection::{
    CurrentDocumentProjection, QueueDocumentProjection, invalidate_current_document_projection,
    with_current_document_projection_pass,
};

/// Wall-clock seconds (since UNIX epoch) of the last controller RPC failure
/// observed by [`observe_live_editor_authority`] (the controller-timeout path).
/// Hot polling paths — notably the supervisor idle-queue watch — read this via
/// [`controller_failed_within`] to back off a degraded controller instead of
/// paying the full read timeout on every poll and saturating it further
/// (`#idlewatchctrlbackoff`). 0 means "no failure observed yet".
static LAST_CONTROLLER_DEGRADED_SECS: AtomicI64 = AtomicI64::new(0);

thread_local! {
    /// True only while a CP runtime effect is executing a document mutation.
    /// Controller-owned operations must use the in-process relay rather than
    /// enqueueing RPCs back through the same controller socket.
    static CONTROLLER_DOCUMENT_MUTATION: Cell<bool> = const { Cell::new(false) };
}

pub fn with_controller_document_mutation<T>(f: impl FnOnce() -> T) -> T {
    CONTROLLER_DOCUMENT_MUTATION.with(|slot| {
        let previous = slot.replace(true);
        let _owner = agent_doc_document_realtime::write_authority::owner_scope_guard();
        let result = f();
        slot.set(previous);
        result
    })
}

fn controller_document_mutation_in_progress() -> bool {
    CONTROLLER_DOCUMENT_MUTATION.with(Cell::get)
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn record_controller_degraded() {
    LAST_CONTROLLER_DEGRADED_SECS.store(unix_secs(), Ordering::Relaxed);
}

/// True when a controller RPC failed within the last `window`. Lets callers
/// (the idle-queue watch) skip controller-bound reads while the controller is
/// wedged. An attached editor remains authoritative while reads are suppressed;
/// callers must pause rather than consult disk.
pub fn controller_failed_within(window: std::time::Duration) -> bool {
    let last = LAST_CONTROLLER_DEGRADED_SECS.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    unix_secs().saturating_sub(last) <= window.as_secs().max(1) as i64
}

#[cfg(test)]
use agent_doc_crdt_relay_io::deregister_replica_for_file as test_support_deregister_replica_for_file;
#[cfg(any(test, feature = "test-support"))]
use agent_doc_crdt_relay_io::ensure_document_model as test_support_ensure_document_model;
#[cfg(test)]
use agent_doc_crdt_relay_io::register_replica_for_file as test_support_register_replica_for_file;
#[cfg(test)]
use agent_doc_crdt_relay_io::{
    observe_replica_projection_for_file as test_support_observe_replica_projection_for_file,
    pull_replica_updates_for_file as test_support_pull_replica_updates_for_file,
};

static DOCUMENT_AUTHORITY_EPOCH: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_DISK_WRITE_EPOCH: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_WRITE_INTENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_AUTHORITY_OBSERVATIONS: LazyLock<
    Mutex<HashMap<PathBuf, DocumentAuthorityObservation>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
const CRDT_POST_PROOF_REBASE_LIMIT: usize = 3;

#[cfg(test)]
#[cfg(test)]
const CRDT_WRITE_SETTLE_MS: u64 = 10;
#[cfg(not(test))]
const CRDT_WRITE_SETTLE_MS: u64 = 500;
/// Marker every error that RETAINS its change for a later retry must carry.
///
/// `#fzmutloss`: a write that fails but retains its intent must also retain the
/// SAME closeout's backlog/status mutations, or the response lands on retry
/// while its `--done` is silently dropped — which is exactly what a CRDT
/// convergence timeout used to do. The classifier in `agent-doc-write-runtime-io`
/// keys off this shared constant instead of guessing from prose, so a producer
/// and its classifier cannot drift.
pub const RETAINED_FOR_RETRY_MARKER: &str = "pending change retained for retry";

#[cfg(test)]
// Leave enough scheduler headroom for the ACK-recovery clock to start after
// the initial relay/model work on slower shared CI runners.
const CRDT_WRITE_CONVERGENCE_TIMEOUT_MS: u64 = 5_000;
#[cfg(not(test))]
const CRDT_WRITE_CONVERGENCE_TIMEOUT_MS: u64 = 60_000;
const CRDT_WRITE_BACKOFF_INITIAL_MS: u64 = 25;
const CRDT_WRITE_BACKOFF_MAX_MS: u64 = 250;
/// `#crdtcasraced`: shared pure backoff policy for the convergence loop.
const CRDT_WRITE_BACKOFF_POLICY: agent_doc_document_realtime::convergence_gate::CrdtWriteBackoff =
    agent_doc_document_realtime::convergence_gate::CrdtWriteBackoff::new(
        CRDT_WRITE_BACKOFF_INITIAL_MS,
        CRDT_WRITE_BACKOFF_MAX_MS,
    );
const CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS: u64 = 25;
const CRDT_PROJECTION_FALLBACK_BACKOFF_MAX_MS: u64 = 250;
const CRDT_PROJECTION_FALLBACK_BACKOFF_POLICY:
    agent_doc_document_realtime::convergence_gate::CrdtWriteBackoff =
    agent_doc_document_realtime::convergence_gate::CrdtWriteBackoff::new(
        CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS,
        CRDT_PROJECTION_FALLBACK_BACKOFF_MAX_MS,
    );
#[cfg(test)]
const CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS: u64 = 1_800;
#[cfg(not(test))]
const CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS: u64 = 8_000;
const _: () =
    assert!(CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS > CRDT_PROJECTION_FALLBACK_BACKOFF_MAX_MS);
#[cfg(test)]
const ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS: u64 = 100;
#[cfg(not(test))]
const ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS: u64 = 10_000;
#[derive(Debug)]
pub struct AwaitEditorReplicaNoDiskWrite(String);

impl std::fmt::Display for AwaitEditorReplicaNoDiskWrite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AwaitEditorReplicaNoDiskWrite {}

fn await_editor_replica_no_disk_write(message: String) -> anyhow::Error {
    AwaitEditorReplicaNoDiskWrite(message).into()
}

/// The shared retained-write remedy for this crate's refusals.
///
/// See `agent_doc_turn::write_ownership`. The point is that this site does not
/// get to decide on its own whether waiting is correct.
fn retained_write_remedy_for(file: &Path) -> String {
    agent_doc_turn::write_ownership::retained_write_remedy(
        agent_doc_capture_io::retained_write_ownership(file),
        &file.display().to_string(),
    )
}

/// Build a retained-write refusal that always names its recovery.
///
/// `#retainedwriteremedy`: appending the remedy at each call site is a rule that
/// only holds while every author remembers it — and one branch did not, emitting
/// "no secondary snapshot/commit or forced disk write was attempted" for a write
/// that had already applied and needed only `agent-doc commit <FILE>`. An agent
/// reading that as a lost response re-sends it, which `#percellconverge` forbids.
/// Routing construction through one function makes the remedy structural instead
/// of remembered.
fn retained_refusal(file: &Path, message: String) -> anyhow::Error {
    await_editor_replica_no_disk_write(format!(
        "{message} {}",
        retained_write_remedy_for(file)
    ))
}

#[derive(Debug)]
struct ForceDiskAuthorityChanged(String);

impl std::fmt::Display for ForceDiskAuthorityChanged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ForceDiskAuthorityChanged {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForceDiskMutationFence {
    NoRegisteredEditorReplica,
    RegisteredEditorReplica,
}

#[derive(Debug)]
struct ActiveForceDiskMutationBaseline {
    fence: ForceDiskMutationFence,
    owner: std::thread::ThreadId,
    holders: usize,
}

static FORCE_DISK_MUTATION_BASELINES: LazyLock<
    Mutex<HashMap<PathBuf, ActiveForceDiskMutationBaseline>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Captures editor authority at the explicit force-disk authorization boundary.
/// The final atomic mutation compares against this baseline so a relay that
/// reconnects during response generation cannot be overwritten.
pub struct ForceDiskAuthorityScope {
    file: PathBuf,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for ForceDiskAuthorityScope {
    fn drop(&mut self) {
        let mut baselines = FORCE_DISK_MUTATION_BASELINES.lock();
        if let Some(active) = baselines.get_mut(&self.file) {
            if active.holders > 1 {
                active.holders -= 1;
            } else {
                baselines.remove(&self.file);
            }
        }
    }
}

pub fn begin_force_disk_authority_scope(
    file: &Path,
    source: &str,
) -> Result<ForceDiskAuthorityScope> {
    let file = file.to_path_buf();
    let owner = std::thread::current().id();
    {
        let mut baselines = FORCE_DISK_MUTATION_BASELINES.lock();
        if let Some(active) = baselines.get_mut(&file) {
            if active.owner != owner {
                anyhow::bail!(
                    "another force-disk authorization is already active for {}; wait for it to finish instead of issuing concurrent closeout commands",
                    file.display()
                );
            }
            active.holders += 1;
            return Ok(ForceDiskAuthorityScope {
                file,
                _not_send: std::marker::PhantomData,
            });
        }
    }
    let current = query_live_editor_authority(&file, source).with_context(|| {
        format!(
            "force-disk could not capture initial editor authority for {}; no disk write was performed",
            file.display()
        )
    })?;
    let baseline = force_disk_mutation_fence(&current);
    let mut baselines = FORCE_DISK_MUTATION_BASELINES.lock();
    if baselines.contains_key(&file) {
        anyhow::bail!(
            "another force-disk authorization is already active for {}; wait for it to finish instead of issuing concurrent closeout commands",
            file.display()
        );
    }
    baselines.insert(
        file.clone(),
        ActiveForceDiskMutationBaseline {
            fence: baseline,
            owner,
            holders: 1,
        },
    );
    drop(baselines);
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "force_disk_authority_scope file={} source={} initial={:?}",
            file.display(),
            source,
            baseline
        ),
    );
    Ok(ForceDiskAuthorityScope {
        file,
        _not_send: std::marker::PhantomData,
    })
}

fn force_disk_mutation_fence(
    current: &agent_doc_crdt_relay_io::CurrentText,
) -> ForceDiskMutationFence {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Detached
        | agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::Current {
            live_editors: 0, ..
        } => ForceDiskMutationFence::NoRegisteredEditorReplica,
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
        | agent_doc_crdt_relay_io::CurrentText::Current { .. } => {
            ForceDiskMutationFence::RegisteredEditorReplica
        }
    }
}

fn ensure_force_disk_mutation_authority(file: &Path) -> Result<()> {
    let current = query_live_editor_authority(file, "force_disk_mutation_fence").with_context(|| {
        format!(
            "force-disk mutation fence could not revalidate editor authority for {}; no disk write was performed; retry after the controller is responsive",
            file.display()
        )
    })?;
    let initial = FORCE_DISK_MUTATION_BASELINES
        .lock()
        .get(file)
        .map(|active| active.fence);
    let current_fence = force_disk_mutation_fence(&current);
    match (initial, current_fence) {
        (_, ForceDiskMutationFence::NoRegisteredEditorReplica)
        | (
            Some(ForceDiskMutationFence::RegisteredEditorReplica),
            ForceDiskMutationFence::RegisteredEditorReplica,
        ) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "force_disk_mutation_fence file={} status=clear initial={:?} editor_authority={}",
                    file.display(),
                    initial,
                    current_text_status(&current)
                ),
            );
            Ok(())
        }
        (initial, ForceDiskMutationFence::RegisteredEditorReplica) => {
            Err(ForceDiskAuthorityChanged(format!(
                "force-disk authority changed before mutation for {}: a live editor relay is now registered (initial={initial:?}, status={}); no disk write was performed; run only agent-doc session-check for the existing binary-owned capture so the editor replica can reconcile; do not resubmit finalize, write --commit, or --force-disk",
                file.display(),
                current_text_status(&current)
            ))
            .into())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrdtConvergenceState {
    TypingQuiescence,
    ControllerModelBackpressure,
    EditorAttachedModelMissing,
    EditorSyncPending,
    DeliveryProjectionPending,
    OperatorAdvancedAfterApply,
    CompareAndSwapRaced,
}

/// Per-state time accumulator for one convergence wait
/// (`#crdtprojectionprofile`).
///
/// The wait already logged its state at each 2s notice, which tells you what it
/// was doing *at that instant* and nothing about the distribution. Sampling those
/// notices across a day gave "88% `delivery_projection_pending`" — enough to rule out
/// git, the controller baseline round trip, and poll oversleep, but not enough to
/// explain the ~70% of `commit_authority` that a replica-bootstrap correlation
/// could not account for. Per-write totals turn that into an attributable number
/// instead of a population statistic.
///
/// One call site by construction: [`Self::tick`] runs at the loop head and
/// attributes the whole previous iteration to whatever state that iteration ended
/// in. State assignments are scattered through the loop body, so charging on exit
/// is the only accounting that cannot silently miss one.
#[derive(Debug)]
struct CrdtConvergenceProfile {
    current: CrdtConvergenceState,
    since: std::time::Instant,
    totals: Vec<(CrdtConvergenceState, std::time::Duration)>,
}

impl CrdtConvergenceProfile {
    fn new(initial: CrdtConvergenceState) -> Self {
        Self {
            current: initial,
            since: std::time::Instant::now(),
            totals: Vec::new(),
        }
    }

    /// Charge the elapsed iteration to the state it ended in, then arm for `next`.
    fn tick(&mut self, next: CrdtConvergenceState) {
        let elapsed = self.since.elapsed();
        match self
            .totals
            .iter_mut()
            .find(|(state, _)| *state == self.current)
        {
            Some((_, total)) => *total += elapsed,
            None => self.totals.push((self.current, elapsed)),
        }
        self.current = next;
        self.since = std::time::Instant::now();
    }

    /// `state=ms` pairs, largest first — the breakdown a reader actually wants.
    fn render(&mut self, final_state: CrdtConvergenceState) -> String {
        self.tick(final_state);
        let mut totals = self.totals.clone();
        totals.sort_by_key(|(_, total)| std::cmp::Reverse(*total));
        totals
            .iter()
            .filter(|(_, total)| total.as_millis() > 0)
            .map(|(state, total)| format!("{}={}ms", state.token(), total.as_millis()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl CrdtConvergenceState {
    const fn token(self) -> &'static str {
        match self {
            Self::TypingQuiescence => "typing_quiescence",
            Self::ControllerModelBackpressure => "controller_model_backpressure",
            Self::EditorAttachedModelMissing => "editor_attached_model_missing",
            Self::EditorSyncPending => "editor_sync_pending",
            Self::DeliveryProjectionPending => "delivery_projection_pending",
            Self::OperatorAdvancedAfterApply => "operator_advanced_after_apply",
            Self::CompareAndSwapRaced => "compare_and_swap_raced",
        }
    }
}

impl std::fmt::Display for CrdtConvergenceState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.token())
    }
}

/// Reconcile the replica cache against process liveness when delivery stalls
/// (`#deliveryackcut`).
///
/// A stalled ACK means the hub's member set is caching a replica that is not
/// really there. Invalidate it against the editor pid and refill: a member whose
/// process is GONE is deregistered outright (no zombie, no tombstone), while a
/// member whose process is ALIVE is left alone and nudged to re-register by the
/// surrounding ACK recovery ladder — a live process owes us a replica, so the
/// repair is to rebuild it, not to drop it.
///
/// **Failure here is not swallowed, because it voids the caller's promise.**
/// The only remaining failure is a vanished document (see
/// `reconcile_replicas_against_process_liveness`), which is not transient, so
/// there is nothing to retry. When the cache cannot be reconciled, nothing will
/// complete an "async delivery", so a caller that reports retained success with
/// `operator_action=none` is making a promise the relay cannot keep. The
/// retained path therefore fails closed on `Err` and names the operator
/// recovery instead.
fn reconcile_stalled_replicas(file: &Path, source: &str) -> Result<()> {
    match agent_doc_crdt_relay_io::reconcile_replicas_against_process_liveness(file) {
        Ok(outcome) if !outcome.removed_dead.is_empty() || !outcome.live_unacked.is_empty() => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_crdt_replica_cache_reconciled file={} removed_dead={:?} live_unacked={:?} timeout_ms={} recovery=refill_via_reregistration",
                    file.display(),
                    outcome.removed_dead,
                    outcome.live_unacked,
                    CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS,
                ),
            );
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_crdt_replica_cache_reconcile_failed file={} error={err} recovery=reload_lib",
                    file.display(),
                ),
            );
            Err(err.context(format!(
                "the replica cache for {} could not be reconciled, so retained delivery cannot be \
                 completed by the relay; run `agent-doc admin reload-lib` to replace the editor \
                 replica",
                file.display(),
            )))
        }
    }
}

struct ProjectionObservationState {
    started: Option<std::time::Instant>,
    fallback_backoff_ms: u64,
}

impl Default for ProjectionObservationState {
    fn default() -> Self {
        Self {
            started: None,
            fallback_backoff_ms: CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionObservationWait {
    Continue,
    ForegroundDeadline,
}

struct DeliveryChangeWait<'a> {
    file: &'a Path,
    source: &'a str,
    live_editors: usize,
    delivery_version: u64,
    signal_immediately: bool,
    /// Optional caller deadline. The projection observation still owns its global
    /// eight-second ceiling, but a smaller outer barrier must never be
    /// lengthened by entering the subscription.
    max_wait_ms: Option<u64>,
}

fn exclusive_controller_elapsed_ms(total_elapsed_ms: u64, delivery_wait_elapsed_ms: u64) -> u64 {
    total_elapsed_ms.saturating_sub(delivery_wait_elapsed_ms)
}

fn await_delivery_change(
    file: &Path,
    delivery_version: u64,
    wait: std::time::Duration,
) -> Result<Option<agent_doc_controller_io::project_controller::DeliveryConvergenceStatus>> {
    if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        agent_doc_controller_io::project_controller::
            await_local_delivery_convergence_change_for_file(
                file,
                Some(delivery_version),
                wait,
            )
    } else {
        agent_doc_controller_io::project_controller::await_delivery_convergence_change_for_file(
            file,
            delivery_version,
            wait,
        )
    }
}

impl ProjectionObservationState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn elapsed_ms(&self) -> u64 {
        self.started
            .map(|started| started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }

    fn wait(
        &mut self,
        file: &Path,
        source: &str,
        live_editors: usize,
    ) -> Result<ProjectionObservationWait> {
        let now = std::time::Instant::now();
        let started = *self.started.get_or_insert(now);
        let elapsed_ms = now
            .duration_since(started)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let _ = (file, source, live_editors);
        if elapsed_ms >= CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS {
            return Ok(ProjectionObservationWait::ForegroundDeadline);
        }
        Ok(ProjectionObservationWait::Continue)
    }

    fn next_fallback_sleep_ms(&mut self, available_ms: u64) -> u64 {
        let sleep_ms = self.fallback_backoff_ms.min(available_ms);
        self.fallback_backoff_ms =
            CRDT_PROJECTION_FALLBACK_BACKOFF_POLICY.next_ms(self.fallback_backoff_ms, false);
        sleep_ms
    }

    fn wait_for_delivery_change(
        &mut self,
        request: DeliveryChangeWait<'_>,
    ) -> Result<ProjectionObservationWait> {
        let DeliveryChangeWait {
            file,
            source,
            live_editors,
            delivery_version,
            signal_immediately,
            max_wait_ms,
        } = request;
        if signal_immediately {
            if self.wait(file, source, live_editors)?
                == ProjectionObservationWait::ForegroundDeadline
            {
                return Ok(ProjectionObservationWait::ForegroundDeadline);
            }
        } else {
            self.started.get_or_insert_with(std::time::Instant::now);
            if self.elapsed_ms() >= CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS {
                return Ok(ProjectionObservationWait::ForegroundDeadline);
            }
        }

        let elapsed_ms = self.elapsed_ms();
        let recovery_remaining_ms =
            CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS.saturating_sub(elapsed_ms);
        let wait_ms = max_wait_ms
            .map(|max_wait_ms| recovery_remaining_ms.min(max_wait_ms))
            .unwrap_or(recovery_remaining_ms);
        if wait_ms == 0 {
            return Ok(ProjectionObservationWait::ForegroundDeadline);
        }
        let status = await_delivery_change(
            file,
            delivery_version,
            std::time::Duration::from_millis(wait_ms),
        );
        match status {
            Ok(Some(_status)) => {
                self.fallback_backoff_ms = CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS;
            }
            Ok(None) | Err(_) => {
                // Preserve the existing fail-closed fallback when the owning
                // controller disappears: the accepted intent remains durable
                // and the delivery recovery deadline still governs. This
                // fallback owns its backoff independently from controller/CAS
                // frontier retries.
                std::thread::sleep(std::time::Duration::from_millis(
                    self.next_fallback_sleep_ms(wait_ms),
                ));
            }
        }
        Ok(ProjectionObservationWait::Continue)
    }

    fn wait_for_delivery_change_charged(
        &mut self,
        request: DeliveryChangeWait<'_>,
        delivery_wait_elapsed: &mut std::time::Duration,
    ) -> Result<ProjectionObservationWait> {
        let started = std::time::Instant::now();
        let outcome = self.wait_for_delivery_change(request);
        *delivery_wait_elapsed += started.elapsed();
        outcome
    }
}

/// Controller transport congestion and an already-running model bootstrap are
/// not document conflicts. Closeout owns a larger convergence deadline than
/// either attempt, so these failures are absorbed by the existing coalesced
/// backoff loop instead of escaping to the agent as a prompt to retry an
/// already-accepted response cell.
fn transient_convergence_backpressure_error(err: &anyhow::Error) -> bool {
    let detail = format!("{err:#}").to_ascii_lowercase();
    [
        "timed out",
        "would block",
        "temporarily unavailable",
        "connection refused",
        "connection reset",
        "broken pipe",
        "failed to connect",
        "no such file or directory",
        "reason=in_progress",
        "active recovery attempt finishes",
    ]
    .iter()
    .any(|needle| detail.contains(needle))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentAuthorityObservation {
    source: String,
    authority: agent_doc_state_backbone::DocumentAuthority,
    reason: String,
    content_hash: Option<String>,
    editor_id: Option<String>,
}

pub struct SessionActorWriteQueueSubmitter;

pub static SESSION_ACTOR_WRITE_QUEUE: SessionActorWriteQueueSubmitter =
    SessionActorWriteQueueSubmitter;

impl agent_doc_queue_io::write_queue::DocumentWriteQueueSubmitter
    for SessionActorWriteQueueSubmitter
{
    fn submit<R, F>(
        &self,
        base_dir: &Path,
        file: &str,
        kind: agent_doc_document_realtime::session_ops::SessionOpKind,
        job: F,
    ) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        let actor = agent_doc_session_actor_io::document_actor_in(base_dir, file);
        actor.submit(kind, move |_ctx| job())
    }
}

pub struct RuntimeWriteConvergenceEffects;

pub static RUNTIME_WRITE_CONVERGENCE_EFFECTS: RuntimeWriteConvergenceEffects =
    RuntimeWriteConvergenceEffects;

impl agent_doc_write_converge_io::EditorConvergenceEffects for RuntimeWriteConvergenceEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        atomic_write_through_authority(file, content)
    }

    fn observe_lazily_current_text(&self, file: &Path, source: &str) -> Result<Option<String>> {
        observe_fresh_lazily_current_text(file, source)
    }

    fn apply_canonical_replace_if_attached(
        &self,
        file: &Path,
        expected_current: &str,
        content: &str,
        source: &str,
    ) -> Result<Option<agent_doc_crdt_relay_io::CpRelayWrite>> {
        apply_canonical_replace_if_attached(file, expected_current, content, source)
    }

    fn guard_visible_delivery_convergence(&self, file: &Path, source: &str) -> Result<()> {
        crate::guard_visible_delivery_convergence(file, source)
    }

    fn guard_visible_write_expected_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()> {
        guard_visible_write_expected_current(file, source, expected_current)
    }

    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<()> {
        atomic_write_if_current_through_authority(file, content, expected_current, source)
    }
}

fn observe_fresh_lazily_current_text(file: &Path, _source: &str) -> Result<Option<String>> {
    #[cfg(any(test, feature = "test-support"))]
    const PUBLISH_TIMEOUT_MS: u64 = 100;
    #[cfg(not(any(test, feature = "test-support")))]
    const PUBLISH_TIMEOUT_MS: u64 = 1_000;

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let timeout = std::time::Duration::from_millis(PUBLISH_TIMEOUT_MS);
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let agent_doc_crdt_relay_io::CurrentText::Current { text, .. } =
            query_live_editor_authority(&canonical, "fresh_lazily_current_observation")?
        {
            return Ok(Some(text));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

pub struct RuntimeQueueConsumeWritebackEffects;

pub static RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS: RuntimeQueueConsumeWritebackEffects =
    RuntimeQueueConsumeWritebackEffects;

impl agent_doc_queue_io::queue_consume::QueueConsumeWriteEffects
    for RuntimeQueueConsumeWritebackEffects
{
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        resolve_disk_current_document_content(file, source)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        atomic_write_through_authority(file, content)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_document_or_disk(
            &RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }
}

pub struct RuntimePipelineFrontmatterEffects;

pub static RUNTIME_PIPELINE_FRONTMATTER_EFFECTS: RuntimePipelineFrontmatterEffects =
    RuntimePipelineFrontmatterEffects;

impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
    for RuntimePipelineFrontmatterEffects
{
    fn read_current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        try_resolve_current_document_content(file, source)
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

pub fn atomic_write_through_authority(path: &Path, content: &str) -> Result<()> {
    let visible_document = agent_doc_document_realtime::write_authority::is_visible_document(path);
    if visible_document && !agent_doc_document_realtime::write_authority::within_owner_scope() {
        log_fence_count_drop_if_any(path, content);
        let base_dir = agent_doc_project_root_io::project_root_containing(path)
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let file = path.to_string_lossy().to_string();
        let result = agent_doc_queue_io::write_queue::serialized_atomic_write_with(
            &SESSION_ACTOR_WRITE_QUEUE,
            &base_dir,
            &file,
            path,
            content,
            atomic_write_through_authority,
        );
        if result.is_ok() {
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "write_authority action=routed transport=write_queue len={} hash={}",
                    content.len(),
                    agent_doc_hash::content_hash(content)
                ),
            );
        }
        return result;
    }

    // Inside the per-document write actor, make the CRDT canonical the
    // mutation plane and materialize disk only after every live editor has
    // ACKed the same canonical frontier. The existing disk projection is the
    // best available merge base for this legacy no-CAS API.
    let projection_base = std::fs::read_to_string(path).unwrap_or_default();
    atomic_write_rebased_through_authority_inner(
        path,
        &projection_base,
        content,
        "serialized_atomic_write",
    )
}

/// Serialize one compare-and-swap intent through the canonical CRDT authority.
///
/// Unlike the legacy [`atomic_write_through_authority`] entry point, callers
/// provide the exact document cut from which `content` was derived. This keeps
/// the captured CAS base attached to the queued mutation and avoids the old
/// apply-then-project sequence submitting the same whole-document target twice.
/// A newer editor cut is component-rebased exactly once inside the write actor.
pub fn atomic_write_rebased_through_authority(
    path: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<()> {
    validate_canonical_document_target(path, content, source)?;
    let visible_document = agent_doc_document_realtime::write_authority::is_visible_document(path);
    if visible_document && !agent_doc_document_realtime::write_authority::within_owner_scope() {
        log_fence_count_drop_if_any(path, content);
        let base_dir = agent_doc_project_root_io::project_root_containing(path)
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let file = path.to_string_lossy().to_string();
        let projection_base = expected_current.to_string();
        let queued_source = source.to_string();
        let result = agent_doc_queue_io::write_queue::serialized_atomic_write_with(
            &SESSION_ACTOR_WRITE_QUEUE,
            &base_dir,
            &file,
            path,
            content,
            move |queued_path, queued_content| {
                atomic_write_rebased_through_authority_inner(
                    queued_path,
                    &projection_base,
                    queued_content,
                    &queued_source,
                )
            },
        );
        if result.is_ok() {
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "write_authority action=routed transport=write_queue_cas source={} len={} hash={}",
                    source,
                    content.len(),
                    agent_doc_hash::content_hash(content)
                ),
            );
        }
        return result;
    }

    atomic_write_rebased_through_authority_inner(path, expected_current, content, source)
}

/// `#preflightprojpass`: every write is self-invalidating.
///
/// Opening a projection pass over the write/closeout path is only safe if a
/// mutation can never leave a memoized pre-mutation projection behind. Relying
/// on each mutating caller to remember an explicit
/// `invalidate_current_document_projection` is the failure mode that makes a
/// cache serve stale text — one forgotten call is a silent wrong answer, and
/// "did every mutator remember?" is not a property a review can keep true.
///
/// So invalidation happens here, at the write chokepoint, wrapping the body's
/// several success exits (the CRDT/editor delivery returns never reach the disk
/// write below them). It runs on failure too: a refused or retained write may
/// still have advanced the CRDT, and an unnecessary invalidation only costs one
/// re-resolve, while a missing one is incorrect.
fn atomic_write_rebased_through_authority_inner(
    path: &Path,
    projection_base: &str,
    content: &str,
    source: &str,
) -> Result<()> {
    let result =
        atomic_write_rebased_through_authority_body(path, projection_base, content, source);
    current_document_projection::invalidate_current_document_projection(path);
    result
}

fn atomic_write_rebased_through_authority_body(
    path: &Path,
    projection_base: &str,
    content: &str,
    source: &str,
) -> Result<()> {
    if agent_doc_document_realtime::write_authority::is_visible_document(path) {
        let mut post_proof_rebases = 0usize;
        loop {
            let relay_write =
                match apply_canonical_replace_if_attached(path, projection_base, content, source) {
                    Ok(relay_write) => relay_write,
                    Err(err)
                        if agent_doc_crdt_relay_io::crdt_authority_for_file(path)
                            .editor_attached() =>
                    {
                        let detail = format!("{err:#}");
                        let reason = if detail.contains("editor_attached_model_missing") {
                            DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica
                        } else {
                            DocumentWriteDeferredReason::EditorProjectionPending
                        };
                        let intent_id = ensure_deferred_document_write_intent(
                            path,
                            projection_base,
                            content,
                            source,
                            reason,
                        )?;
                        return Err(err.context(format!(
                        "{source}: retained editor-owned write for {} before retrying live model \
                         reconciliation (intent_id={intent_id})",
                        path.display()
                    )));
                    }
                    Err(err) => return Err(err),
                };
            let Some(relay_write) = relay_write else {
                break;
            };

            if !relay_write.delivery_converged {
                return Err(retained_refusal(path, format!(
                    "serialized_atomic_write: binary-owned write for {} remains retained while the editor projection converges (content_hash={}); the same intent resumes when the controller derives settlement and closeout continuation from the live projection. Do not recapture or rerun finalize/write --commit, and do not force disk",
                    path.display(),
                    relay_write.content_hash,
                )));
            }

            let current = observe_live_editor_authority_after_model_ensure(
                path,
                "serialized_atomic_write_projection",
            )?;
            match current {
                agent_doc_crdt_relay_io::CurrentText::Current {
                    text,
                    delivery_converged: true,
                    ..
                } if agent_doc_hash::content_hash(&text) == relay_write.content_hash => {
                    if !canonical_editor_projection_is_persisted(
                        path,
                        &text,
                        "serialized_atomic_write_projection",
                    )? {
                        let intent_id = ensure_deferred_document_write_intent(
                            path,
                            projection_base,
                            &text,
                            "serialized_atomic_write_editor_save_pending",
                            DocumentWriteDeferredReason::EditorProjectionPending,
                        )?;
                        return Err(retained_refusal(path, format!(
                            "serialized_atomic_write: editor acknowledged the canonical target for {} (content_hash={}) but its native save has not projected that exact editor version to disk; retained intent {} will resume without a behind-the-editor disk write",
                            path.display(),
                            relay_write.content_hash,
                            intent_id,
                        )));
                    }
                    clear_deferred_document_write_intent(
                        path,
                        &relay_write.content_hash,
                        "serialized_atomic_write_projection",
                    )?;
                    agent_doc_ops_log_io::log_op(
                        path,
                        &format!(
                            "write_authority action=materialized transport=crdt_editor_native_save len={} hash={} delivery_converged=true disk_rewritten=false post_proof_rebases={post_proof_rebases}",
                            text.len(),
                            relay_write.content_hash,
                        ),
                    );
                    return Ok(());
                }
                agent_doc_crdt_relay_io::CurrentText::Detached
                    if std::fs::read_to_string(path)
                        .map(|disk| agent_doc_hash::content_hash(&disk) == relay_write.content_hash)
                        .unwrap_or(false) =>
                {
                    clear_deferred_document_write_intent(
                        path,
                        &relay_write.content_hash,
                        "serialized_atomic_write_projection_after_detach",
                    )?;
                    agent_doc_ops_log_io::log_op(
                        path,
                        &format!(
                            "write_authority action=materialized transport=crdt_editor_saved_projection_after_detach hash={} delivery_converged=true disk_rewritten=false post_proof_rebases={post_proof_rebases}",
                            relay_write.content_hash,
                        ),
                    );
                    return Ok(());
                }
                current => {
                    post_proof_rebases += 1;
                    let observed_hash = match &current {
                        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => {
                            agent_doc_hash::content_hash(text)
                        }
                        _ => "unavailable".to_string(),
                    };
                    let intent_id = ensure_deferred_document_write_intent(
                        path,
                        projection_base,
                        content,
                        "serialized_atomic_write_projection_rebase",
                        DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
                    )?;
                    let retry_same_intent = post_proof_rebases <= CRDT_POST_PROOF_REBASE_LIMIT;
                    agent_doc_ops_log_io::log_op(
                        path,
                        &format!(
                            "serialized_atomic_write_post_proof_rebase file={} attempt={} limit={} intent_id={} proven_hash={} observed_hash={} observed={current:?} action={}",
                            path.display(),
                            post_proof_rebases,
                            CRDT_POST_PROOF_REBASE_LIMIT,
                            intent_id,
                            relay_write.content_hash,
                            observed_hash,
                            if retry_same_intent {
                                "rebase_same_intent"
                            } else {
                                "retain_same_intent_for_async_reconnect"
                            },
                        ),
                    );
                    if retry_same_intent {
                        continue;
                    }
                    return Err(retained_refusal(path, format!(
                        "serialized_atomic_write: editor authority for {} kept advancing after delivery proof; binary-owned intent {intent_id} remains retained and will merge the unsaved editor cut before commit. Do not recapture or rerun finalize/write --commit, and do not force disk; session-check/supervisor recovery resumes this same intent",
                        path.display(),
                    )));
                }
            }
        }
    }

    let current = std::fs::read_to_string(path).unwrap_or_default();
    anyhow::ensure!(
        visible_write_content_matches(&current, projection_base)
            || visible_write_content_matches(&current, content),
        "{source}: compare-and-swap raced for detached document {}; expected_hash={} current_hash={}",
        path.display(),
        agent_doc_hash::content_hash(projection_base),
        agent_doc_hash::content_hash(&current),
    );
    atomic_write_authority_raw(path, content)
}

/// Materialize the canonical editor frontier only when the editor has not
/// already saved those exact bytes. Replacing an identical file after the live
/// JetBrains document ACKed its saved frontier changes the VirtualFile stamp
/// behind that document and can manufacture a File Cache Conflict despite byte
/// equality.
fn canonical_disk_projection_is_exact(path: &Path, canonical: &str) -> bool {
    std::fs::read(path)
        .map(|disk| disk == canonical.as_bytes())
        .unwrap_or(false)
}

/// Ask the owning editor to project its already-authoritative buffer to disk.
///
/// A live editor is the write authority. Writing the same bytes directly to
/// disk after a CRDT delivery projection races the IDE's file-cache conflict handling
/// and can resurrect the older disk snapshot. The native save is therefore a
/// distinct protocol transition: it is successful only when disk contains the
/// exact canonical version and that same version remains the converged live
/// editor authority.
fn editor_save_authority_is_sufficient(
    authoritative_text: &str,
    canonical: &str,
    live_editors: usize,
    delivery_converged: bool,
) -> bool {
    authoritative_text == canonical && live_editors > 0 && delivery_converged
}

fn canonical_editor_projection_is_persisted(
    path: &Path,
    canonical: &str,
    source: &str,
) -> Result<bool> {
    if canonical_disk_projection_is_exact(path, canonical)
        && let agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged,
            ..
        } = observe_live_editor_authority_after_model_ensure(
            path,
            "editor_projection_persistence_proof",
        )?
        && editor_save_authority_is_sufficient(&text, canonical, live_editors, delivery_converged)
    {
        return Ok(true);
    }
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "editor_projection_persistence_pending file={} source={} content_hash={} driver=state_projection operator_action=none disk_write=false",
            path.display(),
            source,
            agent_doc_hash::content_hash(canonical),
        ),
    );
    Ok(false)
}

/// Whether the actual live editor replica has acknowledged the exact canonical
/// version and can therefore own the native-save Effect.
///
/// A controller CP-model match alone is insufficient: the editor `Document`
/// may still contain the prior version until its delivery projection arrives.
pub fn live_editor_projection_ready_for_native_save(
    path: &Path,
    canonical: &str,
    source: &str,
) -> Result<bool> {
    let agent_doc_crdt_relay_io::CurrentText::Current {
        text,
        live_editors,
        delivery_converged,
        ..
    } = observe_live_editor_authority_after_model_ensure(path, source)?
    else {
        return Ok(false);
    };
    Ok(editor_save_authority_is_sufficient(
        &text,
        canonical,
        live_editors,
        delivery_converged,
    ))
}

/// Project the exact live editor authority through the editor's native save
/// path without changing the editor buffer.
///
/// This is the terminal recovery path for a valid, delivery-converged editor
/// cut whose historical deferred-write event was incorrectly retired after an
/// ACK but before disk-save proof. The editor remains the source of truth: no
/// disk candidate is merged into it and no force-disk write is permitted. A
/// canonical CP model is not proof that even a sole editor has applied the
/// revision; every live-editor save therefore requires delivery convergence.
pub fn settle_live_editor_projection_through_authority(path: &Path, source: &str) -> Result<bool> {
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    if canonical == disk {
        return Ok(true);
    }
    // Persisting the exact live editor revision is an authority-convergence
    // effect, not a semantic document mutation. Template validation may gate a
    // later structure-dependent write, but it must never gate saving the
    // operator-authoritative buffer or transfer that responsibility to the
    // operator.
    let agent_doc_crdt_relay_io::CurrentText::Current {
        text,
        live_editors,
        delivery_converged,
        ..
    } = observe_live_editor_authority_after_model_ensure(path, source)?
    else {
        return Ok(false);
    };
    if !editor_save_authority_is_sufficient(&text, &canonical, live_editors, delivery_converged) {
        return Ok(false);
    }
    if !canonical_editor_projection_is_persisted(path, &canonical, source)? {
        return Ok(false);
    }
    let settled_authority = try_resolve_current_document_content(path, source)?;
    let settled_disk = resolve_disk_current_document_content(path, source)?;
    if settled_authority != canonical || settled_disk != canonical {
        return Ok(false);
    }
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "live_editor_projection_settled file={} source={} content_hash={} editor_authority=true delivery_converged={} disk_version_exact=true",
            path.display(),
            source,
            agent_doc_hash::content_hash(&canonical),
            delivery_converged,
        ),
    );
    Ok(true)
}

/// Explicit operator-authorized disk escape hatch. It preserves the same
/// per-document actor serialization as ordinary writes but intentionally does
/// not enter the attached CRDT convergence loop.
pub fn atomic_write_force_disk_through_authority(path: &Path, content: &str) -> Result<()> {
    let visible_document = agent_doc_document_realtime::write_authority::is_visible_document(path);
    if visible_document && !agent_doc_document_realtime::write_authority::within_owner_scope() {
        log_fence_count_drop_if_any(path, content);
        let base_dir = agent_doc_project_root_io::project_root_containing(path)
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let file = path.to_string_lossy().to_string();
        let result = agent_doc_queue_io::write_queue::serialized_atomic_write_with(
            &SESSION_ACTOR_WRITE_QUEUE,
            &base_dir,
            &file,
            path,
            content,
            atomic_write_force_disk_through_authority,
        );
        if result.is_ok() {
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "write_authority action=routed transport=write_queue mode=force_disk len={} hash={}",
                    content.len(),
                    agent_doc_hash::content_hash(content)
                ),
            );
        }
        return result;
    }

    // Revalidate at the serialized mutation boundary. Authorization may have
    // been granted while the editor relay was absent, then become stale while
    // this write waited behind another document operation.
    ensure_force_disk_mutation_authority(path)?;
    retain_force_disk_reconnect_intent(path, content)?;
    atomic_write_authority_raw(path, content)
}

pub fn atomic_write_if_current_through_authority(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<()> {
    guard_visible_write_expected_current_or_target(path, source, expected_current, Some(content))?;
    atomic_write_rebased_through_authority(path, expected_current, content, source)
}

/// Settle a committed projection that is known to differ from the current
/// document only by transient agent-doc markers.
///
/// Session-check calls this only after proving canonical authority and disk are
/// byte-identical and the committed target is normalization-equivalent. Clear
/// every older deferred target on both sides of the CAS so a reconnect cannot
/// replay the stale intermediate projection after the committed bytes become
/// visible.
pub fn settle_committed_projection_if_current_through_authority(
    path: &Path,
    committed_content: &str,
    expected_current: &str,
    source: &str,
) -> Result<()> {
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        canonical == expected_current && disk == expected_current,
        "{source}: refusing committed projection settlement for {} without exact authority/disk current-content proof (expected_hash={}, canonical_hash={}, disk_hash={}, component_divergence={})",
        path.display(),
        agent_doc_hash::content_hash(expected_current),
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            &canonical, &disk,
        ),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    atomic_write_if_current_through_authority(path, committed_content, expected_current, source)?;
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        canonical == committed_content && disk == committed_content,
        "{source}: committed projection settlement for {} did not converge exactly (committed_hash={}, canonical_hash={}, disk_hash={}, component_divergence={})",
        path.display(),
        agent_doc_hash::content_hash(committed_content),
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            &canonical, &disk,
        ),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "committed_projection_settled file={} prior_hash={} committed_hash={} deferred_lineage=cleared",
            path.display(),
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(committed_content),
        ),
    );
    Ok(())
}

/// Resume a committed target whose canonical authority is ahead of disk.
///
/// Ordinarily, a retained intent must prove both the current canonical target
/// and the editor/disk base from which it was computed. For historical versions
/// that cleared the intent after committing but before projecting disk, an
/// absent intent is accepted only when committed authority and disk normalize
/// to identical semantic content after transient markers are removed. The
/// ordinary CRDT write barrier then delivers and materializes the target. A
/// mismatched intent or semantic drift fails closed instead of overwriting text.
pub fn settle_retained_committed_projection_through_authority(
    path: &Path,
    committed_content: &str,
    expected_disk: &str,
    source: &str,
) -> Result<bool> {
    let disk = resolve_disk_current_document_content(path, source)?;
    if disk != expected_disk && disk != committed_content {
        return Ok(false);
    }
    let committed_hash = agent_doc_hash::content_hash(committed_content);
    let disk_hash = agent_doc_hash::content_hash(expected_disk);
    let settlement_basis = match pending_document_write(path) {
        Some(pending)
            if pending.target_content == committed_content
                && pending.target_hash.eq_ignore_ascii_case(&committed_hash)
                && pending.expected_hash.eq_ignore_ascii_case(&disk_hash)
                && pending.expected_content.as_deref() == Some(expected_disk) =>
        {
            "retained_lineage"
        }
        Some(_) => return Ok(false),
        None if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            committed_content,
        )
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                expected_disk,
            ) =>
        {
            "historical_transient_split"
        }
        None => return Ok(false),
    };

    let mut canonical = try_resolve_current_document_content(path, source)?;
    if canonical == expected_disk && canonical != committed_content {
        let Some(relay_write) =
            apply_canonical_replace_if_attached(path, expected_disk, committed_content, source)?
        else {
            return Ok(false);
        };
        if !relay_write.delivery_converged {
            return Ok(false);
        }
        canonical = try_resolve_current_document_content(path, source)?;
    }
    if canonical != committed_content {
        return Ok(false);
    }
    if !settle_live_editor_projection_through_authority(path, source)? {
        return Ok(false);
    }
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        canonical == committed_content && disk == committed_content,
        "{source}: retained committed projection for {} did not converge exactly (committed_hash={}, canonical_hash={}, disk_hash={}, component_divergence={})",
        path.display(),
        committed_hash,
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            &canonical, &disk,
        ),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "retained_committed_projection_settled file={} prior_disk_hash={} committed_hash={} settlement_basis={} deferred_lineage=cleared",
            path.display(),
            disk_hash,
            committed_hash,
            settlement_basis,
        ),
    );
    Ok(true)
}

/// Settle a captured (not yet committed) closeout after an editor reconnect has
/// already installed the retained target as both canonical authority and disk.
///
/// Replacement replica registration bootstraps directly from canonical state,
/// so it can be delivery-converged with an empty ACK queue. In that state the
/// durable deferred-write slot is historical evidence, not an outstanding
/// delivery. Clear it only when the exact retained target is authoritative,
/// projected to disk, and contains the already-durable captured response.
pub fn settle_retained_captured_projection_through_authority(
    path: &Path,
    captured_response: &str,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_document_write(path) else {
        return Ok(false);
    };
    let retained_target_hash = agent_doc_hash::content_hash(&pending.target_content);
    if !pending
        .target_hash
        .eq_ignore_ascii_case(&retained_target_hash)
    {
        return Ok(false);
    }

    // The retained target was composed against an older authority cut. A live
    // editor may have advanced before its delivery projection converged, or the
    // operator may have saved that newer cut before asynchronous recovery
    // resumed. Replay the same retained journal over the current canonical text;
    // never require Ctrl+S, preflight repair, recapture, or a force-disk reset to
    // make progress.
    let mut canonical = try_resolve_current_document_content(path, source)?;
    let observed_canonical = canonical.clone();
    let mut repaired_canonical =
        heal_welded_scaffolding(&canonical).unwrap_or_else(|| canonical.clone());
    let welded_scaffolding_repaired = repaired_canonical != canonical;
    let terminal_debris =
        agent_doc_element_done::prune_proven_redundant_terminal_debris(&repaired_canonical);
    let removed_line_count = terminal_debris
        .as_ref()
        .map_or(0, |pruned| pruned.removed_line_count);
    if let Some(pruned) = terminal_debris {
        repaired_canonical = pruned.content;
    }
    if repaired_canonical != observed_canonical {
        validate_canonical_document_target(
            path,
            &repaired_canonical,
            "retained_captured_terminal_debris_projection",
        )?;
        let Some(_) = apply_canonical_replace_if_attached(
            path,
            &observed_canonical,
            &repaired_canonical,
            "retained_captured_terminal_debris_projection",
        )?
        else {
            return Ok(false);
        };
        let projected = try_resolve_current_document_content(path, source)?;
        if projected != repaired_canonical {
            // The effect receipt is only evidence. The Lazily projection of
            // current authority is the settlement gate and will recompute when
            // replica state advances.
            return Ok(false);
        }
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_captured_terminal_debris_projected file={} source={} removed_line_count={} welded_scaffolding_repaired={} projected_hash={}",
                path.display(),
                source,
                removed_line_count,
                welded_scaffolding_repaired,
                agent_doc_hash::content_hash(&projected),
            ),
        );
        canonical = projected;
    }
    // A newer authority cut that already contains the durable response is the
    // desired semantic result. Replaying an older incomplete intent over it can
    // only regress the closeout and may discard newer operator-owned edits.
    let response_already_materialized =
        agent_doc_turn::response_replay::response_materialized_in_content(
            captured_response,
            &canonical,
        );
    let pending_journal = pending_document_write_journal(path);
    let has_superseded_capture_lineage = pending_journal.iter().any(|intent| {
        !agent_doc_turn::response_replay::response_materialized_in_content(
            captured_response,
            &intent.target_content,
        )
    });
    if has_superseded_capture_lineage {
        // A prior cycle can leave a deferred reconnect target in front of this
        // durable capture. Replaying the whole journal would merge that stale
        // branch into the new response and can resurrect deleted queue state or
        // malformed scaffolding. When current authority already materializes
        // the response, however, it is the safe semantic state: let the normal
        // native-save effect sink below project those exact bytes before
        // retiring the older lineage. Only a current cut that still lacks the
        // response needs exact disk equality before the stale lineage can be
        // discarded.
        let disk = resolve_disk_current_document_content(path, source)?;
        if disk != canonical && !response_already_materialized {
            return Ok(false);
        }
        if disk == canonical {
            validate_canonical_document_target(path, &canonical, source)?;
            clear_all_deferred_document_write_intents(path, source)?;
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "retained_captured_prior_lineage_superseded file={} prior_intent_count={} canonical_hash={} canonical_disk_exact=true active_capture_materialized={}",
                    path.display(),
                    pending_journal.len(),
                    agent_doc_hash::content_hash(&canonical),
                    response_already_materialized,
                ),
            );
        }
    }
    if canonical != pending.target_content
        && !response_already_materialized
        && !has_superseded_capture_lineage
        && let Some(rebased_target) = deferred_document_write_reconnect_content(path, &canonical)?
        && agent_doc_turn::response_replay::response_materialized_in_content(
            captured_response,
            &rebased_target,
        )
    {
        if rebased_target != canonical {
            let Some(relay_write) = apply_canonical_replace_if_attached(
                path,
                &canonical,
                &rebased_target,
                "retained_captured_projection_rebase",
            )?
            else {
                return Ok(false);
            };
            if !relay_write.delivery_converged {
                return Ok(false);
            }
        }
        canonical = try_resolve_current_document_content(path, source)?;
    }
    if !agent_doc_turn::response_replay::response_materialized_in_content(
        captured_response,
        &canonical,
    ) {
        // A reconnect intent can itself be the incomplete value: older
        // recovery code retained the editor/CRDT merge as the newest target
        // even when that merge had dropped the captured response. Replaying
        // that target forever can never make progress. The captured response
        // is a durable semantic intent, so materialize only its response cell
        // over the exact current editor-authoritative document. This keeps
        // every newer operator edit/deletion outside that cell intact.
        let Some(replayed_target) =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                &canonical,
                captured_response,
            )
        else {
            return Ok(false);
        };
        if replayed_target == canonical {
            return Ok(false);
        }
        validate_canonical_document_target(path, &replayed_target, source)?;
        let Some(relay_write) = apply_canonical_replace_if_attached(
            path,
            &canonical,
            &replayed_target,
            "retained_captured_response_cell_replay",
        )?
        else {
            return Ok(false);
        };
        if !relay_write.delivery_converged {
            return Ok(false);
        }
        canonical = try_resolve_current_document_content(path, source)?;
        if canonical != replayed_target
            || !agent_doc_turn::response_replay::response_materialized_in_content(
                captured_response,
                &canonical,
            )
        {
            return Ok(false);
        }
    }
    validate_canonical_document_target(path, &canonical, source)?;
    let settled_target_hash = agent_doc_hash::content_hash(&canonical);

    let mut disk = resolve_disk_current_document_content(path, source)?;
    if disk != canonical {
        let Some(projected) =
            settle_projected_captured_response_through_authority(path, captured_response, source)?
        else {
            return Ok(false);
        };
        if projected != canonical {
            return Ok(false);
        }
        disk = resolve_disk_current_document_content(path, source)?;
        if disk != canonical {
            return Ok(false);
        }
    }

    clear_all_deferred_document_write_intents(path, source)?;
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "retained_captured_projection_settled file={} intent_id={} retained_target_hash={} settled_target_hash={} canonical_disk_exact=true captured_response_materialized=true same_intent_rebased={} deferred_lineage=cleared",
            path.display(),
            pending.intent_id,
            retained_target_hash,
            settled_target_hash,
            retained_target_hash != settled_target_hash,
        ),
    );
    Ok(true)
}

/// Settle a retained document projection that is not owned by an active
/// captured-response closeout.
///
/// Delivery-only reconciliation can retain a deterministic document projection
/// without capturing an assistant response. When canonical editor authority is
/// already the exact retained target but disk still trails it, request the
/// editor's native save and settle only after disk proves the same bytes. No
/// response proof is applicable or necessary.
///
/// Semantic rebase intents are deliberately excluded. Exact bytes do not prove
/// that a target composed before a newer operator cut preserved deletions from
/// that cut; settling one merely because it reached disk can bless a
/// resurrection and erase the causal lineage needed to repair it.
pub fn settle_retained_non_capture_projection_through_authority(
    path: &Path,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_document_write(path) else {
        return Ok(false);
    };
    // agent-doc 0.35.0 briefly published the active exchange prompt marker via
    // `atomic_write_through_authority`. A delayed editor ACK therefore retained
    // a durable write for a cosmetic-only transformation and wedged the next
    // preflight. Retire only that exact historical shape: a serialized write
    // whose target contains the active marker and is otherwise equivalent to
    // its expected exchange content after transient prefixes are removed.
    let transient_active_prompt_marker_intent = pending.source.is_serialized_atomic_write()
        && pending.target_content.contains('🚧')
        && pending.expected_content.as_deref().is_some_and(|expected| {
            expected != pending.target_content
                && agent_doc_document::transient_markers::exchange_prompt_prefix_equivalent(
                    expected,
                    &pending.target_content,
                )
        });
    if transient_active_prompt_marker_intent {
        clear_deferred_document_write_intent(path, &pending.target_hash, source)?;
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_transient_prompt_marker_projection_retired file={} intent_id={} target_hash={} semantic=exchange_prompt_prefix_equivalent delivery_ack_not_required=true deferred_lineage=cleared",
                path.display(),
                pending.intent_id,
                pending.target_hash,
            ),
        );
        return Ok(true);
    }
    let mut canonical = try_resolve_current_document_content(path, source)?;
    if retire_superseded_compact_projection_intents(path, &canonical, source)? > 0 {
        return Ok(true);
    }

    let exact_projection_reason = matches!(
        pending.reason,
        DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica
            | DocumentWriteDeferredReason::EditorDeliveryWorkerStale
            | DocumentWriteDeferredReason::EditorProjectionPending
    );
    // Delivery-stage reasons describe where projection paused, not whether a
    // response target must be replayed byte-for-byte. Socket delivery may land
    // the response before the controller records ACK convergence, leaving an
    // exact-stage intent behind a newer canonical response. Preserve the
    // content-bearing base for every response-introducing intent so settlement
    // can accept that newer operator cut instead of restoring the stale target.
    let semantic_response_base = (canonical != pending.target_content)
        .then_some(pending.expected_content.as_deref())
        .flatten()
        .filter(|expected| {
            !write_policy::buffer_presents_reference_response(&pending.target_content, expected)
        });
    let target_hash = agent_doc_hash::content_hash(&pending.target_content);
    // A reconnect can merge a typed post-delivery rebase target into the live
    // editor before settlement reobserves the intent. Byte-exact canonical
    // content plus the journaled target hash is sufficient to enter the
    // existing live-editor, delivery, and native-save verification below; it
    // does not itself clear the intent or write the document. Unknown semantic
    // rebase intents still require operator-cut lineage.
    let exact_target_proof = matches!(
        &pending.source,
        agent_doc_state_backbone::DocumentWriteSource::SerializedAtomicWriteProjectionRebase
    ) && canonical == pending.target_content
        && pending.target_hash.eq_ignore_ascii_case(&target_hash);
    if !exact_projection_reason && semantic_response_base.is_none() && !exact_target_proof {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_non_capture_projection_settlement_deferred file={} intent_id={} reason={} proof=missing_operator_cut_lineage",
                path.display(),
                pending.intent_id,
                pending.reason,
            ),
        );
        return Ok(false);
    }
    if let Some(semantic_base) = semantic_response_base {
        let rebased = rebase_agent_candidate_over_editor_cut(
            semantic_base,
            &pending.target_content,
            &canonical,
        )?;
        if rebased != canonical {
            let Some(relay_write) = apply_canonical_replace_if_attached(
                path,
                &canonical,
                &rebased,
                "retained_non_capture_response_rebase",
            )?
            else {
                return Ok(false);
            };
            if !relay_write.delivery_converged {
                return Ok(false);
            }
            canonical = try_resolve_current_document_content(path, source)?;
        }
        if !write_policy::buffer_presents_reference_response(&pending.target_content, &canonical) {
            return Ok(false);
        }
        validate_canonical_document_target(path, &canonical, source)?;

        let mut disk = resolve_disk_current_document_content(path, source)?;
        if disk != canonical {
            let agent_doc_crdt_relay_io::CurrentText::Current {
                text,
                live_editors,
                delivery_converged: true,
                ..
            } = observe_live_editor_authority_after_model_ensure(path, source)?
            else {
                return Ok(false);
            };
            if live_editors == 0 || text != canonical {
                return Ok(false);
            }
            if !canonical_editor_projection_is_persisted(
                path,
                &canonical,
                "retained_non_capture_response_settlement",
            )? {
                return Ok(false);
            }
            disk = resolve_disk_current_document_content(path, source)?;
            if disk != canonical {
                return Ok(false);
            }
        }

        let settled_target_hash = agent_doc_hash::content_hash(&canonical);
        clear_all_deferred_document_write_intents(path, source)?;
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_non_capture_response_projection_settled file={} intent_id={} retained_target_hash={} settled_target_hash={} semantic_rebase=true canonical_disk_exact=true native_editor_save=true deferred_lineage=cleared",
                path.display(),
                pending.intent_id,
                pending.target_hash,
                settled_target_hash,
            ),
        );
        return Ok(true);
    }

    if canonical != pending.target_content
        || !pending.target_hash.eq_ignore_ascii_case(&target_hash)
    {
        return Ok(false);
    }
    let mut disk = resolve_disk_current_document_content(path, source)?;
    if disk != pending.target_content {
        let agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged: true,
            ..
        } = observe_live_editor_authority_after_model_ensure(path, source)?
        else {
            return Ok(false);
        };
        if live_editors == 0 || text != pending.target_content {
            return Ok(false);
        }
        if !canonical_editor_projection_is_persisted(
            path,
            &pending.target_content,
            "retained_non_capture_projection_settlement",
        )? {
            return Ok(false);
        }
        disk = resolve_disk_current_document_content(path, source)?;
        if disk != pending.target_content {
            return Ok(false);
        }
    }

    clear_all_deferred_document_write_intents(path, source)?;
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "retained_non_capture_projection_settled file={} intent_id={} target_hash={} canonical_disk_exact=true native_editor_save=true deferred_lineage=cleared",
            path.display(),
            pending.intent_id,
            target_hash,
        ),
    );
    Ok(true)
}

/// Retire one retained projection after a newer editor-authoritative cut has
/// converged to disk.
///
/// This is a compare-and-retire operation: it never writes document bytes, and
/// it refuses while authority/disk disagree or while the retained target is
/// still current.
pub fn retire_retained_projection_superseded_by_authority(
    path: &Path,
    expected_target_hash: &str,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_document_write(path) else {
        return Ok(false);
    };
    if !pending
        .target_hash
        .eq_ignore_ascii_case(expected_target_hash)
    {
        return Ok(false);
    }
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    if canonical != disk
        || agent_doc_hash::content_hash(&canonical).eq_ignore_ascii_case(expected_target_hash)
    {
        return Ok(false);
    }
    if pending.expected_content.as_deref() == Some(canonical.as_str()) {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_projection_supersession_deferred file={} intent_id={} retained_target_hash={} authoritative_hash={} reason=authority_is_undelivered_expected_base",
                path.display(),
                pending.intent_id,
                pending.target_hash,
                agent_doc_hash::content_hash(&canonical),
            ),
        );
        return Ok(false);
    }
    clear_deferred_document_write_intent(path, &pending.target_hash, source)?;
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "retained_projection_superseded file={} intent_id={} retired_target_hash={} authoritative_hash={} authority=editor_crdt disk_exact=true",
            path.display(),
            pending.intent_id,
            pending.target_hash,
            agent_doc_hash::content_hash(&canonical),
        ),
    );
    Ok(true)
}

fn retire_superseded_compact_projection_intents(
    path: &Path,
    canonical: &str,
    source: &str,
) -> Result<usize> {
    let Some(current_archive_timestamp) =
        agent_doc_document::compact_projection::compacted_exchange_archive_timestamp(canonical)
    else {
        return Ok(0);
    };
    if resolve_disk_current_document_content(path, source)? != canonical {
        return Ok(0);
    }
    let authority_exact = match observe_live_editor_authority_after_model_ensure(path, source)? {
        agent_doc_crdt_relay_io::CurrentText::Detached => true,
        agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            delivery_converged: true,
            ..
        } => text == canonical,
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
        | agent_doc_crdt_relay_io::CurrentText::Current {
            delivery_converged: false,
            ..
        } => false,
    };
    if !authority_exact {
        return Ok(0);
    }

    let mut retired = 0;
    for intent in pending_document_write_journal(path) {
        let eligible_source =
            intent.source.is_post_commit_reposition() || intent.source.is_serialized_atomic_write();
        let eligible_reason = matches!(
            intent.reason,
            DocumentWriteDeferredReason::EditorProjectionPending
                | DocumentWriteDeferredReason::ExtendPendingEditorReconnectTarget
        );
        let target_hash_exact = intent
            .target_hash
            .eq_ignore_ascii_case(&agent_doc_hash::content_hash(&intent.target_content));
        if !eligible_source
            || !eligible_reason
            || !target_hash_exact
            || !agent_doc_document::compact_projection::newer_compacted_exchange_supersedes(
                &intent.target_content,
                canonical,
            )
        {
            continue;
        }

        let retained_archive_timestamp =
            agent_doc_document::compact_projection::compacted_exchange_archive_timestamp(
                &intent.target_content,
            )
            .expect("superseded compact projection must have a validated timestamp");
        clear_deferred_document_write_intent(path, &intent.target_hash, source)?;
        retired += 1;
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "retained_superseded_compact_projection_retired file={} intent_id={} target_hash={} retained_archive_timestamp={} current_archive_timestamp={} canonical_disk_exact=true authority_delivery_exact=true stale_target_replayed=false",
                path.display(),
                intent.intent_id,
                intent.target_hash,
                retained_archive_timestamp,
                current_archive_timestamp,
            ),
        );
    }
    Ok(retired)
}

/// Finish the disk half of a response projection only after a live editor has
/// acknowledged the exact canonical frontier. This is the asynchronous twin of
/// the foreground `atomic_write_through_authority` materialization step: it is
/// safe after a delayed ACK because the editor buffer already contains the same
/// response-bearing bytes, and it never overwrites a newer editor cut.
pub fn settle_projected_captured_response_through_authority(
    path: &Path,
    captured_response: &str,
    source: &str,
) -> Result<Option<String>> {
    let current = observe_live_editor_authority_after_model_ensure(path, source)?;
    let agent_doc_crdt_relay_io::CurrentText::Current {
        mut text,
        live_editors,
        delivery_converged: true,
        ..
    } = current
    else {
        return Ok(None);
    };
    if live_editors == 0 {
        return Ok(None);
    }
    if !agent_doc_turn::response_replay::response_materialized_in_content(captured_response, &text)
    {
        let Some(replayed_target) =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                &text,
                captured_response,
            )
        else {
            return Ok(None);
        };
        if replayed_target == text {
            return Ok(None);
        }
        validate_canonical_document_target(path, &replayed_target, source)?;
        let Some(relay_write) = apply_canonical_replace_if_attached(
            path,
            &text,
            &replayed_target,
            "projected_captured_response_cell_replay",
        )?
        else {
            return Ok(None);
        };
        if !relay_write.delivery_converged {
            return Ok(None);
        }
        text = try_resolve_current_document_content(path, source)?;
    }
    if !agent_doc_turn::response_replay::response_materialized_in_content(captured_response, &text)
    {
        return Ok(None);
    }

    if !canonical_editor_projection_is_persisted(
        path,
        &text,
        "projected_captured_response_settlement",
    )? {
        return Ok(None);
    }
    let disk = resolve_disk_current_document_content(path, source)?;
    if disk != text {
        return Ok(None);
    }
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "projected_captured_response_settled file={} content_hash={} live_editors={} delivery_converged=true disk_rewritten=false native_editor_save=true captured_response_materialized=true",
            path.display(),
            agent_doc_hash::content_hash(&text),
            live_editors,
        ),
    );
    Ok(Some(text))
}

/// Apply a repair through the same controller-owned reactive projection used by
/// ordinary writes.
///
/// Repair is not an authority escape hatch. If no editor replica can receive
/// the canonical target, the ordinary write retains its exact intent and
/// returns the typed deferral. It must not project to disk behind the editor or
/// promote a historical editor buffer into controller canonical state.
pub fn atomic_repair_write_if_current_through_authority(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<String> {
    atomic_write_if_current_through_authority(path, content, expected_current, source)?;
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    let (canonical, disk) = await_atomic_repair_projection(
        path,
        content,
        expected_current,
        source,
        canonical,
        disk,
    )?;
    settle_atomic_repair_projection(path, content, expected_current, source, canonical, disk)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomicRepairProjectionState {
    Converged,
    LateEditorAttachment,
    Diverged,
}

fn classify_atomic_repair_projection(
    content: &str,
    expected_current: &str,
    canonical: &str,
    disk: &str,
) -> AtomicRepairProjectionState {
    if canonical == content && disk == content {
        AtomicRepairProjectionState::Converged
    } else if canonical == expected_current && disk == content {
        AtomicRepairProjectionState::LateEditorAttachment
    } else {
        AtomicRepairProjectionState::Diverged
    }
}

/// `#dedupepresettle`: the settle window belongs to every non-converged sample,
/// not only the late-editor-attachment shape.
///
/// A repair whose projection is still propagating samples as `Diverged` —
/// canonical and disk both differ from the target because neither has caught up
/// yet — is indistinguishable, on the first read after the write, from a repair
/// that genuinely lost a race. Returning immediately on `Diverged` gave that
/// sample zero settle time, so `settle_atomic_repair_projection` reported
/// `did not converge exactly before settling deferred lineage` for a write that
/// converged milliseconds later. Observed 2026-08-08 on
/// `tasks/agent-doc/agent-doc-bugs2.md`: `session-check`'s own
/// `self_heal_response_replay_duplication` failed terminally, and a plain rerun
/// of the identical repair converged.
///
/// Poll while the projection is unconverged in either shape. The terminal
/// classification is then decided by the sample taken when the window closes,
/// not the one taken the instant after the write, and both typed refusals in
/// `settle_atomic_repair_projection` are reached with exactly the same
/// semantics — only later.
fn atomic_repair_projection_should_await(state: AtomicRepairProjectionState) -> bool {
    match state {
        AtomicRepairProjectionState::Converged => false,
        AtomicRepairProjectionState::LateEditorAttachment
        | AtomicRepairProjectionState::Diverged => true,
    }
}

/// Pure settle loop over an injected projection sampler.
///
/// The sampler is the only I/O, so the wait policy is testable without a live
/// controller, editor, or filesystem race.
fn await_atomic_repair_projection_samples<S>(
    content: &str,
    expected_current: &str,
    timeout: std::time::Duration,
    mut canonical: String,
    mut disk: String,
    mut sample: S,
) -> Result<(String, String)>
where
    S: FnMut() -> Result<(String, String)>,
{
    if !atomic_repair_projection_should_await(classify_atomic_repair_projection(
        content,
        expected_current,
        &canonical,
        &disk,
    )) {
        return Ok((canonical, disk));
    }

    let started = std::time::Instant::now();
    let mut backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
    while started.elapsed() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
        let (next_canonical, next_disk) = sample()?;
        canonical = next_canonical;
        disk = next_disk;
        if !atomic_repair_projection_should_await(classify_atomic_repair_projection(
            content,
            expected_current,
            &canonical,
            &disk,
        )) {
            return Ok((canonical, disk));
        }
        backoff_ms = CRDT_WRITE_BACKOFF_POLICY.next_ms(backoff_ms, false);
    }
    Ok((canonical, disk))
}

fn await_atomic_repair_projection(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
    canonical: String,
    disk: String,
) -> Result<(String, String)> {
    let entry_state =
        classify_atomic_repair_projection(content, expected_current, &canonical, &disk);
    let started = std::time::Instant::now();
    let (canonical, disk) = await_atomic_repair_projection_samples(
        content,
        expected_current,
        std::time::Duration::from_millis(ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS),
        canonical,
        disk,
        || {
            Ok((
                try_resolve_current_document_content(path, source)?,
                resolve_disk_current_document_content(path, source)?,
            ))
        },
    )?;
    if entry_state != AtomicRepairProjectionState::Converged
        && classify_atomic_repair_projection(content, expected_current, &canonical, &disk)
            == AtomicRepairProjectionState::Converged
    {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "repair_projection_settled file={} source={} target_hash={} wait_ms={} entry_state={:?} recovery=observed_convergence",
                path.display(),
                source,
                agent_doc_hash::content_hash(content),
                started.elapsed().as_millis(),
                entry_state,
            ),
        );
    }
    Ok((canonical, disk))
}

fn settle_atomic_repair_projection(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
    canonical: String,
    disk: String,
) -> Result<String> {
    if canonical == content && disk == content {
        clear_all_deferred_document_write_intents(path, source)?;
        return Ok(canonical);
    }

    let target_hash = agent_doc_hash::content_hash(content);
    if let Some(pending) = pending_document_write_for_target(path, &target_hash) {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "repair_projection_retained file={} source={} intent_id={} target_hash={} canonical_hash={} disk_hash={} recovery=controller_reactive_projection operator_action=none",
                path.display(),
                source,
                pending.intent_id,
                target_hash,
                agent_doc_hash::content_hash(&canonical),
                agent_doc_hash::content_hash(&disk),
            ),
        );
        return Err(retained_refusal(path, format!(
            "{source}: repair projection for {} is retained by the controller's reactive \
             document graph (intent_id={}, target_hash={}, canonical_hash={}, disk_hash={}); \
             foreground acceptance is not terminal disk equality, and the subscribed \
             projection effect owns convergence. Do not resubmit, republish, force disk, or \
             recycle the controller",
            path.display(),
            pending.intent_id,
            target_hash,
            agent_doc_hash::content_hash(&canonical),
            agent_doc_hash::content_hash(&disk),
        )));
    }

    if classify_atomic_repair_projection(content, expected_current, &canonical, &disk)
        == AtomicRepairProjectionState::LateEditorAttachment
    {
        let intent_id = ensure_deferred_document_write_intent(
            path,
            expected_current,
            content,
            source,
            DocumentWriteDeferredReason::EditorProjectionPending,
        )?;
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "repair_projection_late_editor_retained file={} source={} intent_id={} target_hash={} canonical_hash={} disk_hash={} recovery=await_editor_replica_no_disk_write_then_session_check",
                path.display(),
                source,
                intent_id,
                target_hash,
                agent_doc_hash::content_hash(&canonical),
                agent_doc_hash::content_hash(&disk),
            ),
        );
        return Err(retained_refusal(path, format!(
            "{source}: repair target for {} reached disk while the document was detached, then the editor registered with the exact pre-repair authority (intent_id={intent_id}, target_hash={target_hash}); the original compare-and-swap lineage is retained for reactive editor delivery. Run only agent-doc session-check for the existing binary-owned repair; do not resubmit, force disk, or recycle the controller; {RETAINED_FOR_RETRY_MARKER}",
            path.display(),
        )));
    }

    anyhow::ensure!(
        canonical == content && disk == content,
        "{source}: successful repair write for {} did not converge exactly before settling deferred lineage (expected_hash={}, canonical_hash={}, disk_hash={}, component_divergence={})",
        path.display(),
        target_hash,
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            &canonical, &disk,
        ),
    );
    unreachable!("exact repair convergence returned above")
}

/// Apply a binary/CP-authored document update to the live CRDT relay.
///
/// When an editor owns the document, this is the write-side companion to
/// [`try_resolve_current_document_content`]: the controller canonical replica is
/// updated first, with `expected_current` proving that the response was merged
/// against the current editor-buffer state. The real markdown file may then be
/// materialized as a projection of this relay state.
pub fn apply_cp_write_through_relay_authority(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CpRelayWrite>> {
    validate_canonical_document_target(file, content, source)?;
    if controller_document_mutation_in_progress()
        || agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file)
    {
        return agent_doc_crdt_relay_io::apply_cp_write_for_file(
            file,
            expected_current,
            content,
            source,
        );
    }
    agent_doc_controller_io::project_controller::apply_cp_write_via_controller_model_for_doc(
        file,
        expected_current,
        content,
        source,
    )
}

/// Verify that an editor receipt observes the controller-owned canonical text.
///
/// Editor visibility is downstream evidence only. It must never replace
/// canonical state, even when the editor is stale or the relay was rebuilt.
pub fn adopt_verified_editor_text_through_relay_authority(
    file: &Path,
    text: &str,
    source: &str,
) -> Result<Option<bool>> {
    let canonical = try_resolve_current_document_content(file, source)?;
    anyhow::ensure!(
        canonical == text,
        "{source}: refusing editor receipt that diverges from controller canonical for {} (canonical_hash={}, editor_hash={}); retained canonical projection remains authoritative",
        file.display(),
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(text),
    );
    Ok(Some(false))
}

pub fn apply_canonical_replace_if_attached(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CpRelayWrite>> {
    let started = std::time::Instant::now();
    // Keep the write budget as a portable lazily timeline. Individual controller
    // RPC timeouts are congestion signals inside this larger deadline, not a
    // reason to abandon an already-accepted compact/finalize mutation.
    let mut deadline = DeadlineCore::new(CRDT_WRITE_CONVERGENCE_TIMEOUT_MS);
    let mut frontier_backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
    let mut delivery_wait_elapsed = std::time::Duration::ZERO;
    let mut pending_target: Option<String> = None;
    let mut pending_write: Option<agent_doc_crdt_relay_io::CpRelayWrite> = None;
    let mut projection_observation = ProjectionObservationState::default();
    let mut wait_state = CrdtConvergenceState::TypingQuiescence;
    // `#crdtprojectionprofile`: accumulate time per wait state so a single write reports
    // where its convergence latency went, instead of only what state it happened
    // to be in at a 2s notice.
    let mut profile = CrdtConvergenceProfile::new(wait_state);
    let mut last_notice = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(2))
        .unwrap_or_else(std::time::Instant::now);

    loop {
        // Charge the previous iteration to the state it ended in (`#crdtprojectionprofile`).
        profile.tick(wait_state);
        let total_elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let delivery_wait_elapsed_ms =
            delivery_wait_elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        let controller_elapsed_ms =
            exclusive_controller_elapsed_ms(total_elapsed_ms, delivery_wait_elapsed_ms);
        deadline.tick(controller_elapsed_ms);
        if deadline.is_expired() {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_crdt_convergence_timeout file={} reason={} controller_timeout_ms={} delivery_wait_ms={} profile=[{}] recovery=retry_crdt_merge_no_legacy_replay",
                    file.display(),
                    wait_state,
                    CRDT_WRITE_CONVERGENCE_TIMEOUT_MS,
                    delivery_wait_elapsed_ms,
                    profile.render(wait_state),
                ),
            );
            anyhow::bail!(
                "{source}: CRDT convergence for {} did not settle within {}ms (reason={}); {}",
                file.display(),
                CRDT_WRITE_CONVERGENCE_TIMEOUT_MS,
                wait_state,
                RETAINED_FOR_RETRY_MARKER,
            );
        }

        // A CP write is issued only from a quiescent editor cut. Waiting here
        // happens in the caller, outside the controller RPC loop, so editor
        // deltas and delivery projections remain responsive while typing settles.
        if pending_target.is_none() {
            let remaining_ms =
                CRDT_WRITE_CONVERGENCE_TIMEOUT_MS.saturating_sub(controller_elapsed_ms);
            guard_visible_write_current_transition_with_budget(
                file,
                source,
                CRDT_WRITE_SETTLE_MS,
                remaining_ms.max(1),
            )
            .with_context(|| {
                format!(
                    "{source}: waiting for the pre-write editor delivery barrier for {}",
                    file.display()
                )
            })?;
        }

        let mut delivery_wait_cursor: Option<(u64, usize)> = None;
        let observed = match observe_live_editor_authority_after_model_ensure(file, source) {
            Ok(current) => Some(current),
            Err(err) if transient_convergence_backpressure_error(&err) => {
                wait_state = CrdtConvergenceState::ControllerModelBackpressure;
                // The authority/model boundary already records the failed
                // attempt. The common two-second notice below coalesces retry
                // telemetry instead of logging every controller/model poll.
                None
            }
            Err(err) => return Err(err),
        };

        if let Some(observed) = observed {
            match observed {
                agent_doc_crdt_relay_io::CurrentText::Detached => return Ok(None),
                agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
                    wait_state = CrdtConvergenceState::EditorAttachedModelMissing;
                }
                agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
                    wait_state = CrdtConvergenceState::EditorSyncPending;
                }
                agent_doc_crdt_relay_io::CurrentText::Current {
                    text: relay_text,
                    live_editors,
                    delivery_converged,
                    delivery_version,
                    ..
                } => {
                    delivery_wait_cursor = Some((delivery_version, live_editors));
                    if let Some(applied_target) = pending_target.as_ref() {
                        if delivery_converged && relay_text == *applied_target {
                            // A vanished replica makes the relay quorum
                            // vacuously converged. That is not editor-visible
                            // proof: the live IDE process can still hold an
                            // unsaved operator cut. Settle only if the editor
                            // already published/saved the exact target.
                            if !delivery_convergence_is_editor_visible(
                                live_editors,
                                durable_visible_write_content_proves_target(file, applied_target),
                            ) {
                                let relay_write = pending_write
                                    .as_ref()
                                    .expect("pending CRDT target must retain its write receipt");
                                let recycle_status = agent_doc_controller_io::project_controller::
                                        schedule_stale_editor_replica_cp_recycle(file, source);
                                return Err(retained_refusal(file, format!(
                                    "{source}: retained canonical target for {} after its editor replica disappeared (content_hash={}): zero-member delivery convergence is not visible-write proof; disk was not written; recycle_status={recycle_status}; recovery=await_editor_replica_no_disk_write_then_session_check; {RETAINED_FOR_RETRY_MARKER}",
                                    file.display(),
                                    relay_write.content_hash,
                                )));
                            }
                            let mut relay_write = pending_write
                                .take()
                                .expect("pending CRDT target must retain its write receipt");
                            relay_write.delivery_converged = true;
                            relay_write.live_editors = live_editors;
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_crdt_relay_projected file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true disk_projection=pending wait_ms={} profile=[{}] transport=crdt_only",
                                    file.display(),
                                    relay_write.content_hash,
                                    relay_write.update_bytes,
                                    relay_write.targets,
                                    live_editors,
                                    started.elapsed().as_millis(),
                                    profile.render(wait_state),
                                ),
                            );
                            return Ok(Some(relay_write));
                        }
                        if write_policy::decide_crdt_write_admission(delivery_converged)
                            == write_policy::CrdtWriteAdmission::WaitForDeliveryProjection
                        {
                            let relay_write = pending_write
                                .take()
                                .expect("pending CRDT target must retain its write receipt");
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_crdt_delivery_deferred file={} content_hash={} recovery=retained_async_editor_delivery operator_action=none driver=lazy_projection",
                                    file.display(),
                                    relay_write.content_hash,
                                ),
                            );
                            return Ok(Some(relay_write));
                        } else {
                            // A controller handoff can overlap the editor's application of
                            // the valid CP target. A replacement replica may then publish
                            // one transient, structurally incomplete buffer generation
                            // (observed as an exchange close without its open marker).
                            // That cut is not a new target and
                            // must never be fed through the semantic rebase below: doing so
                            // mislabels the malformed editor generation as a malformed
                            // compact target even though the exact valid target is already
                            // retained in Lazily state.
                            //
                            // Preserve the original receipt/intent and ask the replica
                            // reconciler to refill from the retained canonical frontier.
                            // Returning the still-unconverged receipt makes the outer write
                            // boundary stop before snapshot, disk, or commit effects.
                            if let Some(reason) = structurally_invalid_post_apply_editor_cut(
                                applied_target,
                                &relay_text,
                            ) {
                                if let Err(err) = reconcile_stalled_replicas(file, source) {
                                    agent_doc_ops_log_io::log_op(
                                        file,
                                        &format!(
                                            "{source}_post_apply_structural_cut_reconcile_failed file={} retained_hash={} observed_hash={} error={err:#}",
                                            file.display(),
                                            agent_doc_hash::content_hash(applied_target),
                                            agent_doc_hash::content_hash(&relay_text),
                                        ),
                                    );
                                }
                                let relay_write = pending_write
                                    .take()
                                    .expect("pending CRDT target must retain its write receipt");
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_post_apply_structural_cut_retained file={} retained_hash={} observed_hash={} reason={} action=await_replica_rebootstrap_no_rebase_no_disk_write",
                                        file.display(),
                                        relay_write.content_hash,
                                        agent_doc_hash::content_hash(&relay_text),
                                        reason,
                                    ),
                                );
                                return Ok(Some(relay_write));
                            }
                            // The editor may publish queue consumption, a new prompt,
                            // or other operator-owned bytes while ACKing our retained
                            // response. If semantic rebasing says that converged cut
                            // already contains the response, accept it byte-for-byte.
                            // Re-applying the stale whole-document target here creates
                            // a fresh CRDT transition on every closeout retry.
                            let editor_cut = editor_operator_cut_for_agent_rebase(
                                file,
                                expected_current,
                                &relay_text,
                                source,
                            );
                            let settled_target = rebase_agent_candidate_over_editor_cut(
                                expected_current,
                                content,
                                &editor_cut,
                            )
                            .with_context(|| {
                                format!(
                                    "{source}: failed to verify the acknowledged editor cut for {}",
                                    file.display()
                                )
                            })?;
                            let settled_target = canonicalize_and_validate_agent_rebase(
                                &settled_target,
                                content,
                                file,
                                source,
                            )?;
                            if settled_target == relay_text
                                && delivery_convergence_is_editor_visible(
                                    live_editors,
                                    durable_visible_write_content_proves_target(file, &relay_text),
                                )
                            {
                                reconcile_deferred_write_to_canonical_cut_if_needed(
                                    file,
                                    &relay_text,
                                    source,
                                )?;
                                let relay_write =
                                    projected_noop_relay_write(&relay_text, live_editors);
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_relay_semantic_noop file={} prior_target_hash={} acknowledged_hash={} live_editors={} delivery_converged=true action=accept_editor_cut_no_reapply wait_ms={}",
                                        file.display(),
                                        agent_doc_hash::content_hash(applied_target),
                                        relay_write.content_hash,
                                        live_editors,
                                        started.elapsed().as_millis(),
                                    ),
                                );
                                return Ok(Some(relay_write));
                            }

                            // Genuine operator text still leaves part of the target
                            // unapplied. Recompute from the original base/candidate
                            // against that cut, then issue one new CRDT delta.
                            pending_target = None;
                            pending_write = None;
                            projection_observation.reset();
                            frontier_backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
                            wait_state = CrdtConvergenceState::OperatorAdvancedAfterApply;
                            continue;
                        }
                    } else {
                        let effective_target = if relay_text == expected_current
                            || relay_text == content
                        {
                            content.to_string()
                        } else {
                            let editor_cut = editor_operator_cut_for_agent_rebase(
                                file,
                                expected_current,
                                &relay_text,
                                source,
                            );
                            let merged = rebase_agent_candidate_over_editor_cut(
                        expected_current,
                        content,
                        &editor_cut,
                    )
                    .with_context(|| {
                        format!(
                            "{source}: failed to CRDT-merge the settled editor version for {}",
                                        file.display()
                                    )
                                })?;
                            canonicalize_and_validate_agent_rebase(&merged, content, file, source)?
                        };
                        let effective_target = canonicalize_and_validate_agent_rebase(
                            &effective_target,
                            content,
                            file,
                            source,
                        )?;

                        // Buffer authority and delivery health are separate:
                        // keep the live IDE PID as the disk fence, but do not
                        // issue a canonical delivery to a component whose
                        // replica/ACK worker stopped heartbeating. Retain the
                        // exact merged target and refresh the supervisor/plugin
                        // bridge at the next safe capture-backed checkpoint.
                        let editor_delivery_worker_stale =
                            agent_doc_controller_io::project_controller::
                                reliable_sync_editor_live_for_file(file)
                                && agent_doc_controller_io::project_controller::
                                    live_editor_registration_for_file(file)
                                    .ok()
                                    .flatten()
                                    .is_none();
                        if editor_delivery_worker_stale {
                            let intent_id = ensure_deferred_document_write_intent(
                                file,
                                &relay_text,
                                &effective_target,
                                source,
                                DocumentWriteDeferredReason::EditorDeliveryWorkerStale,
                            )?;
                            let recycle_status = agent_doc_controller_io::project_controller::
                                    schedule_stale_editor_replica_cp_recycle(file, source);
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_editor_delivery_worker_stale file={} intent_id={} content_hash={} authority=live_ide_pid delivery=fresh_heartbeat_missing recovery=cp_recycle_no_disk_write recycle_status={recycle_status}",
                                    file.display(),
                                    intent_id,
                                    agent_doc_hash::content_hash(&effective_target),
                                ),
                            );
                            return Err(retained_refusal(file, format!(
                                "{source}: retained the canonical write for {} in CRDT + Lazily state (intent_id={intent_id}), but the live editor delivery worker heartbeat is stale; disk was not written; recycle_status={recycle_status}",
                                file.display(),
                            )));
                        }

                        let zero_replica_visible_write_proven = live_editors == 0
                            && relay_text == effective_target
                            && durable_visible_write_content_proves_target(file, &effective_target);
                        if live_editors == 0 {
                            if zero_replica_visible_write_proven {
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_zero_replica_visible_write_proven file={} content_hash={} authority=lazily_editor_ack action=allow_matching_disk_projection",
                                        file.display(),
                                        agent_doc_hash::content_hash(&effective_target),
                                    ),
                                );
                            } else {
                                // Admit no new CRDT frontier until it has at
                                // least one delivery recipient. Previously this
                                // guard covered only an exact canonical no-op;
                                // a genuine queue/dedupe mutation still crossed
                                // the relay with `targets=0`, manufacturing the
                                // retained intent that the next preflight could
                                // never reconcile. Preserve the merged target in
                                // the durable journal and let replica
                                // re-registration replay it.
                                let intent_id = ensure_deferred_document_write_intent(
                                    file,
                                    &relay_text,
                                    &effective_target,
                                    source,
                                    DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
                )?;
                                let recycle_status = agent_doc_controller_io::project_controller::
                    schedule_stale_editor_replica_cp_recycle(file, source);
                                return Err(retained_refusal(file, format!(
                                    "{source}: deferred write for {} in Lazily state (intent_id={intent_id}): the editor is the current authority, but no editor replica was registered with the relay; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_session_check; run only agent-doc session-check for the existing binary-owned capture; do not resubmit finalize, write --commit, or --force-disk; {RETAINED_FOR_RETRY_MARKER}",
                                    file.display(),
                                )));
                            }
                        }

                        if effective_target == relay_text && !delivery_converged {
                            // The canonical already contains this exact target.
                            // Delivery is a keyed lazy projection; return its
                            // retained receipt without polling or issuing a
                            // recovery request.
                            reconcile_deferred_write_to_canonical_cut_if_needed(
                                file,
                                &effective_target,
                                source,
                            )?;
                            let mut relay_write =
                                projected_noop_relay_write(&effective_target, live_editors);
                            relay_write.delivery_converged = false;
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_crdt_relay_noop_delivery_deferred file={} content_hash={} live_editors={} delivery_converged=false action=retain_existing_delivery_no_reapply driver=lazy_projection",
                                    file.display(),
                                    relay_write.content_hash,
                                    live_editors,
                                ),
                            );
                            return Ok(Some(relay_write));
                        }

                        if delivery_converged
                            && effective_target == relay_text
                            && canonical_disk_projection_is_exact(file, &effective_target)
                            && delivery_convergence_is_editor_visible(
                                live_editors,
                                durable_visible_write_content_proves_target(
                                    file,
                                    &effective_target,
                                ),
                            )
                        {
                            reconcile_deferred_write_to_canonical_cut_if_needed(
                                file,
                                &effective_target,
                                source,
                            )?;
                            let relay_write =
                                projected_noop_relay_write(&effective_target, live_editors);
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_crdt_relay_exact_noop file={} content_hash={} live_editors={} delivery_converged=true action=skip_compare_and_swap wait_ms={}",
                                    file.display(),
                                    relay_write.content_hash,
                                    live_editors,
                                    started.elapsed().as_millis(),
                                ),
                            );
                            return Ok(Some(relay_write));
                        }

                        // Retain every accepted candidate before it crosses the
                        // CRDT delivery boundary. This is the Lazily handoff that
                        // lets a client ACK retry or forced replica re-register
                        // finish after this process exits without asking an agent
                        // to reconstruct or duplicate the response.
                        let retained_intent_id = ensure_deferred_document_write_intent(
                            file,
                            &relay_text,
                            &effective_target,
                            source,
                            DocumentWriteDeferredReason::EditorProjectionPending,
                        )?;

                        match apply_cp_write_through_relay_authority(
                            file,
                            &relay_text,
                            &effective_target,
                            source,
                        ) {
                            Ok(None) => return Ok(None),
                            Ok(Some(relay_write))
                                if relay_write.applied && relay_write.targets == 0 =>
                            {
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_write_deferred file={} intent_id={} content_hash={} targets=0 live_editors=0 recovery=await_editor_replica_no_disk_write",
                                        file.display(),
                                        retained_intent_id,
                                        relay_write.content_hash,
                                    ),
                                );
                                let recycle_status = agent_doc_controller_io::project_controller::
                                    schedule_stale_editor_replica_cp_recycle(file, source);
                                return Err(await_editor_replica_no_disk_write(format!(
                                    "{source}: retained the canonical write for {} in CRDT + Lazily state (intent_id={retained_intent_id}), but the editor replica disappeared during delivery admission; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_session_check; run only agent-doc session-check for the existing binary-owned capture; do not resubmit finalize, write --commit, or --force-disk; {} {RETAINED_FOR_RETRY_MARKER}",
                                    file.display(),
                                    retained_write_remedy_for(file),
                                )));
                            }
                            Ok(Some(mut relay_write)) if relay_write.delivery_converged => {
                                relay_write.live_editors = live_editors;
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_relay_projected file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true disk_projection=pending wait_ms={} profile=[{}] transport=crdt_only",
                                        file.display(),
                                        relay_write.content_hash,
                                        relay_write.update_bytes,
                                        relay_write.targets,
                                        live_editors,
                                        started.elapsed().as_millis(),
                                        profile.render(wait_state),
                                    ),
                                );
                                return Ok(Some(relay_write));
                            }
                            Ok(Some(relay_write)) => {
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_delivery_deferred file={} content_hash={} recovery=retained_async_editor_delivery operator_action=none driver=lazy_projection",
                                        file.display(),
                                        relay_write.content_hash,
                                    ),
                                );
                                return Ok(Some(relay_write));
                            }
                            Err(err) => {
                                let detail = format!("{err:#}");
                                if detail.contains("recovery=retry_crdt_merge")
                                    || detail.contains("editor_sync_pending")
                                {
                                    wait_state = CrdtConvergenceState::CompareAndSwapRaced;
                                    agent_doc_ops_log_io::log_op(
                                        file,
                                        &format!(
                                            "{source}_crdt_write_coalesced_retry file={} reason={} backoff_ms={} recovery=wait_settle_remerge",
                                            file.display(),
                                            wait_state,
                                            frontier_backoff_ms,
                                        ),
                                    );
                                } else {
                                    return Err(err);
                                }
                            }
                        }
                    }
                }
            }
        }

        if last_notice.elapsed() >= std::time::Duration::from_secs(2) {
            eprintln!(
                "[write] Waiting for typing/CRDT versions to settle for {} (reason={}, elapsed={}ms)",
                file.display(),
                wait_state,
                started.elapsed().as_millis(),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_crdt_convergence_wait file={} reason={} elapsed_ms={} backoff_ms={} strategy=coalesced_latest_frontier",
                    file.display(),
                    wait_state,
                    started.elapsed().as_millis(),
                    frontier_backoff_ms,
                ),
            );
            last_notice = std::time::Instant::now();
        }
        let total_elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let delivery_wait_elapsed_ms =
            delivery_wait_elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        let controller_elapsed_ms =
            exclusive_controller_elapsed_ms(total_elapsed_ms, delivery_wait_elapsed_ms);
        let remaining_ms = CRDT_WRITE_CONVERGENCE_TIMEOUT_MS.saturating_sub(controller_elapsed_ms);
        if wait_state == CrdtConvergenceState::DeliveryProjectionPending
            && let Some((delivery_version, live_editors)) = delivery_wait_cursor
        {
            let _ = projection_observation.wait_for_delivery_change_charged(
                DeliveryChangeWait {
                    file,
                    source,
                    live_editors,
                    delivery_version,
                    signal_immediately: false,
                    max_wait_ms: None,
                },
                &mut delivery_wait_elapsed,
            )?;
            continue;
        }
        let sleep_for = std::time::Duration::from_millis(frontier_backoff_ms.min(remaining_ms));
        if !sleep_for.is_zero() {
            std::thread::sleep(sleep_for);
        }
        frontier_backoff_ms = CRDT_WRITE_BACKOFF_POLICY.next_ms(frontier_backoff_ms, false);
    }
}

fn projected_noop_relay_write(
    content: &str,
    live_editors: usize,
) -> agent_doc_crdt_relay_io::CpRelayWrite {
    agent_doc_crdt_relay_io::CpRelayWrite {
        applied: false,
        content_len: content.len(),
        content_hash: agent_doc_hash::content_hash(content),
        update_bytes: 0,
        targets: 0,
        live_editors,
        delivery_converged: true,
    }
}

pub fn reconcile_deferred_write_to_canonical_cut_if_needed(
    file: &Path,
    acknowledged: &str,
    source: &str,
) -> Result<()> {
    let acknowledged_hash = agent_doc_hash::content_hash(acknowledged);
    if pending_document_write(file)
        .is_some_and(|pending| !pending.target_hash.eq_ignore_ascii_case(&acknowledged_hash))
    {
        ensure_deferred_document_write_intent(
            file,
            acknowledged,
            acknowledged,
            source,
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )?;
    }
    Ok(())
}

fn agent_projection_integrity_valid(content: &str) -> bool {
    if agent_doc_element::element::structural_corruption_reason(content).is_some() {
        return false;
    }
    let code_ranges = agent_doc_element::element::find_code_ranges(content);
    let boundary_count = content
        .match_indices("<!-- agent:boundary:")
        .filter(|(start, _)| {
            !code_ranges
                .iter()
                .any(|&(code_start, code_end)| *start >= code_start && *start < code_end)
        })
        .count();
    let boundary_singleton = boundary_count <= 1
        && agent_doc_template::collapse_adjacent_boundary_markers(content)
            .is_ok_and(|normalized| normalized == content);
    let single_exchange = agent_doc_template::repair_duplicate_exchange_opener(content)
        .ok()
        .flatten()
        .is_none();
    boundary_singleton && single_exchange
}

/// Rebase one binary-owned document candidate onto Lazily's current operator
/// cut. Response delivery is a semantic cell, not a whole-document replay:
/// once that response is already present, the live cut wins byte-for-byte; if
/// it is still missing, append only that response cell. Other candidates keep
/// the existing component-aware three-way merge.
fn rebase_agent_candidate_over_editor_cut(
    merge_base: &str,
    agent_target: &str,
    editor_cut: &str,
) -> Result<String> {
    write_policy::rebase_agent_candidate_over_editor_cut(merge_base, agent_target, editor_cut)
}

/// Remove the earlier of exactly two standalone boundary markers inside the
/// one parseable exchange component. Boundary markers are binary-owned
/// protocol frontiers; retaining the last frontier is the same ordering rule
/// used by normal response-cell rendering. Every other byte remains from the
/// current Lazily/editor cut, and ambiguous shapes fail closed.
fn remove_stale_standalone_exchange_boundary(content: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let exchanges = components
        .iter()
        .filter(|component| component.name == "exchange")
        .collect::<Vec<_>>();
    let [exchange] = exchanges.as_slice() else {
        return None;
    };
    let ignored = agent_doc_element::element::find_code_ranges(content)
        .into_iter()
        .chain(agent_doc_element::element::find_quoted_ranges(content))
        .collect::<Vec<_>>();
    let mut boundaries = Vec::new();
    let mut offset = 0usize;

    for line in content.split_inclusive('\n') {
        let line_start = offset;
        let line_end = line_start + line.len();
        offset = line_end;
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let marker_start = line_start + leading;
        if marker_start < exchange.open_end
            || marker_start >= exchange.close_start
            || ignored
                .iter()
                .any(|&(start, end)| marker_start >= start && marker_start < end)
        {
            continue;
        }

        let trimmed = line.trim();
        let Some(id) = trimmed
            .strip_prefix("<!-- agent:boundary:")
            .and_then(|rest| rest.strip_suffix(" -->"))
            .map(str::trim)
        else {
            continue;
        };
        if id.is_empty() || agent_doc_element::id::format_boundary_marker(id) != trimmed {
            return None;
        }
        boundaries.push((line_start, line_end));
    }

    let [(start, end), _] = boundaries.as_slice() else {
        return None;
    };
    let mut repaired = String::with_capacity(content.len() - (end - start));
    repaired.push_str(&content[..*start]);
    repaired.push_str(&content[*end..]);
    agent_projection_integrity_valid(&repaired).then_some(repaired)
}

/// Normalize the narrow structural transient produced when a retained response
/// replay duplicates response cells, protocol boundary markers, or a component
/// closing marker. This is deliberately pure: callers validate and write the
/// returned target through the current document authority before entering their
/// generic integrity gate.
pub fn normalize_recoverable_response_replay_duplication(content: &str) -> Option<String> {
    let response_shell_repaired =
        agent_doc_turn::response_replay::repair_stranded_duplicate_response_headings(content);
    if agent_projection_integrity_valid(content) && response_shell_repaired == content {
        return None;
    }
    // `#boundarysplice`: restore a boundary terminator a cell merge welded into
    // the following line BEFORE the other repairs, since a malformed agent
    // comment makes the document unparseable and blocks every one of them. The
    // boundary is transient binary-owned scaffolding and the repair is lossless,
    // so this cannot touch operator prose.
    let mut normalized =
        agent_doc_element::element::repair_malformed_exchange_close_comment(content)
            .unwrap_or_else(|| content.to_string());
    normalized = agent_doc_element::element::repair_malformed_boundary_comment(&normalized)
        .unwrap_or(normalized);
    normalized = agent_doc_merge::response_cell::deduplicate_response_cells(&normalized)
        .ok()
        .flatten()
        .unwrap_or(normalized);
    // A retained replay can strand a second heading for a response topic whose
    // earlier cell already has a body. This is the same narrow, lossless shell
    // repaired by compact: remove only the empty duplicate heading, never a
    // unique heading or any response/operator body.
    normalized =
        agent_doc_turn::response_replay::repair_stranded_duplicate_response_headings(&normalized);
    if !agent_projection_integrity_valid(&normalized)
        && let Some(repaired) = remove_stale_standalone_exchange_boundary(&normalized)
    {
        normalized = repaired;
    }
    if !agent_projection_integrity_valid(&normalized)
        && let Some(repaired) =
            agent_doc_element::element::repair_duplicated_document_suffix(&normalized)
    {
        normalized = repaired;
    }
    if !agent_projection_integrity_valid(&normalized) {
        normalized = agent_doc_element::element::repair_single_unmatched_duplicate_component_close(
            &normalized,
        )?;
    }
    (normalized != content && agent_projection_integrity_valid(&normalized)).then_some(normalized)
}

/// Collapse the exact full-document concatenation produced when two raw editor
/// generations were briefly registered as independent CRDT heads. One branch
/// must be byte-identical to the retained pending target; the other is treated
/// as the current operator cut and receives the pending semantic change through
/// the normal three-way rebase. Ambiguous shapes fail closed.
fn recover_concatenated_document_generations(
    file: &Path,
    content: &str,
    expected: &str,
    target: &str,
    source: &str,
) -> Result<Option<String>> {
    if content == target || target.is_empty() || expected.is_empty() {
        return Ok(None);
    }
    if !agent_projection_integrity_valid(expected) || !agent_projection_integrity_valid(target) {
        return Ok(None);
    }
    let expected_session = agent_doc_frontmatter::frontmatter::session_id_from_content(expected);
    let target_session = agent_doc_frontmatter::frontmatter::session_id_from_content(target);
    if expected_session.is_none() || expected_session != target_session {
        return Ok(None);
    }

    let prefix_editor = content
        .strip_suffix(target)
        .filter(|candidate| !candidate.is_empty());
    let suffix_editor = content
        .strip_prefix(target)
        .filter(|candidate| !candidate.is_empty());
    let editor_generation = match (prefix_editor, suffix_editor) {
        (Some(prefix), Some(suffix)) if prefix == target && suffix == target => target,
        (Some(prefix), None) => prefix,
        (None, Some(suffix)) => suffix,
        _ => return Ok(None),
    };
    if !agent_projection_integrity_valid(editor_generation)
        || agent_doc_frontmatter::frontmatter::session_id_from_content(editor_generation)
            != expected_session
    {
        return Ok(None);
    }

    if editor_generation == target {
        return Ok(Some(target.to_string()));
    }
    let editor_cut =
        editor_operator_cut_for_agent_rebase(file, expected, editor_generation, source);
    let merged = rebase_agent_candidate_over_editor_cut(expected, target, &editor_cut)?;
    let canonical = canonicalize_and_validate_agent_rebase(&merged, target, file, source)?;
    Ok((canonical != content).then_some(canonical))
}

/// File-aware replay normalization. In addition to the narrow pure repairs,
/// this may collapse two concatenated logical editor generations, but only when
/// the durable pending intent proves one complete branch byte-for-byte.
pub fn normalize_recoverable_response_replay_duplication_for_file(
    file: &Path,
    content: &str,
    source: &str,
) -> Result<Option<String>> {
    for checkpoint in agent_doc_cycle_state_io::load_recent_captured_response_checkpoints(file, 4)?
    {
        let Some(baseline) = checkpoint.baseline_content.as_deref() else {
            continue;
        };
        let materialization =
            agent_doc_template::response_materialization::response_materialization_probe_from_response(
                &checkpoint.response_body,
            );
        if let Some(recovered) =
            agent_doc_merge::response_cell::deduplicate_captured_response_replays(
                content,
                baseline,
                &materialization,
            )?
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_captured_response_replay_candidate_prepared file={} cycle_id={} capture_id={} observed_hash={} target_hash={} strategy=capture_baseline_scoped",
                    file.display(),
                    checkpoint.cycle_id,
                    checkpoint.capture_id,
                    agent_doc_hash::content_hash(content),
                    agent_doc_hash::content_hash(&recovered),
                ),
            );
            return Ok(Some(recovered));
        }
    }
    if let Some(pending) = pending_document_write(file)
        && let Some(expected) = pending.expected_content.as_deref()
        && agent_doc_hash::content_hash(expected) == pending.expected_hash
        && agent_doc_hash::content_hash(&pending.target_content) == pending.target_hash
        && let Some(recovered) = recover_concatenated_document_generations(
            file,
            content,
            expected,
            &pending.target_content,
            source,
        )?
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_concatenated_editor_generations_candidate_prepared file={} intent_id={} observed_hash={} target_hash={} strategy=pending_intent_semantic_rebase",
                file.display(),
                pending.intent_id,
                agent_doc_hash::content_hash(content),
                agent_doc_hash::content_hash(&recovered),
            ),
        );
        return Ok(Some(recovered));
    }
    Ok(normalize_recoverable_response_replay_duplication(content))
}

/// Retire legacy whole-document reconnect intents only when the current
/// authority is an exact repetition of a trusted, structurally valid target
/// and every retained payload is itself only that target (or repetitions of
/// it). This is the event-backed escape hatch for historical
/// `document_write_deferred` rows: it never edits state.db directly and it
/// refuses to discard an intent containing text absent from the trusted cut.
pub fn retire_redundant_doubled_document_write_intents(
    file: &Path,
    observed: &str,
    trusted_target: &str,
    source: &str,
) -> Result<usize> {
    validate_canonical_document_target(file, trusted_target, source)?;
    anyhow::ensure!(
        !trusted_target.is_empty()
            && observed.len() == trusted_target.len() * 2
            && observed
                .strip_prefix(trusted_target)
                .is_some_and(|suffix| suffix == trusted_target),
        "{source}: refusing deferred-intent retirement for {} without an exact 2x trusted projection (observed_hash={}, trusted_hash={})",
        file.display(),
        agent_doc_hash::content_hash(observed),
        agent_doc_hash::content_hash(trusted_target),
    );

    let journal = pending_document_write_journal(file);
    for intent in &journal {
        anyhow::ensure!(
            agent_doc_hash::content_hash(&intent.target_content)
                .eq_ignore_ascii_case(&intent.target_hash),
            "{source}: refusing to retire deferred document write {} for {} because its retained payload hash is invalid",
            intent.intent_id,
            file.display(),
        );
        let target_is_redundant = intent.target_content == trusted_target
            || (intent.target_content.len() == trusted_target.len() * 2
                && intent
                    .target_content
                    .strip_prefix(trusted_target)
                    .is_some_and(|suffix| suffix == trusted_target));
        anyhow::ensure!(
            target_is_redundant,
            "{source}: refusing to retire deferred document write {} for {} because its payload contains content outside the trusted target (intent_hash={}, trusted_hash={})",
            intent.intent_id,
            file.display(),
            intent.target_hash,
            agent_doc_hash::content_hash(trusted_target),
        );
    }

    let retired = journal.len();
    clear_all_deferred_document_write_intents(file, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_redundant_doubled_intents_retired file={} retired_intents={} observed_hash={} trusted_hash={} proof=exact_2x_no_unique_payload_text",
            file.display(),
            retired,
            agent_doc_hash::content_hash(observed),
            agent_doc_hash::content_hash(trusted_target),
        ),
    );
    Ok(retired)
}

fn canonical_document_target_is_valid(content: &str) -> bool {
    agent_doc_element::element::structural_corruption_reason(content).is_none()
        && agent_projection_integrity_valid(content)
        && agent_doc_template::guard_no_conversation_content_inside_tracked_components(content)
            .is_ok()
}

fn validate_canonical_document_target(file: &Path, content: &str, source: &str) -> Result<()> {
    if let Some(reason) = agent_doc_element::element::structural_corruption_reason(content) {
        anyhow::bail!(
            "{source}: refusing structurally invalid canonical target for {} ({reason}); current Lazily/editor authority is unchanged and pending intents remain retained",
            file.display(),
        );
    }
    anyhow::ensure!(
        agent_projection_integrity_valid(content),
        "{source}: refusing structurally invalid canonical target for {} (duplicate exchange or boundary marker); current Lazily/editor authority is unchanged and pending intents remain retained",
        file.display(),
    );
    agent_doc_template::guard_no_conversation_content_inside_tracked_components(content)
        .with_context(|| {
            format!(
                "{source}: refusing semantically corrupt canonical target for {}; current Lazily/editor authority is unchanged and pending intents remain retained",
                file.display()
            )
        })?;
    Ok(())
}

/// Classify a structurally incomplete editor generation observed after a valid
/// CP target has already crossed the relay.
///
/// The retained target is validated before relay mutation. Therefore a
/// different malformed cut at this stage belongs to the editor/re-registration
/// side of the handoff and is not eligible for semantic response rebasing.
fn structurally_invalid_post_apply_editor_cut(
    retained_target: &str,
    observed_editor_cut: &str,
) -> Option<String> {
    if retained_target == observed_editor_cut {
        return None;
    }
    agent_doc_element::element::structural_corruption_reason(observed_editor_cut)
}

/// Resolve the operator-authored editor cut independently from agent projection
/// bytes. A live IDE buffer normally wins as-is. If it is structurally poisoned
/// by a prior non-operator CP projection (duplicate boundary/exchange), and the
/// durable operator-op stream can be replayed exactly from the expected base,
/// use that replay as the authoritative editor branch. This preserves operator
/// text such as `queue: stop` without accepting duplicated agent content.
fn editor_operator_cut_for_agent_rebase(
    file: &Path,
    expected_base: &str,
    observed_editor: &str,
    source: &str,
) -> String {
    let Ok(Some(ops)) = agent_doc_op_capture_io::editor_ops_for_base(file, expected_base) else {
        return observed_editor.to_string();
    };
    let Some(operator_cut) = agent_doc_merge::crdt::replay_editor_ops(expected_base, &ops) else {
        return observed_editor.to_string();
    };
    if operator_cut == observed_editor || canonical_document_target_is_valid(observed_editor) {
        return observed_editor.to_string();
    }
    if !canonical_document_target_is_valid(&operator_cut) {
        return observed_editor.to_string();
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_operator_cut_reconstructed file={} ops={} observed_hash={} operator_hash={} reason=invalid_non_operator_editor_projection recovery=replay_operator_ops_then_agent_intents",
            file.display(),
            ops.len(),
            agent_doc_hash::content_hash(observed_editor),
            agent_doc_hash::content_hash(&operator_cut),
        ),
    );
    operator_cut
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEditorCutReconciliation {
    pub content: String,
    pub replayed_editor_ops: bool,
}

/// Materialize a durable operator-op epoch when a temporarily detached editor
/// left disk at the cycle base. The disk bytes remain the caller's CAS
/// expectation; this returned cut is the semantic current branch used for
/// response merge and snapshot construction.
///
/// `#pauseddeletetombstone`: an editor can report a delete immediately before
/// its reliable-sync lease disappears (for example while a capacity-paused
/// agent is resumed). Treating `observed_current == expected_base` as "no
/// concurrent edits" would bypass the op-aware merge and resurrect the deleted
/// queue rows from the captured response branch.
pub fn reconcile_pending_editor_cut(
    file: &Path,
    expected_base: &str,
    observed_current: &str,
    source: &str,
) -> Result<PendingEditorCutReconciliation> {
    if observed_current != expected_base {
        return Ok(PendingEditorCutReconciliation {
            content: observed_current.to_string(),
            replayed_editor_ops: false,
        });
    }
    let Some(ops) = agent_doc_op_capture_io::editor_ops_for_base(file, expected_base)? else {
        return Ok(PendingEditorCutReconciliation {
            content: observed_current.to_string(),
            replayed_editor_ops: false,
        });
    };
    let Some(operator_cut) = agent_doc_merge::crdt::replay_editor_ops(expected_base, &ops) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_pending_editor_cut_rejected file={} ops={} base_hash={} reason=op_replay_failed",
                file.display(),
                ops.len(),
                agent_doc_hash::content_hash(expected_base),
            ),
        );
        return Ok(PendingEditorCutReconciliation {
            content: observed_current.to_string(),
            replayed_editor_ops: false,
        });
    };
    validate_canonical_document_target(file, &operator_cut, source)?;
    if operator_cut == observed_current {
        return Ok(PendingEditorCutReconciliation {
            content: observed_current.to_string(),
            replayed_editor_ops: false,
        });
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_pending_editor_cut_replayed file={} ops={} base_hash={} operator_hash={} strategy=durable_editor_ops_before_detached_base_shortcut",
            file.display(),
            ops.len(),
            agent_doc_hash::content_hash(expected_base),
            agent_doc_hash::content_hash(&operator_cut),
        ),
    );
    Ok(PendingEditorCutReconciliation {
        content: operator_cut,
        replayed_editor_ops: true,
    })
}

/// Heal a welded boundary marker AND re-establish the single-boundary invariant
/// (`#boundarysplice`).
///
/// Restoring a terminator can reveal a *second* structurally valid boundary — the
/// repaired one plus the document's real one — which immediately trips
/// `duplicate_exchange_boundary` and leaves the document just as wedged, one gate
/// further along. The boundary is binary-owned scaffolding that
/// `reposition_boundary_to_end_*` already rewrites at will, so collapsing to the
/// single canonical marker at the end of the exchange is the correct completion of
/// the repair rather than a second guess.
///
/// Returns `None` when there was no welded marker, so sound documents are never
/// rewritten.
fn heal_welded_boundary(content: &str) -> Option<String> {
    let mut repaired = agent_doc_element::element::repair_malformed_exchange_close_comment(content)
        .unwrap_or_else(|| content.to_string());
    repaired = agent_doc_element::element::repair_malformed_boundary_comment(&repaired)
        .unwrap_or(repaired);
    (repaired != content).then(|| agent_doc_template::reposition_boundary_to_end_clean(&repaired))
}

/// Repair legacy binary-owned scaffolding before the structural adoption gate.
///
/// Boundary corruption and a queue close marker welded to a progressive typing
/// suffix were produced by the same historical component-composition path, so
/// every durable-intent recovery seam must handle both before validation.
fn heal_welded_scaffolding(content: &str) -> Option<String> {
    let mut repaired = agent_doc_element::element::repair_welded_queue_close_marker(content)
        .unwrap_or_else(|| content.to_string());
    repaired = heal_welded_boundary(&repaired).unwrap_or(repaired);
    (repaired != content).then_some(repaired)
}

fn canonicalize_and_validate_agent_rebase(
    merged: &str,
    response_branch: &str,
    file: &Path,
    source: &str,
) -> Result<String> {
    let canonical =
        agent_doc_template::canonicalize_boundary_after_document_merge(merged, response_branch);
    // `#boundarysplice`: canonicalization is the last seam before the structural
    // gate, and a welded boundary marker can enter from either side of the merge —
    // the editor cut OR a retained intent target replayed out of `state.db`.
    // Healing only the intake leaves the poisoned copy in the durable intent, so
    // every reconnect re-fails identically and the document stays wedged forever.
    let canonical = heal_welded_scaffolding(&canonical).unwrap_or(canonical);
    validate_canonical_document_target(file, &canonical, source)?;
    Ok(canonical)
}

fn atomic_write_authority_raw(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;

    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    record_document_write_provenance(path, content);
    // `#preflightprojpass`: the other write chokepoint. Force-disk writes reach
    // disk without passing through the rebased-write path, so they invalidate
    // here; the rebased path invalidating again is a harmless epoch bump.
    current_document_projection::invalidate_current_document_projection(path);
    Ok(())
}

pub fn record_document_write_provenance(path: &Path, content: &str) {
    if !agent_doc_document_realtime::write_authority::is_visible_document(path) {
        return;
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return;
    };
    let write_id = uuid::Uuid::new_v4().to_string();
    let content_hash = agent_doc_hash::content_hash(content);
    let generation = next_monotonic_time_epoch(&DOCUMENT_DISK_WRITE_EPOCH);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("document-disk-write:{document_hash}:{generation}:{write_id}"),
        agent_doc_state_backbone::StateFact::DocumentDiskWriteObserved {
            document_hash,
            generation,
            content_len: content.len().try_into().unwrap_or(u64::MAX),
            content_hash,
            write_id,
            actor: "agent".to_string(),
        },
    );
    if let Err(e) = publish_reactive_state_event(&project_root, &event) {
        eprintln!(
            "[write] WARNING: failed to record write provenance for {}: {}",
            path.display(),
            e
        );
    }
}

fn log_fence_count_drop_if_any(path: &Path, new_content: &str) {
    let Some(old_content) = std::fs::read_to_string(path).ok() else {
        return;
    };
    let old_fences =
        agent_doc_document::write_normalization::count_code_fence_openings(&old_content);
    let new_fences =
        agent_doc_document::write_normalization::count_code_fence_openings(new_content);
    if new_fences < old_fences {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "fence_count_dropped file={} old_fences={} new_fences={} old_len={} new_len={}",
                path.display(),
                old_fences,
                new_fences,
                old_content.len(),
                new_content.len(),
            ),
        );
    }
}

// ── Rung 2 (`#rtwfeed`): CP-owned CRDT current-document feed ──
//
// Rung 1 above is the pure authority decision over a trusted `BufferState`.
// Rung 2 is the durable source of that state: the CP-owned CRDT/lazily model.
// Plugin reports are transport inputs only; Lazily is the sole live editor
// authority and no filesystem projection participates in this path.

fn next_document_authority_epoch() -> u64 {
    next_monotonic_time_epoch(&DOCUMENT_AUTHORITY_EPOCH)
}

fn next_monotonic_time_epoch(counter: &AtomicU64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    loop {
        let current = counter.load(Ordering::Relaxed);
        let next = now.max(current.saturating_add(1));
        match counter.compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(_) => continue,
        }
    }
}

fn record_document_authority(
    file: &std::path::Path,
    source: &str,
    authority: agent_doc_state_backbone::DocumentAuthority,
    reason: &str,
    content_hash: Option<String>,
    editor_id: Option<String>,
) {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "document_authority_state_event_skipped file={} source={} reason=no_project_root",
                file.display(),
                source,
            ),
        );
        return;
    };
    let observation = DocumentAuthorityObservation {
        source: source.to_string(),
        authority,
        reason: reason.to_string(),
        content_hash: content_hash.clone(),
        editor_id: editor_id.clone(),
    };
    let mut observations = DOCUMENT_AUTHORITY_OBSERVATIONS.lock();
    if observations.get(&canonical) == Some(&observation) {
        return;
    }
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let authority_epoch = next_document_authority_epoch();
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("document-authority-{document_hash}-{authority_epoch}-{source}"),
        agent_doc_state_backbone::StateFact::DocumentAuthorityObserved {
            document_hash,
            authority,
            authority_epoch,
            source: source.to_string(),
            reason: reason.to_string(),
            content_hash,
            editor_id,
        },
    );
    match publish_reactive_state_event(&project_root, &event) {
        Ok(_) => {
            observations.insert(canonical, observation);
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_authority_state_event_error file={} source={} error={}",
                    file.display(),
                    source,
                    e,
                ),
            );
        }
    }
}

/// Return the durable deferred write for `file`, if one still awaits editor
/// acknowledgement. Consumers use this to extend the same Lazily lineage
/// instead of starting a competing relay write during closeout.
pub fn pending_document_write(
    file: &Path,
) -> Option<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    agent_doc_cycle_state_io::load_document_projection(file)
        .ok()??
        .document
        .pending_write
        .clone()
}

/// The closeout stage that overtook `intent`, if its own successor converged
/// after it (`#adwritesourceenum`).
///
/// Derived by the projection, which owns the write ordinals — settlement
/// observes only content planes and cannot tell this cycle's later stage from
/// the previous cycle's.
pub fn superseding_closeout_stage(
    file: &Path,
    intent: &agent_doc_state_backbone::DocumentWriteIntentProjection,
) -> Option<agent_doc_state_backbone::CloseoutStage> {
    agent_doc_cycle_state_io::load_document_projection(file)
        .ok()??
        .document
        .superseding_closeout_stage(intent)
}

/// Ordered deferred agent changes for `file`. Newer targets are normally
/// cumulative, but retaining each intent lets reconnect replay an earlier
/// same-component mutation (for example `--backlog-add`) even if a later
/// whole-document merge accidentally omitted it.
pub fn pending_document_write_journal(
    file: &Path,
) -> Vec<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    let Some(document) = agent_doc_cycle_state_io::load_document_projection(file)
        .ok()
        .flatten()
    else {
        return Vec::new();
    };
    if document.document.pending_write_journal.is_empty() {
        document.document.pending_write.into_iter().collect()
    } else {
        document.document.pending_write_journal
    }
}

/// Return the independent durable candidate created by an external disk write
/// while an editor buffer is open. This lineage must never replace a pending
/// agent response write.
pub fn pending_external_disk_candidate(
    file: &Path,
) -> Option<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    agent_doc_cycle_state_io::load_document_projection(file)
        .ok()??
        .document
        .pending_external_disk
        .clone()
}

/// Hash document content through the realtime authority boundary so CLI/editor
/// adapters do not duplicate document-projection policy.
pub fn document_content_hash(content: &str) -> String {
    agent_doc_hash::content_hash(content)
}

/// Extend an existing deferred write with a later canonical target without
/// touching disk. If no deferred write exists, this starts one.
pub fn retain_deferred_document_write_target(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
    reason: DocumentWriteDeferredReason,
) -> Result<String> {
    ensure_deferred_document_write_intent(file, expected_current, content, source, reason)
}

/// Replace a retained target with a canonical refinement derived from the
/// current complete document cut. This is for post-commit normalization such
/// as moving the exchange boundary: the committed/current cut already includes
/// every earlier response, so component-merging the older whole-document
/// target back into it can duplicate prompt cells or split comment delimiters.
pub fn refine_deferred_document_write_target(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
    reason: DocumentWriteDeferredReason,
) -> Result<String> {
    ensure_deferred_document_write_intent_with_mode(
        file,
        expected_current,
        content,
        source,
        reason,
        true,
    )
}

fn pending_document_write_for_target(
    file: &Path,
    target_hash: &str,
) -> Option<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    pending_document_write(file)
        .as_ref()
        .filter(|pending| pending.target_hash.eq_ignore_ascii_case(target_hash))
        .cloned()
}

fn pending_external_disk_candidate_for_target(
    file: &Path,
    target_hash: &str,
) -> Option<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    pending_external_disk_candidate(file)
        .as_ref()
        .filter(|pending| pending.target_hash.eq_ignore_ascii_case(target_hash))
        .cloned()
}

fn durable_visible_write_content_proves_target(file: &Path, content: &str) -> bool {
    let candidate_hash = agent_doc_hash::content_hash(
        &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content),
    );
    agent_doc_cycle_state_io::load_document_projection(file)
        .ok()
        .flatten()
        .and_then(|document| {
            document
                .applied_visible_write_candidate(&candidate_hash)
                .cloned()
        })
        .and_then(|candidate| candidate.commit_candidate_content)
        .is_some_and(|candidate_content| candidate_content == content)
}

fn delivery_convergence_is_editor_visible(
    live_editors: usize,
    durable_visible_write_proven: bool,
) -> bool {
    live_editors > 0 || durable_visible_write_proven
}

fn ensure_deferred_document_write_intent(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
    reason: DocumentWriteDeferredReason,
) -> Result<String> {
    ensure_deferred_document_write_intent_with_mode(
        file,
        expected_current,
        content,
        source,
        reason,
        false,
    )
}

fn ensure_deferred_document_write_intent_with_mode(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
    reason: DocumentWriteDeferredReason,
    refine_current_cut: bool,
) -> Result<String> {
    validate_canonical_document_target(file, content, source)?;
    // The diagnostic tag stays a `&str` (it is only ever logged); the *intent
    // discriminant* is typed once here, at the retention boundary, so behavior
    // downstream compares variants instead of string prefixes
    // (`#adwritesourceenum`). An unrecognized tag becomes `Unknown` and
    // round-trips through `state.db` verbatim.
    let typed_source = agent_doc_state_backbone::DocumentWriteSource::from(source);
    let mut expected_content = expected_current.to_string();
    let mut target_content = content.to_string();
    let requested_target_hash = agent_doc_hash::content_hash(content);
    let external_disk_candidate =
        reason == DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor;
    let mut superseded_editor_reconnect = None;
    let existing_pending = if external_disk_candidate {
        pending_external_disk_candidate(file)
    } else {
        pending_document_write(file)
    };
    if let Some(pending) = existing_pending {
        if pending
            .target_hash
            .eq_ignore_ascii_case(&requested_target_hash)
            && (!external_disk_candidate
                || pending
                    .expected_hash
                    .eq_ignore_ascii_case(&agent_doc_hash::content_hash(expected_current)))
        {
            return Ok(pending.intent_id);
        }
        if pending.source == agent_doc_state_backbone::DocumentWriteSource::EditorReconnect
            && pending.reason
                == DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
        {
            superseded_editor_reconnect = Some(pending.clone());
        }

        // An external disk candidate is a replaceable user-decision value, not
        // a CRDT branch. A later filesystem event replaces the candidate while
        // preserving the exact current editor cut; never component-merge two
        // successive disk versions into the live buffer.
        if refine_current_cut && !external_disk_candidate {
            // The caller proves `content` was derived from the complete
            // current/committed cut. Preserve the original reconnect base but
            // replace the stale target exactly; merging the old target again
            // would replay an already-materialized response branch.
            expected_content = pending
                .expected_content
                .clone()
                .filter(|base| {
                    agent_doc_hash::content_hash(base).eq_ignore_ascii_case(&pending.expected_hash)
                })
                .unwrap_or_else(|| expected_current.to_string());
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_deferred_write_refined_from_current_cut file={} prior_intent_id={} prior_target_hash={} requested_hash={requested_target_hash}",
                    file.display(),
                    pending.intent_id,
                    pending.target_hash,
                ),
            );
        } else if external_disk_candidate {
            // Boundary/marker cleanup after one operator-authorized force-disk
            // write refines that same candidate. Keep the original editor cut
            // as its comparison base; the bytes currently on disk are the
            // prior force-disk target, not a newer editor decision.
            if typed_source.is_force_disk()
                && pending.source.is_force_disk()
                && let Some(retained_base) = pending.expected_content.clone().filter(|base| {
                    agent_doc_hash::content_hash(base).eq_ignore_ascii_case(&pending.expected_hash)
                })
            {
                expected_content = retained_base;
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_external_disk_candidate_replaced file={} prior_intent_id={} prior_target_hash={} requested_hash={requested_target_hash}",
                    file.display(),
                    pending.intent_id,
                    pending.target_hash,
                ),
            );
        } else {
            let expected_hash = agent_doc_hash::content_hash(expected_current);
            if requested_target_hash.eq_ignore_ascii_case(&expected_hash) {
                // The requested target is already the live CP authority. A prior
                // deferred target is therefore obsolete, not a concurrent branch
                // to merge back into the document. Rebase the reconnect lineage on
                // that exact prior target so the editor buffer which failed to ACK
                // it receives the newer canonical target directly, while later
                // operator edits still merge over the superseded cut.
                expected_content = pending.target_content.clone();
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_deferred_write_superseded_by_current_authority file={} prior_intent_id={} prior_target_hash={} requested_hash={requested_target_hash}",
                        file.display(),
                        pending.intent_id,
                        pending.target_hash,
                    ),
                );
            } else if pending.source
                == agent_doc_state_backbone::DocumentWriteSource::EditorReconnect
                && pending.reason
                    == DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
            {
                let merge_base = pending
                    .expected_content
                    .clone()
                    .filter(|base| {
                        agent_doc_hash::content_hash(base)
                            .eq_ignore_ascii_case(&pending.expected_hash)
                    })
                    .unwrap_or_else(|| expected_current.to_string());
                // An editor-reconnect target is a progressive visible snapshot,
                // not an independent component branch. Preserve only any agent
                // response it carried and rebase that semantic response over the
                // newest complete editor cut. Raw component CRDT composition can
                // splice the old partial queue line into the new full line.
                target_content = rebase_agent_candidate_over_editor_cut(
                    &merge_base,
                    &pending.target_content,
                    content,
                )
                .with_context(|| {
                    format!(
                        "failed to supersede progressive editor reconnect {} for {}",
                        pending.intent_id,
                        file.display()
                    )
                })?;
                target_content =
                    canonicalize_and_validate_agent_rebase(&target_content, content, file, source)?;
                expected_content = merge_base;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_deferred_write_superseded_progressive_editor_cut file={} prior_intent_id={} prior_target_hash={} requested_hash={requested_target_hash} target_hash={}",
                        file.display(),
                        pending.intent_id,
                        pending.target_hash,
                        agent_doc_hash::content_hash(&target_content),
                    ),
                );
            } else if !pending.target_hash.eq_ignore_ascii_case(&expected_hash) {
                let legacy_disk_base = std::fs::read_to_string(file).ok().filter(|disk| {
                    agent_doc_hash::content_hash(disk).eq_ignore_ascii_case(&pending.expected_hash)
                });
                let merge_base = pending
                    .expected_content
                    .clone()
                    .filter(|base| {
                        agent_doc_hash::content_hash(base)
                            .eq_ignore_ascii_case(&pending.expected_hash)
                    })
                    .or_else(|| {
                        (expected_hash.eq_ignore_ascii_case(&pending.expected_hash))
                            .then(|| expected_current.to_string())
                    })
                    .or(legacy_disk_base);
                let Some(merge_base) = merge_base else {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "{source}_deferred_write_preserved_prior file={} prior_intent_id={} reason=missing_content_bearing_merge_base requested_hash={requested_target_hash}",
                            file.display(),
                            pending.intent_id,
                        ),
                    );
                    return Ok(pending.intent_id);
                };
                let base_state =
                    agent_doc_merge::crdt::CrdtDoc::from_text(&merge_base).encode_state();
                target_content = agent_doc_merge::crdt::merge_by_component(
                    Some(&base_state),
                    &pending.target_content,
                    content,
                )
                .with_context(|| {
                    format!(
                        "failed to compose deferred document writes for {}",
                        file.display()
                    )
                })?;
                target_content =
                    canonicalize_and_validate_agent_rebase(&target_content, content, file, source)?;
                // Preserve the original editor cut as the merge base across a
                // chain of canonical target refinements (commit boundary moves,
                // marker cleanup, compaction). Re-basing on the prior target would
                // make a reconnecting pre-force editor look like it deleted the
                // response introduced by that target.
                expected_content = merge_base;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_deferred_write_composed file={} prior_intent_id={} requested_hash={requested_target_hash} composed_hash={}",
                        file.display(),
                        pending.intent_id,
                        agent_doc_hash::content_hash(&target_content),
                    ),
                );
            } else if let Some(retained_base) = pending.expected_content.clone().filter(|base| {
                agent_doc_hash::content_hash(base).eq_ignore_ascii_case(&pending.expected_hash)
            }) {
                expected_content = retained_base;
            }
        }
    }
    validate_canonical_document_target(file, &target_content, source)?;
    let target_hash = agent_doc_hash::content_hash(&target_content);
    let pending_for_target = if external_disk_candidate {
        pending_external_disk_candidate_for_target(file, &target_hash)
    } else {
        pending_document_write_for_target(file, &target_hash)
    };
    if let Some(pending) = pending_for_target {
        if let Some(superseded) = superseded_editor_reconnect {
            let superseded_target_hash = superseded.target_hash.clone();
            append_document_write_converged_event(
                file,
                superseded,
                &superseded_target_hash,
                &format!("{source}_superseded_editor_reconnect"),
            )?;
        }
        return Ok(pending.intent_id);
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let sequence = DOCUMENT_WRITE_INTENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let intent_id = format!("{now_nanos}-{sequence}-{target_hash}");
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("document-write-deferred-{document_hash}-{intent_id}"),
        agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
            document_hash,
            intent_id: intent_id.clone(),
            expected_hash: agent_doc_hash::content_hash(&expected_content),
            expected_content: Some(expected_content),
            target_hash: target_hash.clone(),
            target_content,
            source: typed_source,
            reason,
        },
    );
    publish_reactive_state_event(&project_root, &event).with_context(|| {
        format!(
            "failed to retain deferred document write in Lazily state for {}",
            file.display()
        )
    })?;
    // Append the successor before settling its progressive reconnect
    // predecessor. A crash can therefore leave both intents for historical
    // filtering, but can never lose the only durable target.
    if let Some(superseded) = superseded_editor_reconnect {
        let superseded_target_hash = superseded.target_hash.clone();
        append_document_write_converged_event(
            file,
            superseded,
            &superseded_target_hash,
            &format!("{source}_superseded_editor_reconnect"),
        )?;
    }
    Ok(intent_id)
}

/// Reconcile a reappearing editor buffer with the newest durable deferred
/// document target. A clean/stale buffer receives the target directly; later
/// unsaved operator edits are component-merged over the retained base. This is
/// the reconnect half of zero-replica and explicit `--force-disk` recovery.
pub fn deferred_document_write_reconnect_content(
    file: &Path,
    editor_content: &str,
) -> Result<Option<String>> {
    let editor_hash = agent_doc_hash::content_hash(editor_content);
    if let Some(pending) = pending_external_disk_candidate(file) {
        match agent_doc_document_realtime::external_disk_decision(
            &pending.expected_hash,
            &pending.target_hash,
            &editor_hash,
        ) {
            agent_doc_document_realtime::ExternalDiskDecision::AcceptedInEditor => {
                validate_canonical_document_target(
                    file,
                    &pending.target_content,
                    "external_disk_editor_reconnect",
                )?;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "external_disk_editor_decision file={} intent_id={} decision=accepted target_hash={}",
                        file.display(),
                        pending.intent_id,
                        pending.target_hash,
                    ),
                );
                return Ok(Some(pending.target_content));
            }
            agent_doc_document_realtime::ExternalDiskDecision::PendingUserDecision => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "external_disk_editor_decision file={} intent_id={} decision=pending_user_decision expected_hash={} target_hash={} mutation=none",
                        file.display(),
                        pending.intent_id,
                        pending.expected_hash,
                        pending.target_hash,
                    ),
                );
                return Ok(None);
            }
            agent_doc_document_realtime::ExternalDiskDecision::EditorSupersedes => {
                clear_external_disk_candidate_intent(
                    file,
                    &pending.target_hash,
                    "editor_reconnect_superseded_external_disk",
                )?;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "external_disk_editor_decision file={} intent_id={} decision=editor_supersedes editor_hash={} prior_target_hash={} pending_cleared=true",
                        file.display(),
                        pending.intent_id,
                        editor_hash,
                        pending.target_hash,
                    ),
                );
            }
        }
    }
    let pending_journal_all = pending_document_write_journal(file);
    // Builds before progressive reconnect supersession retained every editor
    // keystroke cut as an independent durable intent. Each later intent was
    // created against (and therefore incorporated) the current pending target;
    // replaying the obsolete cuts resurrects truncated queue lines. Preserve
    // independently sourced backlog/response intents, but collapse this one
    // explicitly typed snapshot lineage to its newest active member.
    let pending_journal = pending_journal_all
        .iter()
        .enumerate()
        .filter(|(index, intent)| {
            !(intent.source == agent_doc_state_backbone::DocumentWriteSource::EditorReconnect
                && intent.reason
                    == DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
                && *index + 1 < pending_journal_all.len())
        })
        .map(|(_, intent)| intent.clone())
        .collect::<Vec<_>>();
    if pending_journal.len() != pending_journal_all.len() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_reconnect_superseded_progressive_cuts_filtered file={} filtered={} retained={}",
                file.display(),
                pending_journal_all.len() - pending_journal.len(),
                pending_journal.len(),
            ),
        );
    }
    let Some(pending) = pending_journal.last().cloned() else {
        return Ok(None);
    };
    if editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
        let retained = heal_welded_scaffolding(&pending.target_content)
            .unwrap_or_else(|| pending.target_content.clone());
        validate_canonical_document_target(file, &retained, "editor_reconnect_retained_target")?;
        return Ok(Some(retained));
    }
    let disk_content = std::fs::read_to_string(file).ok();
    // `#boundarysplice`: heal a welded boundary marker in the editor canonical at
    // intake. The corruption is in `editor_content` itself, so every downstream
    // merge and `validate_canonical_document_target` call inherits it — which is
    // the permanent reconnect wedge, since the validator refuses before any repair
    // seam runs and nothing else rewrites the text. Repairing here is lossless and
    // touches only binary-owned scaffolding.
    let mut merged =
        heal_welded_scaffolding(editor_content).unwrap_or_else(|| editor_content.to_string());
    if let Some(pruned) = agent_doc_element_done::prune_proven_redundant_terminal_debris(&merged) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_reconnect_terminal_debris_projected file={} removed_line_count={} projected_hash={}",
                file.display(),
                pruned.removed_line_count,
                agent_doc_hash::content_hash(&pruned.content),
            ),
        );
        merged = pruned.content;
    }
    for (intent_index, intent) in pending_journal.iter().enumerate() {
        let merge_base = intent
            .expected_content
            .clone()
            .filter(|content| {
                agent_doc_hash::content_hash(content).eq_ignore_ascii_case(&intent.expected_hash)
            })
            .or_else(|| {
                disk_content.as_ref().and_then(|disk| {
                    agent_doc_hash::content_hash(disk)
                        .eq_ignore_ascii_case(&intent.expected_hash)
                        .then(|| disk.clone())
                })
            })
            .with_context(|| {
                format!(
                    "deferred write {} for {} has no content-bearing merge base",
                    intent.intent_id,
                    file.display()
                )
            })?;
        if intent_index == 0 {
            merged = editor_operator_cut_for_agent_rebase(
                file,
                &merge_base,
                &merged,
                "editor_reconnect",
            );
        }
        let merged_hash = agent_doc_hash::content_hash(&merged);
        if merged_hash.eq_ignore_ascii_case(&intent.target_hash) {
            continue;
        }
        if merged_hash.eq_ignore_ascii_case(&intent.expected_hash) {
            merged = intent.target_content.clone();
            continue;
        }
        if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&merged)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &intent.target_content,
            )
        {
            // The durable intent differs only in binary-owned cycle scaffolding.
            // Replaying its exact target replaces the old marker generation;
            // component-additive rebasing would otherwise weld both boundaries.
            merged = intent.target_content.clone();
            continue;
        }

        // Re-evaluate each durable semantic intent over the latest operator
        // cut. In particular, a retained response may append only its response
        // cell; replaying its stale whole-document target can resurrect queue
        // or backlog lines the operator deleted while the ACK was pending.
        merged =
            rebase_agent_candidate_over_editor_cut(&merge_base, &intent.target_content, &merged)
                .with_context(|| {
                    format!(
                        "failed to replay deferred agent change {} over editor content for {}",
                        intent.intent_id,
                        file.display()
                    )
                })?;
        merged = agent_doc_merge::response_cell::deduplicate_response_cells(&merged)
            .ok()
            .flatten()
            .unwrap_or(merged);
        merged = canonicalize_and_validate_agent_rebase(
            &merged,
            &intent.target_content,
            file,
            "editor_reconnect",
        )?;
    }
    if agent_doc_hash::content_hash(&merged).eq_ignore_ascii_case(&pending.target_hash) {
        // `#boundarysplice`: the retained target is durable `state.db` content, so
        // a welded boundary captured into it re-fails on every reconnect forever.
        let retained =
            heal_welded_scaffolding(&pending.target_content).unwrap_or(pending.target_content);
        validate_canonical_document_target(file, &retained, "editor_reconnect_retained_target")?;
        return Ok(Some(retained));
    }
    let merged = heal_welded_scaffolding(&merged).unwrap_or(merged);
    validate_canonical_document_target(file, &merged, "editor_reconnect")?;
    ensure_deferred_document_write_intent(
        file,
        &pending.target_content,
        &merged,
        "editor_reconnect",
        DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
    )?;
    Ok(Some(merged))
}

/// Retain an out-of-band disk version while one or more live editor buffers own
/// the document. Repeated disk changes replace the candidate exactly; they are
/// not merged into each other or into the editor buffer.
pub fn retain_external_disk_candidate(
    file: &Path,
    editor_content: &str,
    disk_content: &str,
    source: &str,
) -> Result<Option<String>> {
    if editor_content == disk_content {
        return Ok(None);
    }
    ensure_deferred_document_write_intent(
        file,
        editor_content,
        disk_content,
        source,
        DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor,
    )
    .map(Some)
}

/// Retain an external disk version when editors are known to be open but the
/// CRDT relay cannot yet prove one exact editor cut (for example, two replicas
/// are still converging after reconnect). An empty expected hash is a typed
/// "unknown editor cut" marker: reconnect remains mutation-free until an
/// explicit editor edit, save, accepted target propagation, or final close.
pub fn retain_external_disk_candidate_without_editor_cut(
    file: &Path,
    disk_content: &str,
    source: &str,
) -> Result<String> {
    let target_hash = agent_doc_hash::content_hash(disk_content);
    if let Some(pending) = pending_external_disk_candidate(file) {
        if pending.target_hash.eq_ignore_ascii_case(&target_hash)
            && pending.expected_hash.is_empty()
        {
            return Ok(pending.intent_id);
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_external_disk_candidate_replaced_without_editor_cut file={} prior_intent_id={} prior_target_hash={} requested_hash={target_hash}",
                file.display(),
                pending.intent_id,
                pending.target_hash,
            ),
        );
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let sequence = DOCUMENT_WRITE_INTENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let intent_id = format!("{now_nanos}-{sequence}-{target_hash}");
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("document-write-deferred-{document_hash}-{intent_id}"),
        agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
            document_hash,
            intent_id: intent_id.clone(),
            expected_hash: String::new(),
            expected_content: None,
            target_hash,
            target_content: disk_content.to_string(),
            source: source.into(),
            reason: DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor,
        },
    );
    publish_reactive_state_event(&project_root, &event).with_context(|| {
        format!(
            "failed to retain external disk candidate without editor cut for {}",
            file.display()
        )
    })?;
    Ok(intent_id)
}

fn retain_force_disk_reconnect_intent(file: &Path, content: &str) -> Result<()> {
    if !agent_doc_document_realtime::write_authority::is_visible_document(file) {
        return Ok(());
    }
    // `--force-disk` is an explicit authority replacement, so an older
    // delivery/reconnect lineage must not be replayed over the operator's
    // chosen disk target. Retire it through convergence events before
    // retaining the new disk-vs-editor decision; never mutate state.db.
    clear_all_deferred_document_write_intents(file, "force_disk_override")?;
    let pre_force_disk = std::fs::read_to_string(file).unwrap_or_default();
    if pre_force_disk == content {
        return Ok(());
    }
    ensure_deferred_document_write_intent(
        file,
        &pre_force_disk,
        content,
        "force_disk",
        DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor,
    )?;
    Ok(())
}

/// Clear a force-disk/recovery candidate after a proven local editor edit
/// advances beyond both the pre-write cut and the external target. Both editor
/// plugins report full visible content through the same v3 FFI hook, so this
/// transition is shared and parity-safe.
pub fn clear_pending_external_disk_decision_on_editor_edit(
    file: &Path,
    editor_content: &str,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_external_disk_candidate(file) else {
        return Ok(false);
    };
    let editor_hash = agent_doc_hash::content_hash(editor_content);
    if !pending.expected_hash.is_empty()
        && agent_doc_document_realtime::external_disk_decision(
            &pending.expected_hash,
            &pending.target_hash,
            &editor_hash,
        ) != agent_doc_document_realtime::ExternalDiskDecision::EditorSupersedes
    {
        return Ok(false);
    }
    if pending.expected_hash.is_empty() && editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
        return Ok(false);
    }
    clear_external_disk_candidate_intent(file, &pending.target_hash, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "external_disk_editor_decision file={} intent_id={} decision=editor_supersedes source={} editor_hash={} prior_target_hash={} pending_cleared=true",
            file.display(),
            pending.intent_id,
            source,
            editor_hash,
            pending.target_hash,
        ),
    );
    Ok(true)
}

/// Settle any pending external disk candidate after an editor save has proven
/// that its exact buffer bytes reached disk. This is intentionally independent
/// of which candidate was pending: the saved editor version is authoritative.
pub fn clear_pending_external_disk_decision_on_editor_save(
    file: &Path,
    saved_editor_content: &str,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_external_disk_candidate(file) else {
        return Ok(false);
    };
    let disk = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to verify saved editor content for {}",
            file.display()
        )
    })?;
    if disk != saved_editor_content {
        return Ok(false);
    }
    clear_external_disk_candidate_intent(file, &pending.target_hash, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "external_disk_editor_decision file={} intent_id={} decision=editor_save_overrides_candidate saved_hash={} prior_target_hash={} pending_cleared=true",
            file.display(),
            pending.intent_id,
            agent_doc_hash::content_hash(saved_editor_content),
            pending.target_hash,
        ),
    );
    Ok(true)
}

/// Settle a candidate after an editor plugin has reset/seeded its CRDT replica
/// from the exact accepted disk target.
pub fn clear_pending_external_disk_decision_after_editor_propagation(
    file: &Path,
    editor_content: &str,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_external_disk_candidate(file) else {
        return Ok(false);
    };
    let editor_hash = agent_doc_hash::content_hash(editor_content);
    if !editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
        return Ok(false);
    }
    clear_external_disk_candidate_intent(file, &pending.target_hash, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "external_disk_editor_decision file={} intent_id={} decision=accepted_and_propagated editor_hash={} pending_cleared=true",
            file.display(),
            pending.intent_id,
            editor_hash,
        ),
    );
    Ok(true)
}

/// Clear the pending external-disk decision when the last editor closes.  The
/// explicit close path projects the closing editor/CRDT cut before recording
/// disk authority, so an older external candidate can no longer displace it.
pub fn clear_pending_external_disk_decision_on_last_editor_close(
    file: &Path,
    source: &str,
) -> Result<bool> {
    let Some(pending) = pending_external_disk_candidate(file) else {
        return Ok(false);
    };
    clear_external_disk_candidate_intent(file, &pending.target_hash, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "external_disk_editor_decision file={} intent_id={} decision=last_editor_closed prior_target_hash={} pending_cleared=true authority=closing_editor_projection",
            file.display(),
            pending.intent_id,
            pending.target_hash,
        ),
    );
    Ok(true)
}

/// Materialize the final CRDT/editor authority after an explicit last-editor
/// close notification.
///
/// A generic zero-member relay quorum is not visible-write proof: the IDE may
/// merely be stale or disconnected while still holding an unsaved buffer.  An
/// explicit close notification plus a reliable-sync `false` liveness fact is
/// different.  It is the ownership handoff from the closing editor to disk, so
/// the binary must rebase every retained agent intent over the closing CRDT cut
/// and atomically project that canonical result.  This is deliberately the only
/// zero-member path that can write disk without operator `--force-disk`.
pub fn materialize_last_editor_close_through_authority(file: &Path, source: &str) -> Result<bool> {
    if agent_doc_reliable_sync_io::plane_editor_live_for_path(&file.to_string_lossy())
        != Some(false)
    {
        return Ok(false);
    }
    let base_dir = agent_doc_project_root_io::project_root_containing(file)
        .unwrap_or_else(|| file.parent().unwrap_or(Path::new(".")).to_path_buf());
    let file_key = file.to_string_lossy().to_string();
    let owned_file = file.to_path_buf();
    let owned_source = source.to_string();
    agent_doc_queue_io::write_queue::run_serialized_with(
        &SESSION_ACTOR_WRITE_QUEUE,
        &base_dir,
        &file_key,
        agent_doc_document_realtime::session_ops::SessionOpKind::Lifecycle,
        move || materialize_last_editor_close_in_owner(&owned_file, &owned_source),
    )?
}

fn materialize_last_editor_close_in_owner(file: &Path, source: &str) -> Result<bool> {
    let path = file.to_string_lossy();
    if agent_doc_reliable_sync_io::plane_editor_live_for_path(&path) != Some(false) {
        return Ok(false);
    }

    let disk_before = std::fs::read_to_string(file).with_context(|| {
        format!(
            "{source}: failed to read {} on last editor close",
            file.display()
        )
    })?;
    // The reliable-sync close fact intentionally flips ordinary reads to disk
    // authority.  This handoff still needs the just-closed Lazily cut, so read
    // the retained CRDT model explicitly (recovering its durable projection if
    // this process does not host the controller hub).
    let observed =
        agent_doc_crdt_relay_io::current_text_for_file_with_authority_recovering_projection(
            file,
            agent_doc_document_realtime::crdt_authority::CrdtAuthority::MultiReplica,
        )?;
    let (closing_cut, relay_status, relay_members) = match observed {
        agent_doc_crdt_relay_io::CurrentText::Current {
            text, live_editors, ..
        } => (text, "current", live_editors),
        agent_doc_crdt_relay_io::CurrentText::Detached => (disk_before.clone(), "detached", 0),
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            (disk_before.clone(), "missing_replica", 0)
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            (disk_before.clone(), "sync_pending", 0)
        }
    };

    // The editor cut wins an unresolved external disk candidate on explicit
    // close.  Retained agent intents are then replayed over that exact cut.
    clear_pending_external_disk_decision_on_last_editor_close(file, source)?;
    let target =
        deferred_document_write_reconnect_content(file, &closing_cut)?.unwrap_or(closing_cut);
    validate_canonical_document_target(file, &target, source)?;

    // Serialize the authority handoff with every other writer and revalidate
    // both liveness and disk bytes at the mutation fence.
    if agent_doc_reliable_sync_io::plane_editor_live_for_path(&path) != Some(false) {
        anyhow::bail!(
            "{source}: editor reattached before last-close projection for {}",
            file.display()
        );
    }
    let disk_at_mutation = std::fs::read_to_string(file).with_context(|| {
        format!(
            "{source}: failed to re-read {} at last-close mutation fence",
            file.display()
        )
    })?;
    if disk_at_mutation != disk_before {
        anyhow::bail!(
            "{source}: disk changed during last-close authority handoff for {}; retained intents remain available for retry",
            file.display()
        );
    }
    let wrote = target != disk_before;
    if wrote {
        atomic_write_authority_raw(file, &target)?;
    }
    let final_disk = std::fs::read_to_string(file).with_context(|| {
        format!(
            "{source}: failed to verify {} after last-close projection",
            file.display()
        )
    })?;
    if final_disk != target {
        anyhow::bail!(
            "{source}: last-close disk projection verification failed for {}",
            file.display()
        );
    }

    let target_hash = agent_doc_hash::content_hash(&target);
    if let Some(pending) = pending_document_write(file) {
        if !pending.target_hash.eq_ignore_ascii_case(&target_hash) {
            anyhow::bail!(
                "{source}: retained intent advanced during last-close projection for {}; projected_hash={} pending_hash={}",
                file.display(),
                target_hash,
                pending.target_hash,
            );
        }
        clear_deferred_document_write_intent(file, &pending.target_hash, source)?;
    }
    record_disk_replica_authority(file, source, &target);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "last_editor_close_retained_projection_materialized file={} content_hash={} wrote={} relay_status={} relay_members={} authority=disk",
            file.display(),
            target_hash,
            wrote,
            relay_status,
            relay_members,
        ),
    );
    Ok(true)
}

fn clear_deferred_document_write_intent(
    file: &Path,
    target_hash: &str,
    source: &str,
) -> Result<()> {
    let Some(pending) = pending_document_write_for_target(file, target_hash) else {
        return Ok(());
    };
    append_document_write_converged_event(file, pending, target_hash, source)
}

fn clear_external_disk_candidate_intent(
    file: &Path,
    target_hash: &str,
    source: &str,
) -> Result<()> {
    let Some(pending) = pending_external_disk_candidate_for_target(file, target_hash) else {
        return Ok(());
    };
    append_document_write_converged_event(file, pending, target_hash, source)
}

fn append_document_write_converged_event(
    file: &Path,
    pending: agent_doc_state_backbone::DocumentWriteIntentProjection,
    target_hash: &str,
    source: &str,
) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "document-write-converged-{document_hash}-{}",
            pending.intent_id
        ),
        agent_doc_state_backbone::StateFact::DocumentWriteConverged {
            document_hash,
            intent_id: pending.intent_id,
            target_hash: target_hash.to_string(),
            source: source.to_string(),
            intent_source: pending.source,
        },
    );
    publish_reactive_state_event(&project_root, &event).with_context(|| {
        format!(
            "failed to settle deferred document write in Lazily state for {}",
            file.display()
        )
    })?;
    Ok(())
}

#[cfg(not(test))]
fn publish_reactive_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    agent_doc_controller_io::project_controller::publish_state_event(project_root, event)
}

#[cfg(test)]
fn publish_reactive_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    // Unit fixtures intentionally exercise the pure durable projector without
    // launching a second agent-doc process. Production has no direct route.
    agent_doc_controller_io::project_controller::publish_state_event_existing(project_root, event)
        .or_else(|_| {
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                project_root,
                event,
            )
        })
}

/// Settle every older deferred target after an exact repair projection. Deferred
/// intents form a newest-first event stack; settling only the current target can
/// uncover an older intermediate repair target and replay it after the final
/// boundary/marker normalization. Exact canonical+disk proof at the caller makes
/// every older intent obsolete.
pub fn clear_all_deferred_document_write_intents(file: &Path, source: &str) -> Result<()> {
    for _ in 0..256 {
        let Some(pending) = pending_document_write(file) else {
            return Ok(());
        };
        let intent_id = pending.intent_id.clone();
        clear_deferred_document_write_intent(file, &pending.target_hash, source)?;
        anyhow::ensure!(
            pending_document_write(file)
                .as_ref()
                .is_none_or(|next| next.intent_id != intent_id),
            "{source}: settling deferred document write {} for {} made no progress",
            intent_id,
            file.display(),
        );
    }
    anyhow::bail!(
        "{source}: refusing to settle more than 256 deferred document writes for {}",
        file.display()
    )
}

/// Record that disk is the current document replica because no live editor owns
/// the document.
pub fn record_disk_replica_authority(file: &std::path::Path, source: &str, disk: &str) {
    record_document_authority(
        file,
        source,
        agent_doc_state_backbone::DocumentAuthority::DiskReplica,
        "editor_detached",
        Some(agent_doc_hash::content_hash(disk)),
        None,
    );
}

/// Resolve the live editor relay state and persist the authority decision in
/// the state backbone. Detached callers should record disk authority after they
/// choose to use disk.
pub fn observe_live_editor_authority(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let current = query_live_editor_authority(file, source)?;
    record_current_text_authority(file, source, &current);
    Ok(current)
}

fn query_live_editor_authority(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if controller_document_mutation_in_progress() {
        return agent_doc_crdt_relay_io::current_text_for_file(file);
    }
    let embedded = match query_embedded_relay(file, source)? {
        Some(current) if embedded_relay_observation_is_current(&current) => return Ok(current),
        embedded => embedded,
    };
    if !agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(file) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "document_model_controller_lookup_skipped file={} source={} reason=lazily_editor_absent",
                file.display(),
                source,
            ),
        );
        return Ok(embedded.unwrap_or(agent_doc_crdt_relay_io::CurrentText::Detached));
    }
    #[cfg(test)]
    {
        Ok(embedded.unwrap_or_else(|| {
            agent_doc_crdt_relay_io::current_text_for_file(file)
                .unwrap_or(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
        }))
    }
    #[cfg(not(test))]
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
        file, source,
    ) {
        Ok(Some(current)) => {
            if let Some(embedded) = embedded.as_ref() {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_embedded_relay_refreshed_from_controller file={} source={} embedded_status={} controller_status={} reason=transient_embedded_observation",
                        file.display(),
                        source,
                        current_text_status(embedded),
                        current_text_status(&current),
                    ),
                );
            }
            Ok(current)
        }
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_controller_lookup_unavailable file={} source={} fallback={}",
                    file.display(),
                    source,
                    if embedded.is_some() {
                        "embedded_relay"
                    } else {
                        "missing_replica"
                    },
                ),
            );
            Ok(embedded
                .unwrap_or(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica))
        }
        Err(e) => {
            if let Some(embedded) = embedded {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_controller_lookup_error file={} source={} error={} fallback=embedded_relay embedded_status={}",
                        file.display(),
                        source,
                        e,
                        current_text_status(&embedded),
                    ),
                );
                return Ok(embedded);
            }
            // Record controller degradation so hot polling paths (idle-queue
            // watch) can back off and stop flooding a wedged controller.
            record_controller_degraded();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_controller_lookup_error file={} source={} error={} fallback=none",
                    file.display(),
                    source,
                    e,
                ),
            );
            Err(e)
        }
    }
}

fn embedded_relay_observation_is_current(current: &agent_doc_crdt_relay_io::CurrentText) -> bool {
    matches!(
        current,
        agent_doc_crdt_relay_io::CurrentText::Current { .. }
    )
}

fn query_embedded_relay(
    file: &std::path::Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    if !agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return Ok(None);
    }
    let current = agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "document_model_embedded_relay_observed file={} source={} status={} reason=in_process_authority",
            file.display(),
            source,
            current_text_status(&current)
        ),
    );
    Ok(Some(current))
}

fn current_text_status(current: &agent_doc_crdt_relay_io::CurrentText) -> &'static str {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Detached => "detached",
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            "editor_attached_model_missing"
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => "editor_sync_pending",
        agent_doc_crdt_relay_io::CurrentText::Current { .. } => "current",
    }
}

/// Name the actual unblocker for an editor-authority refusal (`#editorendpointzero`).
///
/// The bare refusal is a dead end: it tells the operator the editor is open and
/// disk will not be used, and stops there. But the binary can enumerate live
/// editor registrations, and the recovery differs sharply by count:
///
/// - **zero endpoints** — the plugin has no registered endpoint for this document,
///   so `admin reload-lib` is a no-op (nothing to deliver to) and `admin recycle`
///   does not touch it either. Only the editor re-registering clears it. This is
///   the state a `make install` / `lib-install` cdylib swap leaves behind when the
///   plugin does not re-register after the hot-reload, which makes install-heavy
///   dogfooding sessions self-inflict it.
/// - **one or more endpoints** — a replica is registered but has no model behind
///   it, which is the shape `reload-lib` clears immediately.
///
/// Diagnostic only: this never changes which replica wins authority.
fn editor_authority_unavailable_unblocker(file: &Path) -> String {
    match agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file) {
        Ok(registrations) if registrations.is_empty() => {
            "; live_editor_endpoints=0 — the editor plugin holds no registered endpoint for this document, so `agent-doc admin reload-lib` cannot help (it has nothing to deliver to) and neither can `admin recycle`. Unblock by making the plugin re-register: reopen this file's editor tab, or restart the IDE. Commonly left behind by a `make install` / `lib-install` cdylib swap the plugin did not re-register after"
                .to_string()
        }
        Ok(registrations) => format!(
            "; live_editor_endpoints={} — a replica is registered but has no model behind it; `agent-doc admin reload-lib` clears this shape",
            registrations.len()
        ),
        // Never swallow: an unavailable count is itself worth reporting, since it
        // means the operator cannot tell the two recoveries apart.
        Err(err) => format!("; live_editor_endpoints=unknown ({err:#})"),
    }
}

fn resolve_closed_editor_disk_fallback_current_doc(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
    reason: &str,
    detail: Option<&str>,
) -> Result<Reconciliation> {
    if observe_editor_open(file) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_doc_resolve_deferred file={} source={} reason={} lazily_editor_open=true fallback=none",
                file.display(),
                source,
                reason,
            ),
        );
        anyhow::bail!(
            "editor_attached_model_missing: {reason}: editor authority unavailable for {}; Lazily still reports the editor open, so disk is not consulted as a fallback{}",
            file.display(),
            editor_authority_unavailable_unblocker(file),
        );
    }

    let disk = match disk {
        Some(disk) => disk.to_string(),
        None => std::fs::read_to_string(file).with_context(|| {
            format!(
                "{source}: editor authority unavailable for {}; failed to read idle disk fallback replica",
                file.display()
            )
        })?,
    };
    record_disk_replica_authority(file, source, &disk);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "realtime_doc_resolve_disk_fallback file={} source={} reason={} lazily_editor_open=false detail={}",
            file.display(),
            source,
            reason,
            detail.unwrap_or("none").replace('\n', " "),
        ),
    );
    Ok(resolve_disk_only_current_doc(
        file,
        &disk,
        "idle_editor_authority_fallback",
        source,
    ))
}

/// Resolve live editor authority, attempting bounded document-model
/// startup/reconciliation before returning a missing-model state to callers.
pub fn observe_live_editor_authority_after_model_ensure(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let current = query_live_editor_authority_after_model_ensure(file, source)?;
    record_current_text_authority(file, source, &current);
    Ok(current)
}

fn query_live_editor_authority_after_model_ensure(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let current = query_live_editor_authority(file, source)?;
    match current {
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            if source == "resolve_current_doc" {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_suppressed file={} source={} reason=read_only_current_doc_resolve status={}",
                        file.display(),
                        source,
                        current_text_status(&current),
                    ),
                );
                return Ok(current);
            }
            let ensured = ensure_document_model_through_authority(file, source)?;
            Ok(ensured)
        }
        agent_doc_crdt_relay_io::CurrentText::Detached
        | agent_doc_crdt_relay_io::CurrentText::Current { .. } => Ok(current),
    }
}

#[cfg(any(test, feature = "test-support"))]
fn ensure_document_model_through_authority(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return test_support_ensure_document_model(file, source);
    }
    ensure_document_model_through_controller_authority(file, source)
}

#[cfg(not(any(test, feature = "test-support")))]
fn ensure_document_model_through_authority(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    ensure_document_model_through_controller_authority(file, source)
}

fn ensure_document_model_through_controller_authority(
    file: &std::path::Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if controller_document_mutation_in_progress() {
        return agent_doc_crdt_relay_io::ensure_document_model(file, source);
    }
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
        file, source,
    )? {
        Some(current) => Ok(current),
        None => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_controller_ensure_unavailable file={} source={} fallback=missing_replica",
                    file.display(),
                    source
                ),
            );
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
        }
    }
}

fn record_current_text_authority(
    file: &std::path::Path,
    source: &str,
    current: &agent_doc_crdt_relay_io::CurrentText,
) {
    match &current {
        agent_doc_crdt_relay_io::CurrentText::Detached => {}
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            record_document_authority(
                file,
                source,
                agent_doc_state_backbone::DocumentAuthority::EditorAttachedMissingReplica,
                "missing_replica",
                None,
                None,
            );
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            record_document_authority(
                file,
                source,
                agent_doc_state_backbone::DocumentAuthority::EditorSyncPending,
                "sync_pending",
                None,
                None,
            );
        }
        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => {
            record_editor_relay_authority(file, source, text);
        }
    }
}

fn record_editor_relay_authority(file: &std::path::Path, source: &str, text: &str) {
    record_document_authority(
        file,
        source,
        agent_doc_state_backbone::DocumentAuthority::EditorRelay,
        "crdt_relay_current",
        Some(agent_doc_hash::content_hash(text)),
        None,
    );
}

const VISIBLE_WRITE_CURRENT_TRANSITION_TIMEOUT_MS: u64 = 5_000;

/// Max re-merge attempts when reconciling the visible-write guard with a
/// foreign disk write that landed after the merge was computed
/// (#ipc-drift-visbuf-reconcile). After this many drifting re-reads, fall back
/// to the fail-closed guard so the operator retries.
pub const VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS: usize = 3;

pub fn guard_visible_write_current_transition(file: &std::path::Path, source: &str) -> Result<()> {
    guard_visible_write_current_transition_with_budget(
        file,
        source,
        0,
        VISIBLE_WRITE_CURRENT_TRANSITION_TIMEOUT_MS,
    )
}

pub fn guard_visible_write_current_transition_with_budget(
    file: &std::path::Path,
    source: &str,
    _debounce_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    guard_visible_write_current_transition_with_policy(file, source, timeout_ms, true)
}

/// Require the post-write editor delivery projection to be visible before a
/// caller creates snapshots or commits.
///
/// Detached documents have no editor delivery quorum and are ready. An attached
/// document with a missing replica is *not* ready here: the mutation path may
/// retain such a write, but secondary compact/closeout effects must not run
/// behind it.
pub fn guard_visible_delivery_convergence(file: &Path, source: &str) -> Result<()> {
    const PROJECTION_EDGE_GRACE: std::time::Duration = std::time::Duration::from_millis(100);
    let deadline = std::time::Instant::now() + PROJECTION_EDGE_GRACE;
    loop {
        match query_live_editor_authority(file, source)? {
            agent_doc_crdt_relay_io::CurrentText::Detached
            | agent_doc_crdt_relay_io::CurrentText::Current {
                delivery_converged: true,
                ..
            } => return Ok(()),
            agent_doc_crdt_relay_io::CurrentText::Current { .. }
                if std::time::Instant::now() < deadline =>
            {
                // This is an observation edge, not a delivery request. It gives
                // an already-running editor projection one scheduler turn to
                // invalidate the derived frontier before the synchronous
                // secondary-effect boundary decides whether to retain.
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            agent_doc_crdt_relay_io::CurrentText::Current {
                delivery_version,
                live_editors,
                ..
            } => {
                return defer_visible_delivery_projection(
                    file,
                    source,
                    delivery_version,
                    live_editors,
                );
            }
            // `#percellconverge`: both branches used to assert retention with no
            // owner. `EditorAttachedMissingReplica` in particular fires off the
            // one-way editor-attachment latch (`#editormodelmissing`), which
            // reports an editor as attached forever once one ever has — so this
            // site could report a durable holder for a document nothing was
            // holding. Ask the same predicate `session-check` and the write path
            // ask, so an agent gets one answer whichever refusal it reaches first.
            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
                return Err(await_editor_replica_no_disk_write(format!(
                    "visible document write for {} is retained by the lazy delivery projection; the attached editor replica is not registered, so no snapshot or commit effect is eligible. {}",
                    file.display(),
                    retained_write_remedy_for(file),
                )));
            }
            agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
                return Err(await_editor_replica_no_disk_write(format!(
                    "visible document write for {} is retained by the lazy delivery projection while editor synchronization is pending; no snapshot or commit effect is eligible. {}",
                    file.display(),
                    retained_write_remedy_for(file),
                )));
            }
        }
    }
}

fn defer_visible_delivery_projection(
    file: &Path,
    source: &str,
    delivery_version: u64,
    live_editors: usize,
) -> Result<()> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_delivery_projection_deferred file={} source={} delivery_version={} live_editors={} recovery=lazy_projection_pending operator_action=none",
            file.display(),
            source,
            delivery_version,
            live_editors,
        ),
    );
    // `#retainedwriteremedy`: this refusal must name its recovery, exactly like
    // its two sibling branches above. Without the remedy the message reads as
    // "nothing happened" — but the write may already have applied and be waiting
    // only on the terminal commit, which `agent-doc commit <FILE>` completes.
    // An agent that reads "no ... write was attempted" as "the response was
    // lost" re-answers or resubmits, which is precisely what `#percellconverge`
    // forbids. Derive the wording from the one ownership predicate so an agent
    // gets the same answer whichever refusal it reaches first.
    Err(await_editor_replica_no_disk_write(format!(
        "visible document write for {} is retained by the lazy delivery projection because the editor state projection has not converged; no secondary snapshot/commit or forced disk write was attempted. {}",
        file.display(),
        retained_write_remedy_for(file),
    )))
}

fn guard_visible_write_current_transition_with_policy(
    file: &Path,
    source: &str,
    timeout_ms: u64,
    allow_missing_replica_defer: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut projection_observation = ProjectionObservationState::default();
    loop {
        // An evicted hub is already a first-class missing-model state. Let the
        // CP write layer attempt durable-projection recovery (or retain/defer
        // immediately when none exists) instead of spending the whole settle
        // budget trying to make a model current before that recovery seam runs.
        let missing_model = matches!(
            query_live_editor_authority(file, source),
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
        );
        let (ready, state, delivery_cursor) = if missing_model && allow_missing_replica_defer {
            (true, "missing_replica_defer", None)
        } else {
            match query_live_editor_authority_after_model_ensure(file, source) {
                Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => (true, "detached", None),
                Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                    delivery_converged: true,
                    ..
                }) => (true, "lazily_current", None),
                Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                    delivery_version,
                    live_editors,
                    ..
                }) => (
                    false,
                    "delivery_pending",
                    Some((delivery_version, live_editors)),
                ),
                Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => {
                    (false, "missing_replica", None)
                }
                Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => {
                    (false, "current_pending", None)
                }
                Err(_) => (false, "authority_unavailable", None),
            }
        };
        if ready {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_current_transition_ready file={} source={} state={}",
                    file.display(),
                    source,
                    state
                ),
            );
            return Ok(());
        }
        if start.elapsed() >= std::time::Duration::from_millis(timeout_ms) {
            if let Some((delivery_version, live_editors)) = delivery_cursor {
                return defer_visible_delivery_projection(
                    file,
                    source,
                    delivery_version,
                    live_editors,
                );
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_deferred_current_transition file={} source={} state={} timeout_ms={}",
                    file.display(),
                    source,
                    state,
                    timeout_ms
                ),
            );
            let recovery = if state == "delivery_pending" {
                "\nThe bounded ACK recovery barrier already replayed delivery and requested \
                 replacement of any build-skewed endpoint. No disk write was attempted. If the \
                 editor host cannot hot-reload its listener, reload the IDE host and retry the \
                 existing operation."
            } else {
                ""
            };
            anyhow::bail!(
                "visible document write for {} deferred: Lazily current transition remained {} for {}ms; retry after it settles{}",
                file.display(),
                state,
                timeout_ms,
                recovery
            );
        }
        if let Some((delivery_version, live_editors)) = delivery_cursor {
            let remaining_ms = u64::try_from(
                std::time::Duration::from_millis(timeout_ms)
                    .saturating_sub(start.elapsed())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX);
            if projection_observation.wait_for_delivery_change(DeliveryChangeWait {
                file,
                source,
                live_editors,
                delivery_version,
                // Give an ordinary in-flight editor pull/ACK the subscription
                // fast path first. The controller-owned recovery timers still
                // replay and force-refresh within the same bounded barrier.
                signal_immediately: false,
                max_wait_ms: Some(remaining_ms),
            })? == ProjectionObservationWait::ForegroundDeadline
            {
                return defer_visible_delivery_projection(
                    file,
                    source,
                    delivery_version,
                    live_editors,
                );
            }
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

pub fn guard_visible_write_expected_current(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    guard_visible_write_expected_current_or_target(file, source, expected_current, None)
}

pub fn guard_visible_write_expected_current_or_target(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<()> {
    match guard_visible_write_reconcile_with_target(file, source, expected_current, target_content)?
    {
        VisibleWriteReconcile::Clean => Ok(()),
        VisibleWriteReconcile::DiskDrifted { fresh_current } => {
            agent_doc_flow_io::log_flow_event(
                file,
                write_policy::visible_write_current_changed_event(source),
                agent_doc_ops_log_io::log_op,
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_deferred_current_changed file={} source={} expected_hash={} current_hash={}",
                    file.display(),
                    source,
                    agent_doc_hash::content_hash(expected_current),
                    agent_doc_hash::content_hash(&fresh_current)
                ),
            );
            anyhow::bail!(
                "visible document write for {} deferred: document changed after the response merge was computed; retry after typing stops",
                file.display()
            )
        }
    }
}

/// Like [`guard_visible_write_expected_current`] but, instead of failing closed
/// when the current document changed *after* the merge was computed, reports the
/// fresh authoritative content so a CRDT-merge caller can re-merge the captured
/// response and retry.
///
/// When an active editor owns the document, the CRDT relay canonical text is the
/// authoritative current document. Disk is only the fallback authority when the
/// relay reports the document is detached.
pub fn guard_visible_write_reconcile_with_target(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<VisibleWriteReconcile> {
    guard_visible_write_current_transition(file, source)?;
    match observe_live_editor_authority_after_model_ensure(file, source) {
        Ok(agent_doc_crdt_relay_io::CurrentText::Current {
            text: relay_text,
            live_editors,
            delivery_converged,
            ..
        }) => {
            let relay_hash = agent_doc_hash::content_hash(&relay_text);
            if live_editors == 0 {
                // #live-editor-reactive (S2b/S3): zero live replicas is a repairable
                // derived signal, not ground truth. If the editor still has this document
                // open, the relay canonical is authoritative — re-merge the captured
                // response against it and NEVER read disk (which would clobber the open
                // editor buffer). Only a genuinely closed editor falls through to the disk
                // replica below.
                if resolve_zero_live_editors(observe_editor_open(file))
                    == ZeroLiveResolution::KeepEditorAuthority
                {
                    if visible_write_content_matches(&relay_text, expected_current) {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "visible_write_crdt_stale_lease_editor_open_clean file={} source={} expected_len={} expected_hash={} live_editors=0 delivery_converged={} recovery=keep_editor_authority_no_live_replica",
                                file.display(),
                                source,
                                expected_current.len(),
                                agent_doc_hash::content_hash(expected_current),
                                delivery_converged,
                            ),
                        );
                        return Ok(VisibleWriteReconcile::Clean);
                    }
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "visible_write_crdt_stale_lease_editor_open_drift file={} source={} expected_len={} expected_hash={} current_len={} current_hash={} live_editors=0 delivery_converged={} recovery=keep_editor_authority_no_live_replica",
                            file.display(),
                            source,
                            expected_current.len(),
                            agent_doc_hash::content_hash(expected_current),
                            relay_text.len(),
                            relay_hash,
                            delivery_converged,
                        ),
                    );
                    return Ok(VisibleWriteReconcile::DiskDrifted {
                        fresh_current: relay_text,
                    });
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "visible_write_crdt_no_live_editors_disk_authority file={} source={} relay_len={} relay_hash={} delivery_converged={}",
                        file.display(),
                        source,
                        relay_text.len(),
                        relay_hash,
                        delivery_converged,
                    ),
                );
            } else {
                if target_content
                    .is_some_and(|target| visible_write_content_matches(&relay_text, target))
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "visible_write_crdt_current_matches_target file={} source={} target_len={} target_hash={} live_editors={} delivery_converged={}",
                            file.display(),
                            source,
                            relay_text.len(),
                            relay_hash,
                            live_editors,
                            delivery_converged,
                        ),
                    );
                    return Ok(VisibleWriteReconcile::Clean);
                }
                if visible_write_content_matches(&relay_text, expected_current) {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "visible_write_crdt_current_clean file={} source={} expected_len={} expected_hash={} live_editors={} delivery_converged={}",
                            file.display(),
                            source,
                            expected_current.len(),
                            agent_doc_hash::content_hash(expected_current),
                            live_editors,
                            delivery_converged,
                        ),
                    );
                    return Ok(VisibleWriteReconcile::Clean);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "visible_write_crdt_current_drift file={} source={} expected_len={} expected_hash={} current_len={} current_hash={} live_editors={} delivery_converged={}",
                        file.display(),
                        source,
                        expected_current.len(),
                        agent_doc_hash::content_hash(expected_current),
                        relay_text.len(),
                        relay_hash,
                        live_editors,
                        delivery_converged,
                    ),
                );
                return Ok(VisibleWriteReconcile::DiskDrifted {
                    fresh_current: relay_text,
                });
            }
        }
        Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => {}
        Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_editor_authority_unavailable file={} source={} reason=missing_replica",
                    file.display(),
                    source,
                ),
            );
            anyhow::bail!(
                "editor is the current authority for {}; editor authority unavailable: editor_attached_model_missing; disk is a non-authoritative replica and was not read",
                file.display()
            );
        }
        Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_editor_authority_unavailable file={} source={} reason=sync_pending",
                    file.display(),
                    source,
                ),
            );
            anyhow::bail!(
                "editor is the current authority for {}; editor authority unavailable: editor_sync_pending; disk is a non-authoritative replica and was not read",
                file.display()
            );
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_crdt_current_error file={} source={} error={}",
                    file.display(),
                    source,
                    e,
                ),
            );
            anyhow::bail!(
                "failed to resolve editor authority for {}; disk is not consulted until the editor is detached or the relay is current: {e}",
                file.display()
            );
        }
    }
    let actual_current = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read detached-editor disk fallback for {}",
            file.display()
        )
    })?;
    record_disk_replica_authority(file, source, &actual_current);
    if actual_current == expected_current {
        return Ok(VisibleWriteReconcile::Clean);
    }

    // Disk drifted but the live editor buffer did not diverge: a foreign
    // agent-doc supervisor appended to the same document mid-generation. This
    // is reconcilable — report the fresh disk content so the caller re-merges
    // instead of failing closed and stranding the captured response.
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_disk_drift_reconcilable file={} source={} expected_hash={} current_hash={}",
            file.display(),
            source,
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(&actual_current)
        ),
    );
    Ok(VisibleWriteReconcile::DiskDrifted {
        fresh_current: actual_current,
    })
}

fn visible_write_content_matches(left: &str, right: &str) -> bool {
    left == right
        || agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(left)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(right)
}

/// Source the durable editor-buffer feed for `file` from the CP-owned CRDT
/// document model.
///
/// Returns `Some(BufferState)` only when the controller/relay says the active
/// editor model is current. No filesystem live-buffer projection is consulted.
pub fn durable_buffer_state(file: &std::path::Path, disk: &str) -> Option<BufferState> {
    let current = if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file).map(Some)
    } else {
        agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
            file,
            "durable_buffer_state",
        )
    };
    match current {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
            text, live_editors, ..
        })) if live_editors > 0 && text != disk => Some(BufferState::new(
            text,
            true,
            next_document_authority_epoch(),
        )),
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors: 0,
            delivery_converged,
            ..
        })) if text != disk => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "durable_buffer_state_crdt_no_live_editors_ignored file={} relay_len={} relay_hash={} disk_len={} disk_hash={} delivery_converged={}",
                    file.display(),
                    text.len(),
                    agent_doc_hash::content_hash(&text),
                    disk.len(),
                    agent_doc_hash::content_hash(disk),
                    delivery_converged,
                ),
            );
            None
        }
        Ok(Some(_)) | Ok(None) => None,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "durable_buffer_state_cp_unavailable file={} error={}",
                    file.display(),
                    err
                ),
            );
            None
        }
    }
}

/// True iff `content` byte-matches the file's blob at one of the last `limit`
/// commits reachable from HEAD (a previously-committed state — never unsaved
/// work). Best-effort: any git error → `false`.
///
/// A committed blob is by definition a previously-saved, recoverable state, so a
/// match proves `content` holds no unsaved operator edits. This is the shared
/// safety predicate the write/closeout gate keys off of. It never
/// consults timestamps.
pub fn content_matches_recent_committed_blob(
    file: &std::path::Path,
    content: &str,
    limit: usize,
) -> bool {
    let lines = match agent_doc_git_io::revision::recent_commit_lines(file, None, limit) {
        agent_doc_git_io::revision::RecentCommitLog::Lines(lines) => lines,
        _ => return false,
    };
    for line in lines {
        let Some(sha) = line.split_whitespace().next() else {
            continue;
        };
        if let Ok(Some(blob)) = agent_doc_git_io::revision::show_rev(file, sha)
            && blob == content
        {
            return true;
        }
    }
    false
}

/// Resolve the authoritative "current document" for a cycle read.
///
/// Active editor ownership is resolved through the CRDT relay. When the editor
/// is active but the relay is missing or not converged, disk is not a fallback:
/// it is a non-authoritative replica, so callers get a retryable error. Here,
/// "disk replica" means the real session document file on disk. Disk is used
/// only after the relay reports the editor is detached.
pub fn try_resolve_current_doc_from_file(file: &std::path::Path) -> Result<Reconciliation> {
    try_resolve_current_doc_from_file_with_source(file, "resolve_current_doc")
}

pub fn try_resolve_current_doc_from_file_with_source(
    file: &std::path::Path,
    source: &str,
) -> Result<Reconciliation> {
    try_resolve_current_doc_with_disk(file, None, source)
}

pub fn try_resolve_current_doc_from_file_after_model_ensure_with_source(
    file: &std::path::Path,
    source: &str,
) -> Result<Reconciliation> {
    try_resolve_current_doc_with_disk_after_model_ensure(file, None, source)
}

pub fn try_resolve_current_document(file: &std::path::Path) -> Result<CurrentDocument> {
    try_resolve_current_document_with_source(file, "resolve_current_doc")
}

pub fn try_resolve_current_document_with_source(
    file: &std::path::Path,
    source: &str,
) -> Result<CurrentDocument> {
    current_document_projection::resolve_projection(file, source)
        .map(|projection| projection.document().clone())
}

fn try_resolve_current_document_uncached_with_source(
    file: &std::path::Path,
    source: &str,
) -> Result<CurrentDocument> {
    let reconciliation = try_resolve_current_doc_from_file_with_source(file, source)?;
    Ok(CurrentDocument::new(file.to_path_buf(), reconciliation))
}

/// Resolve the current document from disk while preserving the typed document
/// model boundary.
///
/// This is for explicit force-disk recovery paths. Normal cycle reads should
/// prefer [`try_resolve_current_document`] so live editor authority can win.
pub fn resolve_disk_current_document(
    file: &std::path::Path,
    source: &str,
) -> Result<CurrentDocument> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "{source}: failed to read disk-authoritative document {}",
            file.display()
        )
    })?;
    record_disk_replica_authority(file, source, &content);
    Ok(CurrentDocument::new(
        file.to_path_buf(),
        reconcile_current_doc(&content, None),
    ))
}

pub fn resolve_disk_current_document_content(
    file: &std::path::Path,
    source: &str,
) -> Result<String> {
    Ok(resolve_disk_current_document(file, source)?.into_content())
}

/// Read the disk plane **without** recording a disk-replica authority claim.
///
/// [`resolve_disk_current_document`] records
/// `DocumentAuthority::DiskReplica` with reason `editor_detached` as a side
/// effect of reading. That is right for a caller that has *elected* disk
/// authority — as [`observe_live_editor_authority`] puts it, "detached callers
/// should record disk authority after they choose to use disk" — and wrong for
/// a caller that is only *observing*.
///
/// Settlement is an observation, and so is `session-check`, which tells the
/// operator it is status-only. Recording from those paths made the act of
/// checking perturb the authority the next check compares against: consecutive
/// `session-check` runs saw a moving `authority_hash` against a fixed
/// `disk_hash` and never converged, while the guidance said to wait for a
/// settlement that observation itself kept resetting. A read that claims
/// authority is not a read (`#retainedsettlereactive`).
pub fn peek_disk_document_content(file: &std::path::Path, source: &str) -> Result<String> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "{source}: failed to read disk-authoritative document {}",
            file.display()
        )
    })?;
    Ok(
        CurrentDocument::new(file.to_path_buf(), reconcile_current_doc(&content, None))
            .into_content(),
    )
}

pub fn try_resolve_current_document_content(
    file: &std::path::Path,
    source: &str,
) -> Result<String> {
    try_resolve_current_document_with_source(file, source)
        .map(CurrentDocument::into_content)
        .with_context(|| {
            format!(
                "{source}: failed to resolve current document {}",
                file.display()
            )
        })
}

/// True when durable reliable-sync liveness says an editor currently has `file`
/// open (`#6b5h`).
///
/// This is the shared signal that a [`try_resolve_current_document_content`]
/// call would resolve to editor authority rather than disk. The warm read is
/// in-memory; a cold process replays durable receiver/sender state. It uses the
/// OR-set plus process-liveness projection, so an idle-but-open editor remains
/// attached without a heartbeat or lease scan.
pub fn live_editor_endpoint_attached_for_file(file: &std::path::Path) -> bool {
    // P4 (`#6b5h` disk-write guard): the controller helper reads the warm CRDT
    // projection and replays its durable receiver journal / retained sender suffix
    // on a cold process. No plugin-owner lease or live-buffer scan is on this path.
    agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(file)
}

/// Resolve the authoritative current document when the caller already has a
/// detached-disk fallback snapshot from the real document file. Prefer
/// [`try_resolve_current_doc_from_file`] in production hot paths so disk is not
/// read before active editor authority is checked. No live-buffer sidecar or
/// compatibility reader participates in this decision.
pub fn try_resolve_current_doc(file: &std::path::Path, disk: &str) -> Result<Reconciliation> {
    try_resolve_current_doc_with_disk(file, Some(disk), "resolve_current_doc")
}

/// #live-editor-reactive (S2b/S3): the pure authority decision for a resolved relay
/// canonical that currently has **zero live replicas**. Kept pure (no IO) so the
/// repair-vs-demote rule is deterministically testable and shared by the resolve path.
///
/// `live_editors == 0` is a *repairable derived* signal, not ground truth. The ground
/// truth is the editor's open-file set: a file open in an editor can diverge from disk
/// (editor-authoritative), a file open in no editor cannot (disk-authoritative).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroLiveResolution {
    /// The editor still has this document open (ground-truth open set), so the relay
    /// canonical is the editor-authoritative projection — keep editor authority. The
    /// dropped replica re-registers on the editor's next edit (`relay_replica_update`
    /// phantom-heal); demoting to disk here would be the phantom-`live_editors=0` wedge.
    KeepEditorAuthority,
    /// The editor is durably known closed, so no editor buffer can diverge from
    /// disk — disk is the authoritative replica.
    DiskAuthority,
}

pub fn resolve_zero_live_editors(editor_open: bool) -> ZeroLiveResolution {
    if editor_open {
        ZeroLiveResolution::KeepEditorAuthority
    } else {
        ZeroLiveResolution::DiskAuthority
    }
}

/// Legacy pure oracle for the pre-P4 cold-miss behavior. Production
/// [`observe_editor_open`] now uses the reliable-sync receiver journal/outbox
/// replay path; this remains test-only to preserve the migration regression
/// model without keeping lease IO in a production authority reader.
#[cfg(test)]
fn observe_editor_open_in(
    registry: &agent_doc_document_realtime::editor_open_docs::EditorOpenDocs,
    path: &str,
    lease_attached: impl FnOnce() -> bool,
) -> bool {
    if !registry.is_tracked(path) {
        // Cold miss (never-seen doc): recover the truth from the durable backup ONCE and
        // seed the reactive authority. The relay resolve path only handles agent-doc
        // session documents, so an open document here is an agent-doc by construction.
        if lease_attached() {
            registry.mark_open_with(path, || true);
        } else {
            registry.mark_closed(path);
        }
    }
    registry.is_open(path)
}

/// #live-editor-reactive (S2b/S3/S4): observe whether the editor holds `file` open.
///
/// P4 (`#live-editors-lazily-plane`): the lazily OR-set projection is the hot
/// authority. A cold process replays the controller's durable receipt journal and
/// the sender's retained suffix, so controller recycle / socket reconnect cannot
/// spuriously read `live_editors == 0` and no plugin-owner lease is reconciled here.
fn observe_editor_open(file: &std::path::Path) -> bool {
    agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(file)
}

fn try_resolve_current_doc_with_disk(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
) -> Result<Reconciliation> {
    try_resolve_current_doc_with_disk_inner(file, disk, source, false)
}

fn try_resolve_current_doc_with_disk_after_model_ensure(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
) -> Result<Reconciliation> {
    try_resolve_current_doc_with_disk_inner(file, disk, source, true)
}

/// `#bn41` / `#px82` — how many times the realtime resolve re-observes editor
/// authority before surfacing `editor_attached_model_missing`. Override with
/// `AGENT_DOC_EDITOR_REPLICA_REOBSERVE_ATTEMPTS`.
const DEFAULT_EDITOR_REPLICA_REOBSERVE_ATTEMPTS: u32 = 3;
const EDITOR_REPLICA_REOBSERVE_ATTEMPTS_ENV: &str = "AGENT_DOC_EDITOR_REPLICA_REOBSERVE_ATTEMPTS";
const EDITOR_REPLICA_REOBSERVE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);

fn editor_replica_reobserve_attempts() -> u32 {
    std::env::var(EDITOR_REPLICA_REOBSERVE_ATTEMPTS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|attempts| *attempts >= 1)
        .unwrap_or(DEFAULT_EDITOR_REPLICA_REOBSERVE_ATTEMPTS)
}

/// `#bn41` — attempt the replica re-registration the binary already *reports* as
/// `editor_replica_reregister=requested` BEFORE surfacing the error.
///
/// The missing piece in this failure family is the editor REPLICA, not the
/// controller: `admin recycle` + `admin reload-lib` clears it by hand with no
/// data loss, which means the binary can clear it itself. `#px82` adds the other
/// half — the failure is INTERMITTENT (observed alternating FAIL/OK across
/// back-to-back preflights), so one bounded observation is a coin flip. Request
/// re-registration, then re-observe within a bounded attempt budget.
///
/// Only `EditorAttachedMissingReplica` is re-observed here. `EditorSyncPending`
/// is a live editor mid-sync, which settles on its own and must not be nudged
/// with a force-refresh; every other status is already settled.
fn observation_is_missing_replica_family(
    file: &std::path::Path,
    observed: &Result<agent_doc_crdt_relay_io::CurrentText>,
) -> bool {
    match observed {
        Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => true,
        // An authority *error* raised while Lazily still reports the editor open
        // is the same family: the replica is gone but the editor is not. When
        // the editor is genuinely closed this is an ordinary detached/idle path
        // and must keep its existing disk-fallback behavior.
        Err(_) => observe_editor_open(file),
        _ => false,
    }
}

/// The realtime liveness state a self-heal exhaustion was recorded against.
///
/// Derived entirely from the Lazily reliable-sync plane, so it changes exactly
/// when the document's editor registration set changes — an editor attaching,
/// detaching, or re-registering. Sync epochs are deliberately excluded: those
/// advance on every keystroke, and an edit is not evidence that a missing
/// replica came back.
#[derive(Clone, PartialEq, Eq)]
struct EditorReplicaLivenessWitness {
    live: bool,
    /// `(pid, editor_id, registration timestamp)`, sorted for a stable identity.
    ///
    /// `pid` keeps the plane's native [`agent_doc_reliable_sync_io::liveness::Pid`]
    /// width. Narrowing it would be lossy in the one operation this type exists
    /// for: the witness is compared by equality, so any saturating conversion
    /// could map two distinct editors onto the same value and suppress a
    /// recovery that should have re-armed.
    registrations: Vec<(agent_doc_reliable_sync_io::liveness::Pid, String, u64)>,
}

/// Observe the current realtime liveness witness for `file`.
///
/// This is one in-memory projection read on the hot path (a cold process
/// hydrates the durable journal once), so it is cheap enough to consult on
/// every resolve — unlike the IPC receipt round-trip it guards.
fn editor_replica_liveness_witness(file: &std::path::Path) -> EditorReplicaLivenessWitness {
    let mut registrations: Vec<(agent_doc_reliable_sync_io::liveness::Pid, String, u64)> =
        agent_doc_crdt_relay_io::reliable_sync_editor_registrations_for_file(file)
            .into_iter()
            .map(|reg| (reg.pid, reg.editor_id, reg.timestamp_ms))
            .collect();
    registrations.sort();
    EditorReplicaLivenessWitness {
        live: observe_editor_open(file),
        registrations,
    }
}

/// Files whose replica self-heal was exhausted without recovering, remembered
/// against the liveness witness that was current when it failed.
static EDITOR_REPLICA_SELF_HEAL_EXHAUSTED: std::sync::LazyLock<
    Mutex<std::collections::HashMap<std::path::PathBuf, EditorReplicaLivenessWitness>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// `true` when the retry loop should be paused for this file: the self-heal
/// already failed against exactly this liveness witness, so another attempt is
/// provably futile and resolution should fall through immediately.
///
/// Paused, not stopped — the suppression lifts the moment the witness changes,
/// with no timer involved.
///
/// Reads used to BAIL when the editor could not answer, so the self-heal was
/// paid at most once per operation. Now that reads descend to disk and the
/// caller continues, every later read site would re-pay the full loop —
/// measured as 3 attempts x a 6s IPC receipt timeout each, which turned a ~57s
/// compact/commit into ~152s.
///
/// This memo is invalidated by the realtime document model rather than by a
/// clock. A TTL answers "did this fail recently?", which is a guess about
/// whether the replica returned: it keeps suppressing recovery for the rest of
/// the window after the editor is already back, and it re-pays the whole loop
/// once the window lapses even when nothing changed. Keying on the Lazily
/// liveness witness answers the question that actually matters — "has anything
/// changed since it failed?" — so recovery is immediate when a registration
/// lands and free when it has not.
fn should_pause_editor_replica_self_heal(
    file: &std::path::Path,
    witness: &EditorReplicaLivenessWitness,
) -> bool {
    EDITOR_REPLICA_SELF_HEAL_EXHAUSTED
        .lock()
        .get(file)
        .is_some_and(|recorded| recorded == witness)
}

fn record_editor_replica_self_heal_exhausted(
    file: &std::path::Path,
    witness: EditorReplicaLivenessWitness,
) {
    EDITOR_REPLICA_SELF_HEAL_EXHAUSTED
        .lock()
        .insert(file.to_path_buf(), witness);
}

fn clear_editor_replica_self_heal_exhausted(file: &std::path::Path) {
    EDITOR_REPLICA_SELF_HEAL_EXHAUSTED.lock().remove(file);
}

/// Files whose terminal missing-replica plugin rebuild has already been asked
/// for, remembered against the liveness witness current when it was asked.
///
/// Separate from [`EDITOR_REPLICA_SELF_HEAL_EXHAUSTED`] on purpose: that memo
/// guards the upstream re-registration loop, this one guards the single
/// last-chance rebuild below it. Sharing one map would let either recovery
/// suppress the other.
static MISSING_REPLICA_TERMINAL_REBUILD_ASKED: std::sync::LazyLock<
    Mutex<std::collections::HashMap<std::path::PathBuf, EditorReplicaLivenessWitness>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Claim the one terminal rebuild attempt for `file` at the current witness.
///
/// Returns `true` at most once per liveness witness. This is the whole reason
/// missing-replica can afford a rebuild here at all: a single compact/commit
/// performs several resolutions, and paying the rebuild at each of them is the
/// measured ~28s-per-read-site regression that made missing-replica ineligible
/// for the Tier 1 rebuild above. Latching on the witness — not a clock — means
/// the attempt re-arms the instant a registration actually changes.
fn claim_terminal_missing_replica_rebuild(
    file: &std::path::Path,
    witness: &EditorReplicaLivenessWitness,
) -> bool {
    let mut asked = MISSING_REPLICA_TERMINAL_REBUILD_ASKED.lock();
    if asked.get(file).is_some_and(|recorded| recorded == witness) {
        return false;
    }
    asked.insert(file.to_path_buf(), witness.clone());
    true
}

fn clear_terminal_missing_replica_rebuild(file: &std::path::Path) {
    MISSING_REPLICA_TERMINAL_REBUILD_ASKED.lock().remove(file);
}

fn reobserve_missing_editor_replica_with_reregistration(
    file: &std::path::Path,
    source: &str,
    require_model_ensure: bool,
    observed: Result<agent_doc_crdt_relay_io::CurrentText>,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if !observation_is_missing_replica_family(file, &observed) {
        // A healthy observation means the editor is answering again; drop any
        // remembered exhaustion so the next failure gets a full retry.
        clear_editor_replica_self_heal_exhausted(file);
        return observed;
    }
    if should_pause_editor_replica_self_heal(file, &editor_replica_liveness_witness(file)) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_replica_reregister_skipped file={} source={} reason=self_heal_exhausted_for_unchanged_liveness_witness",
                file.display(),
                source,
            ),
        );
        return observed;
    }
    let attempts = editor_replica_reobserve_attempts();
    let mut current = observed;
    for attempt in 1..=attempts {
        let reregister = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
            file,
            agent_doc_crdt_relay_io::CrdtReplicaEventReason::CanonicalProjection,
            0,
        ) {
            Ok(()) => "requested".to_string(),
            Err(err) => format!("failed:{}", format!("{err:#}").replace('\n', "\\n")),
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_replica_reregister_attempt file={} source={} attempt={}/{} reregister={} (#bn41)",
                file.display(),
                source,
                attempt,
                attempts,
                reregister
            ),
        );
        std::thread::sleep(EDITOR_REPLICA_REOBSERVE_BACKOFF);
        let reobserved = if require_model_ensure {
            query_live_editor_authority_after_model_ensure(file, source)
        } else {
            query_live_editor_authority(file, source)
        };
        let status = match &reobserved {
            Ok(agent_doc_crdt_relay_io::CurrentText::Current { .. }) => "current",
            Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => "detached",
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => {
                "editor_attached_model_missing"
            }
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => "editor_sync_pending",
            Err(_) => "error",
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_replica_reobserved file={} source={} attempt={}/{} status={} (#px82)",
                file.display(),
                source,
                attempt,
                attempts,
                status
            ),
        );
        current = reobserved;
        if !observation_is_missing_replica_family(file, &current) {
            clear_editor_replica_self_heal_exhausted(file);
            return current;
        }
    }
    // Every attempt was spent without recovering. Remember the liveness witness
    // as of the END of the loop: a registration that landed while we were
    // retrying is already reflected here, so only genuinely newer realtime state
    // re-arms the retry. Later reads in this operation fall through for free.
    record_editor_replica_self_heal_exhausted(file, editor_replica_liveness_witness(file));
    current
}

fn try_resolve_current_doc_with_disk_inner(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
    require_model_ensure: bool,
) -> Result<Reconciliation> {
    let observed = if require_model_ensure {
        query_live_editor_authority_after_model_ensure(file, source)
    } else {
        query_live_editor_authority(file, source)
    };
    // `#bn41`/`#px82`: self-heal the missing-replica family BEFORE surfacing it.
    // Both shapes belong to it — an explicit `EditorAttachedMissingReplica`
    // status and an authority *error* raised while Lazily still reports the
    // editor open. The latter is what reaches
    // `resolve_closed_editor_disk_fallback_current_doc`'s fail-closed bail.
    let observed = reobserve_missing_editor_replica_with_reregistration(
        file,
        source,
        require_model_ensure,
        observed,
    );
    let current = match observed {
        Ok(current) => current,
        Err(e) => {
            let detail = format!("{e:#}");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{} file={} source={} error={}",
                    OpsLogEvent::RealtimeDocResolveCrdtError,
                    file.display(),
                    source,
                    e,
                ),
            );
            return resolve_closed_editor_disk_fallback_current_doc(
                file,
                disk,
                source,
                if require_model_ensure {
                    "model_ensure_error"
                } else {
                    "editor_authority_error"
                },
                Some(&detail),
            );
        }
    };
    match current {
        agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged,
            ..
        } => {
            if live_editors == 0 {
                // #live-editor-reactive (S2b/S3): route the zero-live-replica decision
                // through the reliable-sync open-docs projection instead of demoting
                // on the raw `live_editors == 0` poll. An editor that still has the doc
                // open keeps editor authority — demoting to disk here is the phantom-
                // `live_editors=0` wedge that stranded pane sync and logged `authority=disk`
                // every second while the plugin was alive. Only a genuinely closed editor
                // falls through to disk.
                if resolve_zero_live_editors(observe_editor_open(file))
                    == ZeroLiveResolution::KeepEditorAuthority
                {
                    record_editor_relay_authority(file, source, &text);
                    let reconciliation = Reconciliation {
                        authority: agent_doc_document_realtime::DocAuthority::EditorBuffer,
                        content: text,
                        diverged: false,
                        reason: "crdt_relay_stale_lease_editor_open",
                    };
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "{} authority={} reason={} diverged={} file={} source={} answered_by=crdt_relay live_editors={} delivery_converged={} editor_open=true recovery=keep_editor_authority_no_live_replica",
                            OpsLogEvent::RealtimeDocResolve,
                            reconciliation.authority.as_str(),
                            reconciliation.reason,
                            reconciliation.diverged,
                            file.display(),
                            source,
                            live_editors,
                            delivery_converged,
                        ),
                    );
                    return Ok(reconciliation);
                }
                let relay_len = text.len();
                let relay_hash = agent_doc_hash::content_hash(&text);
                let disk = match disk {
                    Some(disk) => disk.to_string(),
                    None => std::fs::read_to_string(file).with_context(|| {
                        format!(
                            "relay has no live editors for {}; failed to read disk fallback replica",
                            file.display()
                        )
                    })?,
                };
                record_disk_replica_authority(file, source, &disk);
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "realtime_doc_resolve_crdt_no_live_editors_disk_authority file={} source={} relay_len={} relay_hash={} disk_len={} disk_hash={} delivery_converged={}",
                        file.display(),
                        source,
                        relay_len,
                        relay_hash,
                        disk.len(),
                        agent_doc_hash::content_hash(&disk),
                        delivery_converged,
                    ),
                );
                return Ok(resolve_disk_only_current_doc(file, &disk, "editor_absent", source));
            }
            record_editor_relay_authority(file, source, &text);
            let reconciliation = Reconciliation {
                authority: agent_doc_document_realtime::DocAuthority::EditorBuffer,
                content: text,
                diverged: false,
                reason: "crdt_relay_current",
            };
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{} authority={} reason={} diverged={} file={} source={} answered_by=crdt_relay live_editors={} delivery_converged={}",
                    OpsLogEvent::RealtimeDocResolve,
                    reconciliation.authority.as_str(),
                    reconciliation.reason,
                    reconciliation.diverged,
                    file.display(),
                    source,
                    live_editors,
                    delivery_converged,
                ),
            );
            Ok(reconciliation)
        }
        agent_doc_crdt_relay_io::CurrentText::Detached => {
            let disk = match disk {
                Some(disk) => disk.to_string(),
                None => std::fs::read_to_string(file).with_context(|| {
                    format!(
                        "editor is detached for {}; failed to read disk fallback replica",
                        file.display()
                    )
                })?,
            };
            record_disk_replica_authority(file, source, &disk);
            Ok(resolve_detached_current_doc(file, &disk, source))
        }
        // Read-path precedence is **editor buffer, then disk** — and only those
        // two tiers (no git tier).
        //
        // Both arms below mean "an editor is attached but cannot answer right
        // now": its replica is missing, or its sync has not converged. These
        // used to bail, which wedged every READ of the document behind a
        // transient editor condition even though the last saved disk image was
        // available and adequate to read.
        //
        // The descent is deliberately scoped to READS. Commit authority is a
        // SEPARATE guard (`agent-doc-commit-io`, "editor is the current
        // authority ... was not used as commit authority") and is intentionally
        // left fail-closed: reading a slightly stale disk image is recoverable,
        // but *committing* one while the editor holds newer unsaved text
        // destroys operator edits — the `content_ours` clobber class the write
        // path exists to refuse. So a read may descend to disk; a write may not.
        //
        // Disk is still marked a replica (`record_disk_replica_authority`), so
        // nothing downstream mistakes this for editor-authoritative text.
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            resolve_editor_unavailable_disk_read_fallback(file, disk, source, "missing_replica")
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            resolve_editor_unavailable_disk_read_fallback(file, disk, source, "sync_pending")
        }
    }
}

/// Read-path resolution when an attached editor cannot answer.
///
/// Precedence is `editor buffer -> disk`, with **rebuild before descent**: if an
/// editor buffer is available, rebuild the document model from it rather than
/// dropping a tier. Disk is the fallback for when the rebuild cannot produce a
/// live model, not the immediate next step.
///
/// This matters because the editor buffer is the only tier that can hold unsaved
/// operator text. Descending to disk on the first unavailable observation would
/// silently read *past* edits that are still live in the editor, and a
/// recoverable transient (a replica still registering, a sync mid-flight) would
/// be treated as a lost tier. Rebuilding turns most of those transients back
/// into an editor-authoritative read.
///
/// Used only for resolving current text. Commit authority never descends here.
fn resolve_editor_unavailable_disk_read_fallback(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
    reason: &str,
) -> Result<Reconciliation> {
    use agent_doc_turn::authority_recovery::{
        AuthorityObservation, AuthorityRecoveryDecision, AuthorityRecoveryFacts,
        decide_authority_recovery,
    };

    // Tier 1 retry: rebuild the model from the live editor buffer.
    //
    // Two preconditions:
    //
    // 1. Lazily still reports the editor open — the "there is an editor buffer
    //    available" precondition. With no editor there is nothing to rebuild
    //    from and this is an ordinary detached read.
    // 2. The observation is `sync_pending`. The missing-replica family is
    //    ALREADY self-healed upstream by
    //    `reobserve_missing_editor_replica_with_reregistration` (#bn41/#px82),
    //    which re-registers and re-observes with backoff before we ever get
    //    here. Rebuilding it again duplicates that work at real cost — measured
    //    ~28s per read site, which multiplied across the several resolutions a
    //    single compact/commit performs and pushed those tests past their
    //    timeout. Once that upstream refresh exhausts, missing-replica must fail
    //    closed while the editor remains open. Sync-pending has no upstream
    //    self-heal, so the rebuild is the only attempt there.
    let observation = match reason {
        "missing_replica" => AuthorityObservation::MissingReplica,
        "sync_pending" => AuthorityObservation::SyncPending,
        _ => AuthorityObservation::Error,
    };
    let initial_decision = decide_authority_recovery(AuthorityRecoveryFacts {
        observation,
        editor_open: observe_editor_open(file),
        retries_remaining: false,
        // Missing-replica already spent its plugin refresh loop upstream.
        rebuild_after_retry_exhaustion: reason == "sync_pending",
    });
    let rebuild_eligible = matches!(
        initial_decision,
        AuthorityRecoveryDecision::RebuildFromPlugin
    );
    if rebuild_eligible {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_doc_resolve_editor_model_rebuild_attempt file={} source={} reason={} \
                 precedence=editor_then_disk",
                file.display(),
                source,
                reason,
            ),
        );
        match ensure_document_model_through_authority(file, source) {
            Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                text,
                live_editors,
                delivery_converged,
                ..
            }) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "realtime_doc_resolve_editor_model_rebuilt file={} source={} reason={} \
                         tier=editor_buffer live_editors={} delivery_converged={}",
                        file.display(),
                        source,
                        reason,
                        live_editors,
                        delivery_converged,
                    ),
                );
                // The rebuild restored the editor tier — resolve as editor
                // authority and never touch disk.
                record_editor_relay_authority(file, source, &text);
                let reconciliation = Reconciliation {
                    authority: agent_doc_document_realtime::DocAuthority::EditorBuffer,
                    content: text,
                    diverged: false,
                    reason: "crdt_relay_current",
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{} authority={} reason={} diverged={} file={} \
                         source={} answered_by=crdt_relay live_editors={} delivery_converged={} \
                         recovery=editor_model_rebuilt",
                        OpsLogEvent::RealtimeDocResolve,
                        reconciliation.authority.as_str(),
                        reconciliation.reason,
                        reconciliation.diverged,
                        file.display(),
                        source,
                        live_editors,
                        delivery_converged,
                    ),
                );
                return Ok(reconciliation);
            }
            Ok(other) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "realtime_doc_resolve_editor_model_rebuild_incomplete file={} source={} \
                         reason={} status={}",
                        file.display(),
                        source,
                        reason,
                        current_text_status(&other),
                    ),
                );
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "realtime_doc_resolve_editor_model_rebuild_failed file={} source={} \
                         reason={} error={}",
                        file.display(),
                        source,
                        reason,
                        format!("{err:#}").replace('\n', "\\n"),
                    ),
                );
            }
        }
    }

    // `#missingreplicarebuild` — last chance before failing closed.
    //
    // The operator directive is: realtime model, then rebuild from the plugin,
    // then disk. Missing-replica is excluded from the Tier 1 rebuild above
    // because `reobserve_missing_editor_replica_with_reregistration` already
    // refreshes it upstream. But when THAT exhausts, this path used to fail
    // closed without ever asking the plugin to rebuild — leaving an operator
    // whose editor is attached-but-not-answering no in-binary recovery at all,
    // and forcing a manual IDE restart to re-register the replica.
    //
    // Ask exactly once, here at the exhaustion boundary, and only then fail
    // closed. Disk is still never adopted while the editor is open — that
    // invariant belongs to the descent decision below and is unchanged.
    //
    // The witness latch is what makes this affordable: the several resolutions a
    // single compact/commit performs share ONE attempt instead of each paying
    // the rebuild, which is the regression that made missing-replica ineligible
    // above. It re-arms only when a registration actually changes.
    if !rebuild_eligible && reason == "missing_replica" && observe_editor_open(file) {
        let witness = editor_replica_liveness_witness(file);
        if claim_terminal_missing_replica_rebuild(file, &witness) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "realtime_doc_resolve_missing_replica_terminal_rebuild_attempt file={} \
                     source={} reason={} precedence=editor_then_disk scope=once_per_witness",
                    file.display(),
                    source,
                    reason,
                ),
            );
            match ensure_document_model_through_authority(file, source) {
                Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                    text,
                    live_editors,
                    delivery_converged,
                    ..
                }) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "realtime_doc_resolve_missing_replica_terminal_rebuilt file={} \
                             source={} reason={} tier=editor_buffer live_editors={} \
                             delivery_converged={}",
                            file.display(),
                            source,
                            reason,
                            live_editors,
                            delivery_converged,
                        ),
                    );
                    // The plugin answered: the editor tier is restored, so this
                    // resolution never reaches the disk question at all.
                    record_editor_relay_authority(file, source, &text);
                    clear_terminal_missing_replica_rebuild(file);
                    let reconciliation = Reconciliation {
                        authority: agent_doc_document_realtime::DocAuthority::EditorBuffer,
                        content: text,
                        diverged: false,
                        reason: "crdt_relay_current",
                    };
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "{} authority={} reason={} diverged={} file={} \
                             source={} answered_by=crdt_relay live_editors={} delivery_converged={} \
                             recovery=missing_replica_terminal_rebuild",
                            OpsLogEvent::RealtimeDocResolve,
                            reconciliation.authority.as_str(),
                            reconciliation.reason,
                            reconciliation.diverged,
                            file.display(),
                            source,
                            live_editors,
                            delivery_converged,
                        ),
                    );
                    return Ok(reconciliation);
                }
                Ok(other) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "realtime_doc_resolve_missing_replica_terminal_rebuild_incomplete \
                             file={} source={} reason={} status={}",
                            file.display(),
                            source,
                            reason,
                            current_text_status(&other),
                        ),
                    );
                }
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "realtime_doc_resolve_missing_replica_terminal_rebuild_failed file={} \
                             source={} reason={} error={}",
                            file.display(),
                            source,
                            reason,
                            format!("{err:#}").replace('\n', "\\n"),
                        ),
                    );
                }
            }
        }
    }

    // Tier 2 is reachable only after the editor is proven detached. A failed
    // rebuild while it remains open must not turn stale disk into current text.
    let descent_decision = decide_authority_recovery(AuthorityRecoveryFacts {
        observation,
        editor_open: observe_editor_open(file),
        retries_remaining: false,
        rebuild_after_retry_exhaustion: false,
    });
    if descent_decision == AuthorityRecoveryDecision::FailClosed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_doc_resolve_disk_read_refused file={} source={} reason={} \
                 editor_open=true recovery={} invariant=attached_editor_never_descends_to_disk",
                file.display(),
                source,
                reason,
                if rebuild_eligible {
                    "plugin_rebuild_exhausted"
                } else {
                    "upstream_replica_refresh_exhausted"
                },
            ),
        );
        anyhow::bail!(
            "editor is still attached for {}; {reason} recovery exhausted and disk read authority is refused",
            file.display()
        );
    }
    debug_assert_eq!(
        descent_decision,
        AuthorityRecoveryDecision::DescendToDisk,
        "only a detached editor may reach disk fallback"
    );

    // The editor is detached — read the disk replica.
    let disk = match disk {
        Some(disk) => disk.to_string(),
        None => std::fs::read_to_string(file).with_context(|| {
            format!(
                "read {} from disk after the attached editor was unavailable ({reason})",
                file.display()
            )
        })?,
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "realtime_doc_resolve_disk_read_fallback file={} source={} reason={} tier=disk \
         precedence=editor_then_disk scope=read_only editor_open=false after_rebuild_attempt={} disk_len={}",
            file.display(),
            source,
            reason,
            rebuild_eligible,
            disk.len(),
        ),
    );
    record_disk_replica_authority(file, source, &disk);
    Ok(resolve_detached_current_doc(file, &disk, source))
}

/// Resolve the detached-editor fallback path.
///
fn resolve_detached_current_doc(
    file: &std::path::Path,
    disk: &str,
    source: &str,
) -> Reconciliation {
    let buffer = durable_buffer_state(file, disk);
    let reconciliation = reconcile_current_doc(disk, buffer.as_ref());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} authority={} reason={} diverged={} file={} source={} answered_by=detached",
            OpsLogEvent::RealtimeDocResolve,
            reconciliation.authority.as_str(),
            reconciliation.reason,
            reconciliation.diverged,
            file.display(),
            source,
        ),
    );
    reconciliation
}

fn resolve_disk_only_current_doc(
    file: &std::path::Path,
    disk: &str,
    reason: &'static str,
    source: &str,
) -> Reconciliation {
    let reconciliation = reconcile_current_doc(disk, None);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} authority={} reason={} diverged={} file={} source={} answered_by=disk_only",
            OpsLogEvent::RealtimeDocResolve,
            reconciliation.authority.as_str(),
            reason,
            reconciliation.diverged,
            file.display(),
            source,
        ),
    );
    Reconciliation {
        reason,
        ..reconciliation
    }
}

// ---------------------------------------------------------------------------
// Retained-write settlement as one derived fact (`#retainedsettlereactive`).
//
// `preflight` and `session-check` must agree on "is a retained document write
// still unsettled?". They used to derive it from different inputs and never
// shared a cell, which let both "unsettled" and "ok" be true at once and
// deadlocked the session. These adapters feed the observations into the
// document-scoped cells in `agent_doc_state_backbone::retained_write` and hand
// both consumers the *same* `Computed`.
// ---------------------------------------------------------------------------

use agent_doc_state_backbone::retained_write::{
    ContentObservation, RetainedIntentFacts, RetainedWriteSettlement, SettlementVerdict,
};

/// The captured response body for `file`'s current cycle, if one is projected.
///
/// This is the intent's semantic payload: the thing whose presence in the
/// converged document proves a rebased intent actually landed.
fn projected_captured_response(file: &Path) -> Option<String> {
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file).ok()??;
    let capture_id = state.capture_id.as_deref()?;
    let capture =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id).ok()??;
    (capture.cycle_id == state.cycle_id).then_some(capture.response_body)
}

/// Observe one content plane. `Err` becomes `None` — "I could not look" is a
/// distinct outcome from "I looked and it is outstanding"
/// (`#idlerevisionreactive`), and collapsing them would turn a transport blip
/// into a permanent refusal to open a cycle.
fn observe_plane(
    content: Result<String>,
    payload: Option<&str>,
    intent_added_lines: &[&str],
) -> Option<ContentObservation> {
    let content = content.ok()?;
    let payload_materialized = payload.is_some_and(|payload| {
        agent_doc_turn::response_replay::response_materialized_in_content(payload, &content)
    });
    Some(ContentObservation {
        content_hash: agent_doc_hash::content_hash(&content),
        payload_materialized,
        intent_delta_materialized:
            agent_doc_state_backbone::retained_write::added_lines_materialized_in(
                intent_added_lines,
                &content,
            ),
    })
}

/// Build the document-scoped settlement cells for `file` from live observations.
///
/// Storage hydrates the sources here and nowhere else; it never arbitrates the
/// decision (`#lzdurablesink`).
fn observe_retained_write_settlement(file: &Path, source: &str) -> RetainedWriteSettlement {
    let scope = agent_doc_state_backbone::DocumentScope::new();
    let settlement = RetainedWriteSettlement::new_in(&scope);

    let Some(pending) = pending_document_write(file) else {
        // No intent: the verdict is `NoRetainedIntent` without paying for a
        // single content read.
        return settlement;
    };

    let captured_response = projected_captured_response(file);
    // The intent carries a response payload only if its own target actually
    // contains that response. A delivery-only projection has nothing that can
    // stand in for its byte target, so it must settle on exact bytes.
    let payload = captured_response.as_deref().filter(|payload| {
        agent_doc_turn::response_replay::response_materialized_in_content(
            payload,
            &pending.target_content,
        )
    });

    // The lines this intent was going to add. A closeout writes more than once
    // (response/backlog, then the queue mirror), so an interrupted one leaves the
    // earlier intent stamped against bytes its own successor already replaced;
    // the delta is what proves that successor carried it.
    let added_lines = agent_doc_state_backbone::retained_write::intent_added_lines(
        pending.expected_content.as_deref(),
        &pending.target_content,
    );

    settlement.observe_pending(Some(RetainedIntentFacts {
        intent_id: pending.intent_id.clone(),
        target_hash: pending.target_hash.clone(),
        reason: pending.reason.clone(),
        source: pending.source.clone(),
        superseding_stage: superseding_closeout_stage(file, &pending),
        carries_response_payload: payload.is_some(),
        carries_content_delta: !added_lines.is_empty(),
    }));
    settlement.observe_authority(observe_plane(
        try_resolve_current_document_content(file, source),
        payload,
        &added_lines,
    ));
    // Observation must not claim authority: `peek_disk_document_content` reads
    // the same bytes without recording a disk-replica authority claim.
    settlement.observe_disk(observe_plane(
        peek_disk_document_content(file, source),
        payload,
        &added_lines,
    ));
    settlement
}

/// Is the retained intent stranded rather than merely slow?
///
/// True when authority and disk have already converged and the intent still
/// cannot settle: there is no delivery in flight, so no amount of waiting or
/// retrying changes the answer. Guidance that says "retry" is actively wrong in
/// this state, which is what turned one wedge into a long escalation on
/// 2026-07-26. Kept here, beside the settlement adapters, so `session-check`
/// asks the shared verdict rather than re-deriving the condition.
pub fn retained_write_is_stranded(file: &Path, source: &str) -> bool {
    matches!(
        retained_write_settlement(file, source),
        SettlementVerdict::Unsettled {
            cause: agent_doc_state_backbone::retained_write::UnsettledCause::PayloadAbsentFromConvergedContent,
            ..
        }
    )
}

/// Binary-owned cycle boundary requesting retained-write recovery.
///
/// These values are internal transition provenance, not editor wire protocol.
/// Keeping the closed set typed prevents preflight, session-check, and finalize
/// from silently inventing mismatched recovery/gate source labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedWriteCycleBoundary {
    Preflight,
    SessionCheck,
    FinalizePreCapture,
    RegressionTest,
}

impl RetainedWriteCycleBoundary {
    pub const fn recovery_source(self) -> &'static str {
        match self {
            Self::Preflight => "preflight_retained_write_recovery",
            Self::SessionCheck => "session_check_retained_write_recovery",
            Self::FinalizePreCapture => "finalize_pre_capture_retained_write_recovery",
            Self::RegressionTest => "preflight_retained_write_recovery_test",
        }
    }

    pub const fn gate_source(self) -> &'static str {
        match self {
            Self::Preflight => "preflight_retained_document_write_gate",
            Self::SessionCheck => "session_check_retained_write_gate",
            Self::FinalizePreCapture => "finalize_pre_capture_retained_write_gate",
            Self::RegressionTest => "retained_write_recovery_test_gate",
        }
    }

    pub const fn recovered_settlement_source(self) -> &'static str {
        match self {
            Self::Preflight => "preflight_retained_write_recovered_settlement",
            Self::SessionCheck => "session_check_retained_write_recovered_settlement",
            Self::FinalizePreCapture => "finalize_pre_capture_retained_write_recovered_settlement",
            Self::RegressionTest => "retained_write_recovery_test_settlement",
        }
    }
}

/// Recover a causally replayable retained write before a binary-owned boundary
/// opens or accepts a new response cycle (`#0dsr`).
/// response cycle (`#0dsr`).
///
/// Editor reconnect already replays this journal, but a healthy long-lived
/// editor may never reconnect. Once the shared settlement verdict proves that
/// authority and disk agree while the intent is still absent, waiting cannot
/// make progress. Reuse the same content-bearing semantic rebase over the exact
/// current authority cut, then clear the journal only after canonical and disk
/// both prove the replayed target. A delivery still in flight stays untouched.
pub fn recover_retained_document_write_before_new_cycle(
    file: &Path,
    boundary: RetainedWriteCycleBoundary,
) -> Result<bool> {
    use agent_doc_state_backbone::retained_write::RecoveryAction;

    let source = boundary.recovery_source();
    let action = match retained_write_settlement(file, source).recovery_action() {
        RecoveryAction::AwaitConvergence { intent_id } => {
            if !settle_live_editor_projection_through_authority(file, source)? {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "retained_write_preflight_recovery_deferred file={} source={} intent_id={} reason=editor_native_save_pending",
                        file.display(),
                        source,
                        intent_id,
                    ),
                );
                return Ok(false);
            }

            match retained_write_settlement(file, source).recovery_action() {
                RecoveryAction::Continue => {
                    let canonical = try_resolve_current_document_content(file, source)?;
                    clear_all_deferred_document_write_intents(file, source)?;
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "retained_write_preflight_recovered file={} source={} intent_id={} target_hash={} canonical_disk_exact=true recovery=editor_native_save operator_cut_preserved=true",
                            file.display(),
                            source,
                            intent_id,
                            agent_doc_hash::content_hash(&canonical),
                        ),
                    );
                    return Ok(true);
                }
                action => action,
            }
        }
        action => action,
    };
    let intent_id = match action {
        RecoveryAction::Continue => return Ok(false),
        RecoveryAction::AwaitConvergence { intent_id } => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_write_preflight_recovery_deferred file={} source={} intent_id={} reason=authority_disk_diverged_after_editor_native_save",
                    file.display(),
                    source,
                    intent_id,
                ),
            );
            return Ok(false);
        }
        RecoveryAction::ReplayStranded { intent_id } => intent_id,
    };

    let canonical = try_resolve_current_document_content(file, source)?;
    let Some(replayed_target) = deferred_document_write_reconnect_content(file, &canonical)? else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "retained_write_preflight_recovery_deferred file={} source={} intent_id={} reason=no_safe_replay_target",
                file.display(),
                source,
                intent_id,
            ),
        );
        return Ok(false);
    };
    validate_canonical_document_target(file, &replayed_target, source)?;

    if replayed_target != canonical {
        atomic_write_if_current_through_authority(file, &replayed_target, &canonical, source)?;
    }

    let settled_canonical = try_resolve_current_document_content(file, source)?;
    let settled_disk = resolve_disk_current_document_content(file, source)?;
    if settled_canonical != replayed_target || settled_disk != replayed_target {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "retained_write_preflight_recovery_deferred file={} source={} intent_id={} reason=replay_not_converged target_hash={} canonical_hash={} disk_hash={}",
                file.display(),
                source,
                intent_id,
                agent_doc_hash::content_hash(&replayed_target),
                agent_doc_hash::content_hash(&settled_canonical),
                agent_doc_hash::content_hash(&settled_disk),
            ),
        );
        return Ok(false);
    }

    clear_all_deferred_document_write_intents(file, source)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "retained_write_preflight_recovered file={} source={} intent_id={} target_hash={} canonical_disk_exact=true operator_cut_preserved=true",
            file.display(),
            source,
            intent_id,
            agent_doc_hash::content_hash(&replayed_target),
        ),
    );
    Ok(true)
}

/// The shared derived fact. `preflight` and `session-check` both read this.
///
/// The verdict is derived in the **controller's** per-document graph whenever a
/// controller is reachable. That hop is the point: `preflight` and
/// `session-check` are separate short-lived processes, so deriving in-process
/// would give each its own private graph replayed from SQLite — which is the
/// divergence this exists to remove. The local path below is a fallback for an
/// actorless document (no controller), where there is no shared graph to join
/// and hydrate-then-derive is the honest best available.
///
/// `#retainedclearreactive`: on the controller path, reading this is also what
/// *clears* a `Satisfied` intent — the controller subscribes a per-document
/// settle effect to the same verdict slot, so there is no `settle_*` companion
/// for a consumer to forget. The actorless branch below has no shared graph to
/// subscribe in (`observe_retained_write_settlement` mints a fresh
/// `DocumentScope` per call, so an `Effect` there would be this same projection
/// wearing a costume), so it applies the clear directly and says so.
/// Join the controller's reactive settlement receipt with this caller's
/// durable observation.
///
/// The controller may settle an intent as a side effect of reading its
/// `Computed<SettlementVerdict>`. In that case the RPC response is the receipt
/// for the exact intent and converged content cut even when this short-lived
/// caller cannot reconstruct semantic payload evidence from the already
/// rebased document. Accept only that causally closed shape: same intent, and
/// the receipt's settled hash must equal both observed planes.
fn controller_settlement_receipt_matches(
    local: &SettlementVerdict,
    controller: &SettlementVerdict,
    observations: &agent_doc_controller_io::project_controller::RetainedWriteObservations,
) -> bool {
    let SettlementVerdict::Satisfied {
        intent_id,
        settled_hash,
        ..
    } = controller
    else {
        return false;
    };
    local.intent_id() == Some(intent_id.as_str())
        && observations.authority_hash.as_deref() == Some(settled_hash.as_str())
        && observations.disk_hash.as_deref() == Some(settled_hash.as_str())
}

pub fn retained_write_settlement(file: &Path, source: &str) -> SettlementVerdict {
    let settlement = observe_retained_write_settlement(file, source);
    let local_verdict = settlement.verdict();
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return settle_actorless_document(file, local_verdict, source);
    };
    let (authority, disk) = settlement.observations();
    let observations = agent_doc_controller_io::project_controller::RetainedWriteObservations {
        authority_payload_materialized: authority
            .as_ref()
            .is_some_and(|plane| plane.payload_materialized),
        authority_intent_delta_materialized: authority
            .as_ref()
            .is_some_and(|plane| plane.intent_delta_materialized),
        authority_hash: authority.map(|plane| plane.content_hash),
        disk_payload_materialized: disk
            .as_ref()
            .is_some_and(|plane| plane.payload_materialized),
        disk_intent_delta_materialized: disk
            .as_ref()
            .is_some_and(|plane| plane.intent_delta_materialized),
        disk_hash: disk.map(|plane| plane.content_hash),
        settlement_receipt: local_verdict
            .should_clear_intent()
            .then(|| local_verdict.clone()),
    };
    match agent_doc_controller_io::project_controller::retained_write_settlement(
        &project_root,
        file,
        &observations,
    ) {
        Ok(verdict) if verdict != local_verdict => {
            if controller_settlement_receipt_matches(&local_verdict, &verdict, &observations) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "retained_write_settlement_controller_receipt file={} source={} intent_id={} settled_hash={} action=use_reactive_settlement",
                        file.display(),
                        source,
                        verdict.intent_id().unwrap_or("none"),
                        observations.authority_hash.as_deref().unwrap_or("none"),
                    ),
                );
                verdict
            } else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "retained_write_settlement_controller_projection_lag file={} source={} controller_intent_id={} durable_intent_id={} authority_hash={} disk_hash={} controller_verdict={:?} durable_verdict={:?} action=use_durable_observation",
                        file.display(),
                        source,
                        verdict.intent_id().unwrap_or("none"),
                        local_verdict.intent_id().unwrap_or("none"),
                        observations.authority_hash.as_deref().unwrap_or("none"),
                        observations.disk_hash.as_deref().unwrap_or("none"),
                        verdict,
                        local_verdict,
                    ),
                );
                local_verdict
            }
        }
        Ok(verdict) => verdict,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_write_settlement_local_fallback file={} source={} reason={e}",
                    file.display(),
                    source,
                ),
            );
            settle_actorless_document(file, local_verdict, source)
        }
    }
}

/// Apply a `Satisfied` verdict's clear when there is no controller graph to
/// subscribe it in (`#retainedclearreactive`).
///
/// On the controller path the clear is an `Effect` gated on the verdict cell,
/// so no consumer has to remember it. An actorless document has no such graph —
/// each observation builds and drops its own `DocumentScope` — so the clear
/// stays a plain projection over the verdict this function was handed. It is
/// private on purpose: the failure this replaces was a *public* `settle_*`
/// companion that every consumer of the verdict had to call.
///
/// Idempotent by construction: every verdict other than `Satisfied` returns
/// unchanged, so calling it twice clears nothing twice.
fn settle_actorless_document(
    file: &Path,
    verdict: SettlementVerdict,
    source: &str,
) -> SettlementVerdict {
    let SettlementVerdict::Satisfied {
        intent_id,
        retained_target_hash,
        settled_hash,
        proof,
        ..
    } = &verdict
    else {
        return verdict;
    };
    if let Err(e) = clear_deferred_document_write_intent(file, retained_target_hash, source) {
        eprintln!(
            "[agent-doc] actorless retained-write settlement failed for {}: {e}",
            file.display()
        );
        return verdict;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "retained_write_settled_from_derived_verdict file={} intent_id={intent_id} retained_target_hash={retained_target_hash} settled_hash={settled_hash} proof={} source={source} plane=actorless_local",
            file.display(),
            proof.token(),
        ),
    );
    verdict
}

/// True when a retained write genuinely blocks opening a new cycle.
///
/// Replaces `pending_document_write(file).is_some()` at the preflight gate: an
/// intent that the converged document has already satisfied — and one whose
/// planes could not be observed — are both *not* outstanding writes.
pub fn retained_write_blocks_new_cycle(file: &Path, source: &str) -> bool {
    let verdict = retained_write_settlement(file, source);
    // A gate that refuses must say why. Without this the operator sees only
    // "retained document-write effect remains unsettled" while `session-check`
    // reports ok, and the cause — which of the two `UnsettledCause`s, and for
    // which intent — is nowhere on disk. That is what turns a diagnosable
    // refusal into a poll loop (`#closeoutwaitchurn`).
    if let agent_doc_state_backbone::retained_write::SettlementVerdict::Unsettled {
        intent_id,
        cause,
    } = &verdict
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "retained_write_blocks_new_cycle file={} source={} intent_id={} cause={}",
                file.display(),
                source,
                intent_id,
                cause.token(),
            ),
        );
    }
    verdict.blocks_new_cycle()
}

pub fn retained_write_blocks_session_closeout(file: &Path, source: &str) -> bool {
    let verdict = retained_write_settlement(file, source);
    if matches!(
        &verdict,
        agent_doc_state_backbone::retained_write::SettlementVerdict::Unobserved { .. }
            | agent_doc_state_backbone::retained_write::SettlementVerdict::Unsettled { .. }
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "retained_write_blocks_session_closeout file={} source={} intent_id={} verdict={:?}",
                file.display(),
                source,
                verdict.intent_id().unwrap_or("none"),
                verdict,
            ),
        );
    }
    verdict.blocks_session_closeout()
}

// `#retainedclearreactive`: there is deliberately no public
// `settle_retained_write_through_derived_verdict` here any more. It was the
// imperative form `#idlerevisionreactive` names — "a side effect that must be
// *called* at the right moment" — and `#preflightsettleparity` had already
// proven the failure mode by fixing "preflight forgot to call the clear" with a
// *second* call site. A third consumer of the verdict would have reintroduced
// it. The clear is now an `Effect` in the controller's per-document graph gated
// on the same verdict cell every consumer reads
// (`ControllerDocumentGraphs::ensure_settle_effect`), with
// [`settle_actorless_document`] above as the private no-controller fallback.

#[cfg(test)]
mod tests {
    use super::*;

    fn satisfied_settlement(intent_id: &str, settled_hash: &str) -> SettlementVerdict {
        SettlementVerdict::Satisfied {
            intent_id: intent_id.to_string(),
            retained_target_hash: "retained-target".to_string(),
            settled_hash: settled_hash.to_string(),
            proof:
                agent_doc_state_backbone::retained_write::SatisfiedProof::RebasedPayloadMaterialized,
            intent_source:
                agent_doc_state_backbone::write_source::DocumentWriteSource::PendingWrite,
        }
    }

    #[test]
    fn reactive_settlement_receipt_joins_same_intent_on_exact_durable_cut() {
        let local = SettlementVerdict::Unsettled {
            intent_id: "intent-1".to_string(),
            cause: agent_doc_state_backbone::retained_write::UnsettledCause::PayloadAbsentFromConvergedContent,
        };
        let controller = satisfied_settlement("intent-1", "settled-cut");
        let observations = agent_doc_controller_io::project_controller::RetainedWriteObservations {
            authority_hash: Some("settled-cut".to_string()),
            disk_hash: Some("settled-cut".to_string()),
            ..Default::default()
        };

        assert!(controller_settlement_receipt_matches(
            &local,
            &controller,
            &observations,
        ));
    }

    #[test]
    fn reactive_settlement_receipt_rejects_other_intent_or_content_cut() {
        let local = SettlementVerdict::Unsettled {
            intent_id: "intent-1".to_string(),
            cause: agent_doc_state_backbone::retained_write::UnsettledCause::PayloadAbsentFromConvergedContent,
        };
        let observations = agent_doc_controller_io::project_controller::RetainedWriteObservations {
            authority_hash: Some("settled-cut".to_string()),
            disk_hash: Some("settled-cut".to_string()),
            ..Default::default()
        };

        assert!(!controller_settlement_receipt_matches(
            &local,
            &satisfied_settlement("intent-2", "settled-cut"),
            &observations,
        ));
        assert!(!controller_settlement_receipt_matches(
            &local,
            &satisfied_settlement("intent-1", "other-cut"),
            &observations,
        ));
    }

    #[test]
    fn atomic_repair_projection_classifies_late_editor_attachment() {
        assert_eq!(
            classify_atomic_repair_projection("target", "before", "target", "target"),
            AtomicRepairProjectionState::Converged,
        );
        assert_eq!(
            classify_atomic_repair_projection("target", "before", "before", "target"),
            AtomicRepairProjectionState::LateEditorAttachment,
        );
        assert_eq!(
            classify_atomic_repair_projection("target", "before", "operator edit", "target"),
            AtomicRepairProjectionState::Diverged,
            "a newer editor cut must never be treated as the late pre-repair authority",
        );
        assert_eq!(
            classify_atomic_repair_projection("target", "before", "before", "before"),
            AtomicRepairProjectionState::Diverged,
            "the grace path requires proof that the repair already reached disk",
        );
    }

    /// `#dedupepresettle`: an in-flight projection samples as `Diverged`, and
    /// returning on that first sample turned a repair that converged
    /// milliseconds later into a terminal
    /// `did not converge exactly before settling deferred lineage` failure.
    #[test]
    fn atomic_repair_projection_awaits_a_transiently_diverged_sample() {
        let samples = std::cell::RefCell::new(vec![
            ("stale".to_string(), "stale".to_string()),
            ("target".to_string(), "target".to_string()),
        ]);
        let (canonical, disk) = await_atomic_repair_projection_samples(
            "target",
            "before",
            std::time::Duration::from_millis(ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS),
            // The first read after the write: neither plane has caught up, so
            // this classifies as `Diverged`, not `LateEditorAttachment`.
            "stale".to_string(),
            "stale".to_string(),
            || Ok(samples.borrow_mut().remove(0)),
        )
        .unwrap();

        assert_eq!(canonical, "target");
        assert_eq!(disk, "target");
    }

    #[test]
    fn atomic_repair_projection_still_awaits_late_editor_attachment() {
        let samples = std::cell::RefCell::new(vec![("target".to_string(), "target".to_string())]);
        let (canonical, disk) = await_atomic_repair_projection_samples(
            "target",
            "before",
            std::time::Duration::from_millis(ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS),
            "before".to_string(),
            "target".to_string(),
            || Ok(samples.borrow_mut().remove(0)),
        )
        .unwrap();

        assert_eq!(canonical, "target");
        assert_eq!(disk, "target");
    }

    /// The settle window must not become a spin: a converged entry sample
    /// returns without sampling at all.
    #[test]
    fn atomic_repair_projection_returns_converged_without_sampling() {
        let sampled = std::cell::Cell::new(0u32);
        let (canonical, disk) = await_atomic_repair_projection_samples(
            "target",
            "before",
            std::time::Duration::from_millis(ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS),
            "target".to_string(),
            "target".to_string(),
            || {
                sampled.set(sampled.get() + 1);
                Ok(("target".to_string(), "target".to_string()))
            },
        )
        .unwrap();

        assert_eq!(canonical, "target");
        assert_eq!(disk, "target");
        assert_eq!(sampled.get(), 0);
    }

    /// A projection that never converges must still reach the existing typed
    /// terminal classification — the window bounds the wait, it does not
    /// swallow the divergence.
    #[test]
    fn atomic_repair_projection_returns_the_last_sample_after_the_window() {
        let (canonical, disk) = await_atomic_repair_projection_samples(
            "target",
            "before",
            std::time::Duration::from_millis(ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS),
            "stale".to_string(),
            "stale".to_string(),
            || Ok(("operator edit".to_string(), "stale".to_string())),
        )
        .unwrap();

        assert_eq!(
            classify_atomic_repair_projection("target", "before", &canonical, &disk),
            AtomicRepairProjectionState::Diverged,
        );
    }

    #[test]
    fn retained_write_cycle_boundaries_own_closed_set_source_flags() {
        let cases = [
            (
                RetainedWriteCycleBoundary::Preflight,
                "preflight_retained_write_recovery",
                "preflight_retained_document_write_gate",
            ),
            (
                RetainedWriteCycleBoundary::SessionCheck,
                "session_check_retained_write_recovery",
                "session_check_retained_write_gate",
            ),
            (
                RetainedWriteCycleBoundary::FinalizePreCapture,
                "finalize_pre_capture_retained_write_recovery",
                "finalize_pre_capture_retained_write_gate",
            ),
        ];
        for (boundary, recovery, gate) in cases {
            assert_eq!(boundary.recovery_source(), recovery);
            assert_eq!(boundary.gate_source(), gate);
            assert!(
                boundary
                    .recovered_settlement_source()
                    .contains("settlement")
            );
        }
    }

    /// `#crdtprojectionprofile`: the accumulator must charge time to the state an
    /// iteration *ended* in, and must aggregate repeat visits to one state.
    ///
    /// State assignments are scattered through the convergence loop body, so the
    /// only accounting that cannot silently miss one is charging on exit at the
    /// loop head. This pins that: two separate stretches in the same state sum
    /// into a single entry rather than appearing twice, and zero-duration states
    /// are omitted so the breakdown stays readable.
    #[test]
    fn convergence_profile_charges_each_iteration_to_the_state_it_ended_in() {
        let mut profile = CrdtConvergenceProfile::new(CrdtConvergenceState::TypingQuiescence);

        // Two non-adjacent stretches in DeliveryProjectionPending must aggregate.
        profile.tick(CrdtConvergenceState::DeliveryProjectionPending);
        std::thread::sleep(std::time::Duration::from_millis(12));
        profile.tick(CrdtConvergenceState::CompareAndSwapRaced);
        std::thread::sleep(std::time::Duration::from_millis(4));
        profile.tick(CrdtConvergenceState::DeliveryProjectionPending);
        std::thread::sleep(std::time::Duration::from_millis(12));

        let rendered = profile.render(CrdtConvergenceState::DeliveryProjectionPending);

        assert_eq!(
            rendered.matches("delivery_projection_pending=").count(),
            1,
            "repeat visits to one state must aggregate into a single entry: {rendered}"
        );
        assert!(
            rendered.contains("compare_and_swap_raced="),
            "a state that was actually occupied must appear: {rendered}"
        );
        assert!(
            !rendered.contains("editor_sync_pending="),
            "a state never entered must not appear: {rendered}"
        );
        // Largest first, so the dominant cost reads off the front.
        assert!(
            rendered.starts_with("delivery_projection_pending="),
            "the breakdown must be ordered by cost: {rendered}"
        );
    }

    #[test]
    fn delivery_subscription_time_does_not_consume_controller_retry_budget() {
        let total_elapsed_ms = CRDT_WRITE_CONVERGENCE_TIMEOUT_MS
            .saturating_add(CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS)
            .saturating_sub(500);
        assert_eq!(
            exclusive_controller_elapsed_ms(
                total_elapsed_ms,
                CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS,
            ),
            CRDT_WRITE_CONVERGENCE_TIMEOUT_MS - 500
        );
        assert_eq!(exclusive_controller_elapsed_ms(4_000, 8_000), 0);

        let mut controller_deadline = DeadlineCore::new(CRDT_WRITE_CONVERGENCE_TIMEOUT_MS);
        controller_deadline.tick(exclusive_controller_elapsed_ms(
            total_elapsed_ms,
            CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS,
        ));
        assert!(
            !controller_deadline.is_expired(),
            "the delivery subscription owns its recovery budget; only controller/frontier time \
             may expire the controller deadline"
        );
    }

    #[test]
    fn delivery_fallback_backoff_is_independent_from_frontier_backoff() {
        let frontier_backoff_ms = CRDT_WRITE_BACKOFF_MAX_MS;
        let mut projection_observation = ProjectionObservationState::default();

        assert_eq!(
            projection_observation.next_fallback_sleep_ms(1_000),
            CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS
        );
        assert_eq!(
            projection_observation.next_fallback_sleep_ms(1_000),
            CRDT_PROJECTION_FALLBACK_BACKOFF_POLICY
                .next_ms(CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS, false)
        );
        assert_eq!(
            frontier_backoff_ms, CRDT_WRITE_BACKOFF_MAX_MS,
            "delivery fallback must not reset or advance the controller/CAS frontier backoff"
        );

        projection_observation.reset();
        assert_eq!(
            projection_observation.next_fallback_sleep_ms(1_000),
            CRDT_PROJECTION_FALLBACK_BACKOFF_INITIAL_MS,
            "a new delivery frontier starts its own fallback schedule at the floor"
        );
    }

    #[test]
    fn controller_document_mutation_scope_is_nested_and_restored() {
        assert!(!controller_document_mutation_in_progress());
        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
        with_controller_document_mutation(|| {
            assert!(controller_document_mutation_in_progress());
            assert!(agent_doc_document_realtime::write_authority::within_owner_scope());
            with_controller_document_mutation(|| {
                assert!(controller_document_mutation_in_progress());
                assert!(agent_doc_document_realtime::write_authority::within_owner_scope());
            });
        });
        assert!(!controller_document_mutation_in_progress());
        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
    }

    fn witness(
        live: bool,
        registrations: &[(agent_doc_reliable_sync_io::liveness::Pid, &str, u64)],
    ) -> EditorReplicaLivenessWitness {
        EditorReplicaLivenessWitness {
            live,
            registrations: registrations
                .iter()
                .map(|(pid, id, ts)| (*pid, (*id).to_string(), *ts))
                .collect(),
        }
    }

    /// `#missingreplicarebuild` — the terminal rebuild is claimable exactly once
    /// per liveness witness.
    ///
    /// This latch is the load-bearing part of the fix, not an optimization.
    /// Missing-replica was excluded from the Tier 1 rebuild because paying it at
    /// every read site measured ~28s per site, and a single compact/commit
    /// performs several resolutions. Without the latch, re-enabling the rebuild
    /// here would reintroduce exactly that regression — so the property under
    /// test is that repeated resolutions at an UNCHANGED witness claim nothing,
    /// while a genuine registration change re-arms it.
    #[test]
    fn terminal_missing_replica_rebuild_is_claimed_once_per_liveness_witness() {
        let file = std::path::Path::new("/tmp/agent-doc-terminal-rebuild-claim.md");
        clear_terminal_missing_replica_rebuild(file);
        let observed = witness(true, &[(4242, "jetbrains-a", 1_000)]);

        assert!(
            claim_terminal_missing_replica_rebuild(file, &observed),
            "the first resolution at a new witness must get the one attempt"
        );
        for _ in 0..5 {
            assert!(
                !claim_terminal_missing_replica_rebuild(file, &observed),
                "later resolutions in the same compact/commit must NOT re-pay the \
                 rebuild while nothing has changed"
            );
        }

        // A re-registration is real evidence the replica may be back, so the
        // attempt re-arms — no timer involved.
        let rearmed = witness(true, &[(4242, "jetbrains-a", 2_000)]);
        assert!(
            claim_terminal_missing_replica_rebuild(file, &rearmed),
            "a changed registration witness must re-arm the terminal rebuild"
        );

        // The two memos are independent: claiming the terminal rebuild must not
        // suppress the upstream self-heal retry loop.
        clear_editor_replica_self_heal_exhausted(file);
        assert!(
            !should_pause_editor_replica_self_heal(file, &rearmed),
            "the terminal-rebuild latch must not leak into the upstream self-heal memo"
        );
        clear_terminal_missing_replica_rebuild(file);
    }

    /// The self-heal memo suppresses the retry loop only while the realtime
    /// liveness witness is unchanged — the burst-collapse the TTL used to buy.
    #[test]
    fn self_heal_memo_suppresses_retry_while_liveness_witness_is_unchanged() {
        let file = std::path::Path::new("/tmp/agent-doc-self-heal-unchanged.md");
        clear_editor_replica_self_heal_exhausted(file);
        let observed = witness(true, &[(4242, "jetbrains-a", 1_000)]);

        assert!(
            !should_pause_editor_replica_self_heal(file, &observed),
            "a file with no recorded exhaustion must be allowed a full retry"
        );

        record_editor_replica_self_heal_exhausted(file, observed.clone());
        assert!(
            should_pause_editor_replica_self_heal(file, &observed),
            "an unchanged liveness witness must keep suppressing the futile retry loop"
        );

        clear_editor_replica_self_heal_exhausted(file);
    }

    /// The realtime replacement for the TTL: a changed liveness witness re-arms
    /// the self-heal IMMEDIATELY, with no timer to wait out. Each shape below
    /// would have stayed suppressed for the remainder of a 10s TTL window.
    #[test]
    fn self_heal_memo_rearms_immediately_when_liveness_witness_changes() {
        let file = std::path::Path::new("/tmp/agent-doc-self-heal-changed.md");
        let exhausted_at = witness(false, &[]);

        for (label, current) in [
            (
                "an editor registration appearing",
                witness(false, &[(4242, "jetbrains-a", 1_000)]),
            ),
            ("the document going live", witness(true, &[])),
        ] {
            clear_editor_replica_self_heal_exhausted(file);
            record_editor_replica_self_heal_exhausted(file, exhausted_at.clone());
            assert!(
                !should_pause_editor_replica_self_heal(file, &current),
                "{label} must re-arm the self-heal immediately, not wait out a TTL"
            );
        }

        // A re-registration by the SAME editor pid is a new registration
        // timestamp, which is exactly the "replica came back" signal.
        let stale = witness(true, &[(4242, "jetbrains-a", 1_000)]);
        let reregistered = witness(true, &[(4242, "jetbrains-a", 2_000)]);
        clear_editor_replica_self_heal_exhausted(file);
        record_editor_replica_self_heal_exhausted(file, stale);
        assert!(
            !should_pause_editor_replica_self_heal(file, &reregistered),
            "a fresh registration timestamp for the same pid must re-arm the self-heal"
        );

        clear_editor_replica_self_heal_exhausted(file);
    }

    /// Pids keep the plane's native `u64` width. Narrowing them to `u32` with a
    /// saturating conversion would map both editors below onto `u32::MAX`,
    /// making two distinct registrations compare equal and suppressing a
    /// self-heal that must re-arm.
    #[test]
    fn self_heal_memo_distinguishes_pids_beyond_u32() {
        let file = std::path::Path::new("/tmp/agent-doc-self-heal-wide-pid.md");
        let exhausted_at = witness(true, &[(u64::from(u32::MAX) + 1, "jetbrains-a", 1_000)]);
        let different_editor = witness(true, &[(u64::from(u32::MAX) + 2, "jetbrains-a", 1_000)]);

        clear_editor_replica_self_heal_exhausted(file);
        record_editor_replica_self_heal_exhausted(file, exhausted_at);
        assert!(
            !should_pause_editor_replica_self_heal(file, &different_editor),
            "distinct pids above u32::MAX must not collapse into one witness"
        );

        clear_editor_replica_self_heal_exhausted(file);
    }

    /// A healthy observation drops the memo outright, so the next failure gets a
    /// full retry rather than inheriting a stale suppression.
    #[test]
    fn self_heal_memo_is_cleared_by_a_healthy_observation() {
        let file = std::path::Path::new("/tmp/agent-doc-self-heal-cleared.md");
        let observed = witness(true, &[(4242, "jetbrains-a", 1_000)]);
        record_editor_replica_self_heal_exhausted(file, observed.clone());
        clear_editor_replica_self_heal_exhausted(file);
        assert!(
            !should_pause_editor_replica_self_heal(file, &observed),
            "clearing the memo must restore a full retry for the same witness"
        );
    }

    /// `#idlewatchctrlbackoff` — recording controller degradation must flip
    /// [`controller_failed_within`] true so the idle-queue watch backs off.
    #[test]
    fn controller_failed_within_is_true_after_degradation_recorded() {
        record_controller_degraded();
        assert!(
            controller_failed_within(std::time::Duration::from_secs(60)),
            "controller_failed_within must report true right after a degradation was recorded"
        );
    }

    #[test]
    fn controller_transport_congestion_is_retryable_but_semantic_errors_are_not() {
        assert!(transient_convergence_backpressure_error(&anyhow::anyhow!(
            "timed out after 0.8s waiting for project controller response"
        )));
        assert!(transient_convergence_backpressure_error(&anyhow::anyhow!(
            "connection reset by peer"
        )));
        assert!(transient_convergence_backpressure_error(&anyhow::anyhow!(
            "document model publish suppressed; reason=in_progress; retry after the active recovery attempt finishes"
        )));
        assert!(!transient_convergence_backpressure_error(&anyhow::anyhow!(
            "malformed template patchback"
        )));
    }

    #[test]
    fn transient_embedded_relay_observations_require_controller_refresh() {
        assert!(!embedded_relay_observation_is_current(
            &agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        ));
        assert!(!embedded_relay_observation_is_current(
            &agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
        ));
        assert!(!embedded_relay_observation_is_current(
            &agent_doc_crdt_relay_io::CurrentText::Detached
        ));
        assert!(embedded_relay_observation_is_current(
            &agent_doc_crdt_relay_io::CurrentText::Current {
                text: "controller handoff".to_string(),
                live_editors: 1,
                delivery_converged: true,
                delivery_version: 7,
                semantics: None,
            }
        ));
    }

    // ── Rung 2 (`#rtwfeed`) CP-owned CRDT feed ──

    /// Build a temp project with `.agent-doc/` and the document on disk. Returns
    /// the `TempDir` (keep alive), the file `PathBuf`, and the canonical path
    /// string used by both the Lazily liveness plane and CRDT relay.
    fn temp_doc(disk: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, disk).unwrap();
        agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&file).unwrap();
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    #[test]
    fn invalid_agent_projection_reconstructs_operator_cut_from_durable_ops() {
        let base = concat!(
            "---\nqueue: go\n---\n\n",
            "<!-- agent:queue go -->\n- work\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior\n\nDone.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, file, _) = temp_doc(base);
        let go_offset = base.find("queue: go").unwrap() + "queue: ".len();
        let base_hash = agent_doc_hash::content_hash(base);
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: go_offset,
                len: 2,
            },
        )
        .unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: go_offset,
                text: "stop".to_string(),
            },
        )
        .unwrap();
        let operator_cut = base.replacen("queue: go", "queue: stop", 1);
        let poisoned = format!(
            "{operator_cut}\n<!-- agent:exchange -->\n### Re: duplicated agent projection\n\nDuplicate.\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n"
        );
        let duplicate_boundary = operator_cut.replacen(
            "<!-- /agent:exchange -->",
            "### Re: duplicated tail\n\nDuplicate.\n<!-- agent:boundary:def456 -->\n<!-- /agent:exchange -->",
            1,
        );
        let unclosed_exchange = operator_cut.replacen("<!-- /agent:exchange -->\n", "", 1);
        let orphan_comment_terminator =
            operator_cut.replacen("<!-- /agent:queue -->", "<!-- /agent:queue -->\n-->", 1);

        assert!(!agent_projection_integrity_valid(&poisoned));
        assert!(!agent_projection_integrity_valid(&duplicate_boundary));
        assert!(!agent_projection_integrity_valid(&unclosed_exchange));
        assert!(!canonical_document_target_is_valid(
            &orphan_comment_terminator
        ));
        let recovered =
            editor_operator_cut_for_agent_rebase(&file, base, &poisoned, "test_reconnect");
        assert_eq!(recovered, operator_cut);
        assert!(recovered.contains("queue: stop"));
        assert_eq!(recovered.matches("agent:boundary:").count(), 1);
        assert_eq!(
            editor_operator_cut_for_agent_rebase(&file, base, &unclosed_exchange, "test_reconnect"),
            operator_cut
        );
        assert_eq!(
            editor_operator_cut_for_agent_rebase(
                &file,
                base,
                &orphan_comment_terminator,
                "test_reconnect",
            ),
            operator_cut
        );
    }

    #[test]
    fn detached_base_replays_pending_operator_queue_deletion() {
        let base = concat!(
            "---\nqueue: go\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#next]\n",
            "- ~~do [#old-a]~~\n",
            "- ~~do [#old-b]~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior\n\nDone.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, file, _) = temp_doc(base);
        let deleted = "- ~~do [#old-a]~~\n- ~~do [#old-b]~~\n";
        let offset = base.find(deleted).unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(base),
            agent_doc_merge::crdt::EditorOp::Delete {
                offset,
                len: deleted.len(),
            },
        )
        .unwrap();

        let reconciled =
            reconcile_pending_editor_cut(&file, base, base, "paused_closeout").unwrap();

        assert!(reconciled.replayed_editor_ops);
        assert!(reconciled.content.contains("- do [#next]"));
        assert!(!reconciled.content.contains("#old-a"));
        assert!(!reconciled.content.contains("#old-b"));
        assert!(
            agent_doc_op_capture_io::has_pending_editor_ops(&file),
            "the op epoch stays durable until the caller completes its write"
        );
    }

    #[test]
    fn response_replay_boundary_duplication_is_recoverable_before_integrity_gate() {
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "<!-- agent:boundary:stale -->\n",
            "### Re: retained — gpt-5\n\nRetained response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let normalized = normalize_recoverable_response_replay_duplication(duplicated)
            .expect("duplicate response-replay boundary should be recoverable");

        assert!(agent_projection_integrity_valid(&normalized));
        assert_eq!(normalized.matches("agent:boundary:").count(), 1);
        assert!(normalized.contains("agent:boundary:latest"));
        assert!(normalized.contains("❯ operator prompt"));
        assert!(normalized.contains("Retained response."));
    }

    #[test]
    fn stranded_duplicate_response_heading_is_recoverable_before_integrity_gate() {
        let interrupted = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ original prompt\n\n",
            "### Re: do [#recommendedungatereview-0xc5] — gpt-5\n\n",
            "Original response body.\n\n",
            "### Re: another topic — gpt-5\n\n",
            "Another response body.\n\n",
            "### Re: do [#recommendedungatereview-0xc5] — gpt-5\n\n",
            "### Re: latest topic — gpt-5\n\n",
            "Latest response body.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let shell_repaired =
            agent_doc_turn::response_replay::repair_stranded_duplicate_response_headings(
                interrupted,
            );
        assert_ne!(
            shell_repaired, interrupted,
            "the shared response-shell canonicalizer must recognize this replay shape"
        );
        assert!(
            agent_projection_integrity_valid(&shell_repaired),
            "the response-shell repair must restore projection integrity"
        );

        let normalized = normalize_recoverable_response_replay_duplication(interrupted)
            .expect("the proven empty duplicate response shell should be recoverable");

        assert!(agent_projection_integrity_valid(&normalized));
        assert_eq!(
            normalized
                .matches("### Re: do [#recommendedungatereview-0xc5] — gpt-5")
                .count(),
            1
        );
        assert!(normalized.contains("Original response body."));
        assert!(normalized.contains("Another response body."));
        assert!(normalized.contains("Latest response body."));
    }

    #[test]
    fn exact_doubled_projection_retires_only_redundant_deferred_lineage() {
        let trusted = concat!(
            "---\nagent_doc_session: doubled-retirement\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:trusted -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (dir, file, _) = temp_doc(trusted);
        let canonical = file.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        for index in 0..4 {
            let event = agent_doc_state_backbone::StateEvent::new(
                format!("legacy-delivery-failed-to-all-{index}"),
                agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                    document_hash: document_hash.clone(),
                    intent_id: format!("legacy-intent-{index}"),
                    expected_hash: agent_doc_hash::content_hash(trusted),
                    expected_content: Some(trusted.to_string()),
                    target_hash: agent_doc_hash::content_hash(trusted),
                    target_content: trusted.to_string(),
                    source: agent_doc_state_backbone::DocumentWriteSource::from(
                        "legacy_delivery_failed_to_all",
                    ),
                    reason: DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
                },
            );
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                dir.path(),
                &event,
            )
            .unwrap();
        }
        assert_eq!(pending_document_write_journal(&file).len(), 4);
        let doubled = format!("{trusted}{trusted}");

        assert_eq!(
            retire_redundant_doubled_document_write_intents(
                &file,
                &doubled,
                trusted,
                "test_doubled_retirement",
            )
            .unwrap(),
            4,
        );
        assert!(pending_document_write_journal(&file).is_empty());
    }

    #[test]
    fn doubled_projection_retirement_preserves_intent_with_unique_text() {
        let trusted = concat!(
            "---\nagent_doc_session: doubled-retirement-refusal\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:trusted -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let target_with_unique_text = trusted.replace(
            "❯ operator prompt",
            "❯ operator prompt\n\nUnique retained response.",
        );
        let (_dir, file, _) = temp_doc(trusted);
        retain_deferred_document_write_target(
            &file,
            trusted,
            &target_with_unique_text,
            "text_bearing_retained_intent",
            DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
        )
        .unwrap();
        let doubled = format!("{trusted}{trusted}");

        let error = retire_redundant_doubled_document_write_intents(
            &file,
            &doubled,
            trusted,
            "test_doubled_retirement_refusal",
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("content outside the trusted target")
        );
        assert_eq!(pending_document_write_journal(&file).len(), 1);
    }

    #[test]
    fn structurally_invalid_document_target_is_rejected_before_retention() {
        let trusted = concat!(
            "---\nagent_doc_session: invalid-retention\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:trusted -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, file, _) = temp_doc(trusted);
        let doubled = format!("{trusted}{trusted}");

        assert!(
            retain_deferred_document_write_target(
                &file,
                trusted,
                &doubled,
                "invalid_retention_test",
                DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
            )
            .is_err()
        );
        assert!(pending_document_write_journal(&file).is_empty());
    }

    #[test]
    fn concatenated_logical_generations_rebase_pending_target_over_operator_cut() {
        let base = concat!(
            "---\nagent_doc_session: sim-refresh\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ original prompt\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- manually-delete-me\n",
            "<!-- /agent:queue -->\n",
        );
        let target = base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: retained — gpt-5\n\nRetained answer.\n<!-- agent:boundary:target -->",
        );
        let operator_cut = base
            .replace(
                "<!-- agent:boundary:base -->",
                "❯ prompt added during refresh\n<!-- agent:boundary:base -->",
            )
            .replace("- manually-delete-me\n", "");
        let (_dir, file, _) = temp_doc(base);

        for duplicated in [
            format!("{target}{operator_cut}"),
            format!("{operator_cut}{target}"),
        ] {
            assert!(!agent_projection_integrity_valid(&duplicated));
            let recovered = recover_concatenated_document_generations(
                &file,
                &duplicated,
                base,
                &target,
                "test_refresh_recovery",
            )
            .unwrap()
            .expect("one branch is the exact pending target");
            assert!(agent_projection_integrity_valid(&recovered));
            assert_eq!(
                recovered.matches("agent_doc_session: sim-refresh").count(),
                1
            );
            assert_eq!(recovered.matches("agent:boundary:").count(), 1);
            assert_eq!(recovered.matches("Retained answer.").count(), 1);
            assert_eq!(recovered.matches("prompt added during refresh").count(), 1);
            assert!(!recovered.contains("manually-delete-me"));
        }
    }

    #[test]
    fn concatenated_generation_recovery_requires_exact_pending_branch() {
        let base = concat!(
            "---\nagent_doc_session: sim-refresh\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ original prompt\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let target = base.replace(
            "original prompt",
            "original prompt\n\n### Re: retained\nAnswer",
        );
        let unrelated = base.replace("original prompt", "different operator document");
        let (_dir, file, _) = temp_doc(base);

        let duplicated_without_target = format!("{unrelated}{base}");
        assert!(
            recover_concatenated_document_generations(
                &file,
                &duplicated_without_target,
                base,
                &target,
                "test_refresh_recovery",
            )
            .unwrap()
            .is_none()
        );

        let exact_retry = format!("{target}{target}");
        assert_eq!(
            recover_concatenated_document_generations(
                &file,
                &exact_retry,
                base,
                &target,
                "test_refresh_recovery",
            )
            .unwrap()
            .as_deref(),
            Some(target.as_str())
        );
    }

    #[test]
    fn standalone_boundary_fallback_keeps_latest_frontier_and_operator_cut() {
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Earlier exchange.\n",
            "<!-- agent:boundary:stale -->\n",
            "❯ operator prompt after the old frontier\n",
            "### Re: retained — gpt-5\n\nRetained response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- keep-this-item\n",
            "<!-- /agent:queue -->\n",
        );

        let normalized = remove_stale_standalone_exchange_boundary(duplicated)
            .expect("exactly two exchange frontiers should retain the latest");

        assert!(agent_projection_integrity_valid(&normalized));
        assert!(!normalized.contains("agent:boundary:stale"));
        assert!(normalized.contains("agent:boundary:latest"));
        assert!(normalized.contains("❯ operator prompt after the old frontier"));
        assert!(normalized.contains("Retained response."));
        assert!(normalized.contains("- keep-this-item"));
        assert!(!normalized.contains("deleted-queue-item"));
    }

    #[test]
    fn boundary_fallback_fails_closed_for_ambiguous_or_example_markers() {
        let three = concat!(
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:first -->\n",
            "<!-- agent:boundary:second -->\n",
            "<!-- agent:boundary:third -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let example = concat!(
            "<!-- agent:exchange -->\n",
            "```md\n",
            "<!-- agent:boundary:example -->\n",
            "```\n",
            "<!-- agent:boundary:real -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(remove_stale_standalone_exchange_boundary(three).is_none());
        assert!(remove_stale_standalone_exchange_boundary(example).is_none());
        assert!(agent_projection_integrity_valid(example));
    }

    #[test]
    fn unmatched_duplicate_done_close_is_repaired_without_touching_operator_cut() {
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Earlier exchange.\n",
            "<!-- agent:boundary:current -->\n",
            "❯ Please write this prompt after the boundary.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- keep-this-item\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:done -->\n",
            "- completed item\n",
            "<!-- /agent:done -->\n",
            "<!-- /agent:done -->\n",
        );
        let expected = duplicated.replacen(
            "<!-- /agent:done -->\n<!-- /agent:done -->\n",
            "<!-- /agent:done -->\n",
            1,
        );

        let normalized = normalize_recoverable_response_replay_duplication(duplicated)
            .expect("one replay-duplicated done close should be recoverable");

        assert_eq!(normalized, expected);
        assert!(agent_projection_integrity_valid(&normalized));
        assert!(normalized.contains("❯ Please write this prompt after the boundary."));
        assert!(normalized.contains("- keep-this-item"));
        assert!(!normalized.contains("deleted-queue-item"));
    }

    #[test]
    fn unmatched_operator_authored_close_without_duplicate_evidence_is_not_repaired() {
        let malformed = concat!(
            "<!-- agent:exchange -->\n",
            "Operator prompt.\n",
            "<!-- agent:boundary:current -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- /agent:done -->\n",
        );

        assert!(normalize_recoverable_response_replay_duplication(malformed).is_none());
    }

    #[test]
    fn marker_examples_in_code_do_not_supply_duplicate_close_evidence() {
        let malformed = concat!(
            "<!-- agent:exchange -->\n",
            "```md\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "```\n",
            "<!-- agent:boundary:current -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- /agent:done -->\n",
        );

        assert!(normalize_recoverable_response_replay_duplication(malformed).is_none());
    }

    #[test]
    fn malformed_canonical_target_is_rejected_before_relay_mutation() {
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "Question.\n",
            "<!-- /agent:exchange -->\n",
        );
        let malformed = "<!-- agent:exchange -->\nQuestion.\n";
        let (_dir, file, _) = temp_doc(baseline);

        let err = apply_cp_write_through_relay_authority(
            &file,
            baseline,
            malformed,
            "test_malformed_target",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("structurally invalid canonical target")
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
    }

    #[test]
    fn malformed_post_apply_editor_cut_is_not_rebased_over_valid_retained_target() {
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "Question.\n",
            "<!-- /agent:exchange -->\n",
        );
        let retained_target = concat!(
            "<!-- agent:exchange -->\n",
            "Question.\n\n",
            "*Compacted.*\n",
            "<!-- /agent:exchange -->\n",
        );
        let transient_editor_cut = concat!(
            "Question.\n\n",
            "*Compacted.*\n",
            "<!-- /agent:exchange -->\n",
        );

        validate_canonical_document_target(
            Path::new("session.md"),
            retained_target,
            "test_valid_retained_target",
        )
        .expect("the retained compact target is valid");
        let reason =
            structurally_invalid_post_apply_editor_cut(retained_target, transient_editor_cut)
                .expect("the replacement replica cut must be classified as transient corruption");

        assert!(reason.contains("component 'exchange'"));
        assert!(reason.contains("without matching open"));
        assert_eq!(
            structurally_invalid_post_apply_editor_cut(retained_target, retained_target),
            None,
        );
        assert_eq!(
            structurally_invalid_post_apply_editor_cut(baseline, retained_target),
            None,
            "a different but structurally valid editor cut remains eligible for semantic rebase",
        );
    }

    fn push_test_liveness(
        _file: &std::path::Path,
        _document_hash: &str,
        ops: &[agent_doc_reliable_sync_io::liveness::LivenessOp],
    ) {
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(ops);
    }

    fn seed_reliable_sync_open(file: &std::path::Path, tag: &str) {
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        push_test_liveness(
            &canonical,
            &document_hash,
            &[
                agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                    document_hash: document_hash.clone(),
                    pid: std::process::id().into(),
                    tag: tag.to_string(),
                },
                agent_doc_reliable_sync_io::liveness::LivenessOp::Register(
                    agent_doc_reliable_sync_io::liveness::EditorRegistration {
                        document_hash: document_hash.clone(),
                        pid: std::process::id().into(),
                        path: canonical.to_string_lossy().into_owned(),
                        editor_id: tag.to_string(),
                        editor_kind: "test".to_string(),
                        editor_version: "test".to_string(),
                        capabilities: vec![
                            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY.to_string(),
                            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY.to_string(),
                        ],
                        timestamp_ms,
                    },
                ),
            ],
        );
    }

    fn seed_reliable_sync_open_without_registration(file: &std::path::Path, tag: &str) {
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        push_test_liveness(
            &canonical,
            &document_hash,
            &[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: document_hash.clone(),
                pid: std::process::id().into(),
                tag: tag.to_string(),
            }],
        );
    }

    fn seed_reliable_sync_close(file: &std::path::Path, tag: &str) {
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        push_test_liveness(
            &canonical,
            &document_hash,
            &[agent_doc_reliable_sync_io::liveness::LivenessOp::Close {
                document_hash: document_hash.clone(),
                pid: std::process::id().into(),
                observed_tags: vec![tag.to_string()],
            }],
        );
    }

    fn project_next_crdt_delivery(
        file: std::path::PathBuf,
        identity: &'static str,
    ) -> std::thread::JoinHandle<()> {
        project_crdt_deliveries(file, identity, 1, std::time::Duration::ZERO)
    }

    fn project_crdt_deliveries(
        file: std::path::PathBuf,
        identity: &'static str,
        count: usize,
        initial_delay: std::time::Duration,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            std::thread::sleep(initial_delay);
            let mut projected = 0usize;
            loop {
                let pull = test_support_pull_replica_updates_for_file(&file, identity)
                    .expect("pull CRDT delivery")
                    .expect("test editor remains attached");
                if let Some(update) = pull.updates.last() {
                    assert_eq!(
                        test_support_observe_replica_projection_for_file(
                            &file,
                            identity,
                            &update.expected_content_hash,
                        )
                        .expect("project CRDT delivery"),
                        Some(true),
                    );
                    // Model the editor's native post-projection save. Visible
                    // projection and
                    // disk projection are separate protocol transitions in
                    // production; most convergence fixtures want both, while the
                    // retained-capture regression below exercises the gap between
                    // them explicitly.
                    let canonical = agent_doc_crdt_relay_io::with_hub(&file, |hub| {
                        hub.canonical_text().to_string()
                    })
                    .expect("read canonical editor buffer after projection");
                    std::fs::write(&file, canonical)
                        .expect("simulate native editor save after projection");
                    projected += 1;
                    if projected == count {
                        return;
                    }
                }
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(3),
                    "timed out waiting for CRDT delivery"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    }

    #[test]
    fn canonical_replace_returns_retained_then_ack_converges_projection() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: settled\n\nDone.\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-visible-ack";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let ack = project_next_crdt_delivery(file.clone(), identity);

        let write =
            apply_canonical_replace_if_attached(&file, baseline, target, "test_crdt_visible_ack")
                .unwrap()
                .expect("attached CRDT write");
        ack.join().unwrap();

        assert!(
            !write.delivery_converged,
            "the foreground call returns the retained projection without polling"
        );
        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert!(matches!(
            current,
            agent_doc_crdt_relay_io::CurrentText::Current {
                ref text,
                delivery_converged: true,
                ..
            } if text == target
        ));
        assert_eq!(write.content_hash, agent_doc_hash::content_hash(target));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
        assert_eq!(
            pending_document_write(&file)
                .expect("delivery projection alone must keep the write intent durable")
                .target_content,
            target,
        );
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "test_crdt_visible_ack_disk_settlement",
            )
            .unwrap(),
        );
        assert!(pending_document_write(&file).is_none());
        assert!(
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap()
                .contains("driver=lazy_projection")
        );
    }

    #[test]
    fn compact_exchange_retains_once_while_cumulative_ack_settles_async() {
        // Regression for the live JB failure: a response was already visible in
        // the editor but its ACK was lost, so Compact Exchange sat behind
        // `prior_delivery_projection_pending` for a full minute. The next target is safe
        // to queue once: the relay's final-content ACK drains the cumulative
        // prefix. The improved Lazily path converges from that cumulative ACK
        // directly, without a second recovery transport.
        let baseline = "# Session\n\nseed\n";
        let source = "# Session\n\nseed\n\n## Exchange\n\nold response\n";
        let compacted = "# Session\n\nseed\n\n## Exchange\n\n*Compacted.*\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-compact-prior-ack";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let prior =
            apply_cp_write_through_relay_authority(&file, baseline, source, "seed_prior_delivery")
                .unwrap()
                .expect("seed write should use attached CRDT relay");
        assert!(
            !prior.delivery_converged,
            "the prior frontier must await ACK"
        );

        let ack = project_crdt_deliveries(
            file.clone(),
            identity,
            2,
            std::time::Duration::from_millis(850),
        );
        let started = std::time::Instant::now();
        let write = apply_canonical_replace_if_attached(&file, source, compacted, "compact")
            .unwrap()
            .expect("compact write should remain pending and then converge");
        ack.join().unwrap();

        assert!(
            started.elapsed() >= std::time::Duration::from_millis(800),
            "fixture must exercise a delayed prior delivery projection"
        );
        assert!(
            !write.delivery_converged,
            "compact returns the retained target instead of polling the delayed ACK"
        );
        assert_eq!(write.content_hash, agent_doc_hash::content_hash(compacted));
        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert!(matches!(
            current,
            agent_doc_crdt_relay_io::CurrentText::Current {
                ref text,
                delivery_converged: true,
                ..
            } if text == compacted
        ));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_crdt_delivery_deferred")
                && log.contains("driver=lazy_projection")
                && !log.contains("compact_crdt_projection_recovery_signal"),
            "compact should retain once and let the cumulative Lazily ACK settle asynchronously:\n{log}"
        );
        let current_text_observations = log
            .lines()
            .filter(|line| line.contains("crdt_current_text file="))
            .count();
        assert!(
            current_text_observations <= 10,
            "one delayed ACK must not trigger a current-text polling storm \
             (observations={current_text_observations}):\n{log}"
        );
    }

    #[test]
    fn compact_exchange_returns_retained_success_when_editor_ack_stays_delayed() {
        let baseline = "# Session\n\nseed\n";
        let compacted = "# Session\n\nseed\n\n## Exchange\n\n*Compacted.*\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-compact-retained-async";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let started = std::time::Instant::now();
        let write = apply_canonical_replace_if_attached(&file, baseline, compacted, "compact")
            .expect("a retained canonical target is not a compact command failure")
            .expect("compact should use the attached CRDT relay");

        assert!(!write.delivery_converged);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "retaining the lazy delivery projection must not wait through a foreground ACK deadline"
        );
        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert!(matches!(
            current,
            agent_doc_crdt_relay_io::CurrentText::Current { ref text, .. }
                if text == compacted
        ));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_crdt_delivery_deferred")
                && log.contains("recovery=retained_async_editor_delivery")
                && log.contains("operator_action=none"),
            "compact must report retained asynchronous recovery as success:\n{log}"
        );
        assert!(
            !log.contains("compact_crdt_projection_recovery_signal")
                && !log.contains("ack_replay")
                && !log.contains("force_refresh"),
            "retained delivery must not issue imperative recovery requests:\n{log}"
        );
        assert!(!log.contains("did not settle within"), "{log}");

        let barrier_started = std::time::Instant::now();
        let barrier_error =
            guard_visible_delivery_convergence(&file, "compact_secondary_effect_test")
                .expect_err("secondary effects must stop behind the retained unACKed target");
        assert!(
            barrier_started.elapsed() < std::time::Duration::from_millis(500),
            "the settlement observation must return pending without polling"
        );
        assert!(
            barrier_error
                .downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "the barrier must preserve the typed no-disk-write failure: {barrier_error:#}"
        );
        assert!(
            format!("{barrier_error:#}").contains("no secondary snapshot/commit"),
            "the operator-facing error must name the effects that were withheld: {barrier_error:#}"
        );
    }

    /// `#deliveryackcut`: a stalled ACK reconciles the replica cache against
    /// process liveness on the path a real zombie actually reaches.
    ///
    /// `async_delivery_recovery_active` flips true on the first retained
    /// projection observation,
    /// so a registered-but-not-ACKing replica ends at
    /// `RetainedForAsyncDelivery`, not `BlockMissingRetention`. Reconciling only
    /// on the blocked path would never run for the common case.
    ///
    /// The fixture's editor process is alive, so the correct outcome is
    /// `live_unacked` — the process owes us a replica and gets nudged to
    /// re-register. It must NOT be dropped, and the canonical must stay
    /// serveable, because the retained target IS the canonical.
    #[test]
    fn a_stalled_ack_keeps_the_retained_projection_without_recovery_request() {
        let baseline = "# Session\n\nseed\n";
        let compacted = "# Session\n\nseed\n\n## Exchange\n\n*Compacted.*\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-stalled-ack-reconcile";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let write = apply_canonical_replace_if_attached(&file, baseline, compacted, "compact")
            .expect("a retained canonical target is not a failure")
            .expect("should use the attached CRDT relay");
        assert!(!write.delivery_converged, "the fixture never ACKs");

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("compact_crdt_delivery_deferred"));
        assert!(log.contains("driver=lazy_projection"));
        assert!(
            !log.contains("crdt_replica_cache_reconciled"),
            "the foreground call must not launch ACK recovery:\n{log}"
        );

        // The canonical stays serveable: the retained target IS the canonical.
        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert!(
            matches!(
                current,
                agent_doc_crdt_relay_io::CurrentText::Current { ref text, .. }
                    if text == compacted
            ),
            "retained canonical must remain current text, got {current:?}"
        );
    }

    #[test]
    fn canonical_replace_crdt_rebases_over_settled_operator_text_once() {
        let baseline = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n- baseline\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\nReady.\n<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n",
        );
        let operator = baseline.replace("- baseline\n", "- baseline\n- operator edit\n");
        let agent = baseline.replace(
            "Ready.\n<!-- agent:boundary:old -->\n<!-- /agent:exchange -->",
            "Ready.\n\n### Re: agent\n\nApplied once.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-rebase";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(client_id, 0, baseline.chars().count() as u32, &operator)
                .unwrap();
        })
        .unwrap();
        let ack = project_next_crdt_delivery(file.clone(), identity);

        let write =
            apply_canonical_replace_if_attached(&file, baseline, &agent, "test_crdt_rebase")
                .unwrap()
                .expect("attached CRDT write");
        ack.join().unwrap();
        let current = match agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("expected current CRDT text, got {other:?}"),
        };

        assert!(
            !write.delivery_converged,
            "the foreground call returns before the editor receipt"
        );
        let delivery = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert!(matches!(
            delivery,
            agent_doc_crdt_relay_io::CurrentText::Current {
                delivery_converged: true,
                ..
            }
        ));
        assert!(current.contains("- operator edit"));
        assert_eq!(current.matches("### Re: agent").count(), 1);
        assert_eq!(current.matches("Applied once.").count(), 1);
        assert!(current.contains("agent:boundary:new"));
        assert!(!current.contains("agent:boundary:old"));
        assert_eq!(current.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn serialized_atomic_write_uses_crdt_before_disk_projection() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody normalized\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-atomic-projection";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let ack = project_next_crdt_delivery(file.clone(), identity);

        let err = atomic_write_through_authority(&file, target).unwrap_err();
        assert!(err.to_string().contains("remains retained"));
        ack.join().unwrap();
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "serialized_atomic_write_after_projection_ack_test",
            )
            .unwrap(),
            "the asynchronous exact projection receipt should settle the retained write",
        );

        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
        let current = match agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current {
                text,
                delivery_converged,
                ..
            } => {
                assert!(delivery_converged);
                text
            }
            other => panic!("expected current CRDT text, got {other:?}"),
        };
        assert_eq!(current, target);
        assert!(
            pending_document_write(&file).is_none(),
            "native disk-save proof must retire the matching write intent",
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("retained_non_capture_projection_settled"));
    }

    #[test]
    fn cas_atomic_write_retains_the_captured_target_after_one_submit() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: once\n\nApplied once.\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-cas-single-submit";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let ack = project_next_crdt_delivery(file.clone(), identity);

        let error =
            atomic_write_if_current_through_authority(&file, target, baseline, "cas_single_submit")
                .expect_err("the foreground boundary should retain until editor settlement");
        assert!(error.to_string().contains("remains retained"));
        ack.join().unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let relay_writes = log
            .lines()
            .filter(|line| line.contains("crdt_cp_write file="))
            .collect::<Vec<_>>();
        assert_eq!(
            relay_writes.len(),
            1,
            "CAS target was resubmitted: {relay_writes:?}"
        );
        assert!(relay_writes[0].contains("source=cas_single_submit"));
        assert!(relay_writes[0].contains("applied=true"));
    }

    #[test]
    fn serialized_atomic_write_rebases_post_proof_editor_advance_exactly_once() {
        let baseline = concat!(
            "# Session\n\n",
            "<!-- agent:backlog -->\n- [ ] existing #existing\n<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n- existing queue\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\nReady.\n<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n",
        );
        let target = baseline
            .replace(
                "- [ ] existing #existing\n",
                "- [ ] existing #existing\n- [ ] randomized high-scale follow-up #haivensharreg\n",
            )
            .replace(
                "Ready.\n<!-- agent:boundary:old -->",
                "Ready.\n\n### Re: Haiven load test\n\nCaptured exactly once.\n<!-- agent:boundary:new -->",
            );
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-post-proof-rebase";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let ack = project_next_crdt_delivery(file.clone(), identity);
        let err = atomic_write_through_authority(&file, &target).unwrap_err();
        assert!(err.to_string().contains("remains retained"));
        ack.join().unwrap();
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            let current = hub.canonical_text();
            let operator = current.replace(
                "- existing queue\n",
                "- existing queue\n- operator typed after delivery proof\n",
            );
            hub.apply_local(client_id, 0, current.chars().count() as u32, &operator)
                .unwrap();
        })
        .unwrap();
        assert!(
            !settle_retained_non_capture_projection_through_authority(
                &file,
                "serialized_atomic_write_post_receipt_operator_advance_test",
            )
            .unwrap(),
            "an unsaved post-receipt operator delta must remain pending",
        );
        let operator_cut = match agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("expected current CRDT text, got {other:?}"),
        };
        std::fs::write(&file, &operator_cut).expect("simulate the operator's native save");
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "serialized_atomic_write_post_receipt_operator_save_test",
            )
            .unwrap(),
            "the exact native-save projection should settle without resubmitting the response",
        );

        let disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(disk.matches("### Re: Haiven load test").count(), 1);
        assert_eq!(disk.matches("#haivensharreg").count(), 1);
        assert!(disk.contains("operator typed after delivery proof"));
        let canonical = match agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current {
                text,
                delivery_converged: true,
                ..
            } => text,
            other => panic!("expected converged CRDT text, got {other:?}"),
        };
        assert_eq!(canonical, disk);
        assert_eq!(canonical.matches("### Re: Haiven load test").count(), 1);
        assert_eq!(canonical.matches("#haivensharreg").count(), 1);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("retained_non_capture_response_projection_settled"));
        assert!(!log.contains("action=rebase_same_intent"));
    }

    #[test]
    fn serialized_atomic_write_retained_ack_timeout_forbids_agent_recapture() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: retained\n\nExactly once.\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-atomic-retained-ack";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let err = atomic_write_through_authority(&file, target).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("binary-owned write"), "{message}");
        assert!(message.contains("same intent"), "{message}");
        assert!(message.contains("Do not recapture"), "{message}");
        assert!(message.contains("do not force disk"), "{message}");
        assert!(
            !message.contains("retry through the document actor"),
            "{message}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        let pending = pending_document_write(&file).expect("retained write intent");
        assert_eq!(pending.target_hash, agent_doc_hash::content_hash(target));
    }

    #[test]
    fn canonical_disk_projection_is_exact_after_editor_saved_same_bytes() {
        let canonical = "# Session\n\neditor-saved canonical\n";
        let (_dir, file, _content) = temp_doc(canonical);

        assert!(canonical_disk_projection_is_exact(&file, canonical));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), canonical);
    }

    #[test]
    fn serialized_atomic_write_defers_zero_replica_editor_owner_without_touching_disk() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n<!-- agent:boundary:deferred -->\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-zero-replica-defer";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let started = std::time::Instant::now();
        let err = atomic_write_through_authority(&file, target).unwrap_err();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "zero-replica authority must fail fast instead of stalling the turn"
        );
        let message = format!("{err:#}");
        assert!(
            message.contains("await_editor_replica_no_disk_write_then_session_check"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("run only agent-doc session-check"),
            "{message}"
        );
        assert!(!message.contains("retry_finalize"), "{message}");
        assert!(!message.contains("resubmit finalize without"), "{message}");
        // `#mrnh` / `#ghosteditorliveness`: the recovery must prove it TARGETS the
        // editor replica (a registration exists to re-register), not that a nudge was
        // hardcoded as "requested". `reason=editor_reregister_primary` is emitted only
        // when the counted signal found a live registration (`found > 0`); the honest
        // per-delivery outcome then follows (`requested:N` when the socket is
        // deliverable, `delivery_failed_to_all:N` in this synthetic harness with no
        // real editor socket). Both are editor-replica recovery, neither is a disk
        // write or supervisor recycle.
        assert!(
            format!("{err:#}").contains("reason=editor_reregister_primary"),
            "zero-replica recovery must target editor replica re-registration (a registration exists), not disk/supervisor recovery: {err:#}"
        );
        assert!(
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .is_none(),
            "targeted editor re-registration must not recycle a healthy supervisor",
        );
        assert!(!dir.path().join(".agent-doc/crdt-replica-events").exists());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            baseline,
            "the editor-owned file projection must not change behind JetBrains"
        );
        assert_eq!(
            try_resolve_current_document_content(&file, "zero_replica_admission_verify").unwrap(),
            baseline,
            "a zero-recipient mutation must be retained before apply, not installed as a new canonical frontier",
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("targets=0 live_editors=0 recovery=await_editor_replica_no_disk_write"),
            "pre-admission must avoid manufacturing a zero-target CRDT transition:\n{log}",
        );

        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(dir.path())
                .unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let pending = projection
            .document(&document_hash)
            .and_then(|document| document.document.pending_write.as_ref())
            .expect("deferred target must survive in Lazily state");
        assert_eq!(pending.target_content, target);
        assert_eq!(pending.target_hash, agent_doc_hash::content_hash(target));
        assert_eq!(pending.expected_content.as_deref(), Some(baseline));
        assert_eq!(pending.reason, "editor_owner_without_registered_replica");

        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline)
                .unwrap()
                .as_deref(),
            Some(target),
            "a clean stale editor buffer should restore the durable target",
        );
        let editor_with_unsaved_note = format!("{baseline}\noperator note\n");
        let merged = deferred_document_write_reconnect_content(&file, &editor_with_unsaved_note)
            .unwrap()
            .expect("deferred write should merge with later editor text");
        assert!(merged.contains("agent:boundary:deferred"));
        assert!(merged.contains("operator note"));
    }

    #[test]
    fn explicit_last_editor_close_projects_retained_response_over_unsaved_queue_deletion() {
        let baseline = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: base — gpt-5\n\nBase response.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#deleted-unsaved]\n",
            "- do [#kept]\n",
            "<!-- /agent:queue -->\n",
        );
        let editor_cut = baseline.replace("- do [#deleted-unsaved]\n", "");
        let target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: retained — gpt-5\n\nRetained response.\n<!-- /agent:exchange -->",
        );
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-last-close-unsaved-delete";
        seed_reliable_sync_open(&file, identity);
        let (client_id, bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("closing editor replica should attach");
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let replica_text = replica.text();
        replica.apply_local_edit(0, replica_text.len() as u32, &editor_cut);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &file,
            identity,
            &replica.encode_state(),
        )
        .unwrap()
        .expect("unsaved deletion should publish to Lazily");
        assert!(agent_doc_crdt_relay_io::deregister_replica_for_file(&file, identity).unwrap());

        let err = atomic_write_through_authority(&file, &target).unwrap_err();
        assert!(format!("{err:#}").contains("await_editor_replica_no_disk_write"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        assert!(pending_document_write(&file).is_some());

        seed_reliable_sync_close(&file, identity);
        assert_eq!(
            agent_doc_reliable_sync_io::plane_editor_live_for_path(&file.to_string_lossy()),
            Some(false),
        );
        assert!(materialize_last_editor_close_through_authority(&file, "last_close_test").unwrap());

        let projected = std::fs::read_to_string(&file).unwrap();
        assert!(projected.contains("Retained response."));
        assert!(projected.contains("do [#kept]"));
        assert!(
            !projected.contains("deleted-unsaved"),
            "the closing editor's queue tombstone must be monotonic:\n{projected}"
        );
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn retained_response_rebase_preserves_live_prompt_and_operator_deletions() {
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Compare the profiles.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#haivenresume]\n",
            "- do [#haivenapply]\n",
            "- do [#haivenprofiles]\n",
            "- do [#kept]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#haivenresume] resume\n",
            "- [ ] [#haivenapply] apply\n",
            "- [ ] [#haivenprofiles] profiles\n",
            "- [ ] [#kept] keep\n",
            "<!-- /agent:backlog -->\n",
        );
        let agent_target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: Compare the profiles. — gpt-5\n\nComparison complete.\n<!-- /agent:exchange -->",
        );
        let editor_cut = baseline
            .replace(
                "❯ Compare the profiles.\n",
                "❯ Compare the profiles.\n❯ Implement gRPC batching.\n",
            )
            .replace("- do [#haivenresume]\n", "")
            .replace("- do [#haivenapply]\n", "")
            .replace("- do [#haivenprofiles]\n", "")
            .replace("- [ ] [#haivenresume] resume\n", "")
            .replace("- [ ] [#haivenapply] apply\n", "")
            .replace("- [ ] [#haivenprofiles] profiles\n", "");

        let rebased =
            rebase_agent_candidate_over_editor_cut(baseline, &agent_target, &editor_cut).unwrap();

        assert!(rebased.contains("### Re: Compare the profiles."));
        assert!(rebased.contains("❯ Implement gRPC batching."));
        assert!(rebased.contains("[#kept]"));
        assert!(!rebased.contains("haivenresume"));
        assert!(!rebased.contains("haivenapply"));
        assert!(!rebased.contains("haivenprofiles"));
    }

    #[test]
    fn haiven_boundary_rebase_preserves_live_prompt_once_and_deleted_queue_items_stay_deleted() {
        let live_prompt = "❯ I'm deferring to DESIGN.md and focusing haiven-websocket-hub-takehome-v2.md on salient design aspects + load test-driven design evolution. Please review. I need the highlights of the benchmarks. Please print them here. If we need to redo any benchmarks, please do so. I have until tomorrow morning.";
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Compare the profiles.\n",
            "<!-- agent:boundary:15ab685a -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#haivenresume]\n",
            "- do [#haivenapply]\n",
            "- do [#haivenprofiles]\n",
            "- do [#kept]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#haivenresume] resume\n",
            "- [ ] [#haivenapply] apply\n",
            "- [ ] [#haivenprofiles] profiles\n",
            "- [ ] [#kept] keep\n",
            "<!-- /agent:backlog -->\n",
        );
        let agent_target = baseline.replace(
            "<!-- agent:boundary:15ab685a -->",
            "### Re: Compare the profiles. — gpt-5\n\nComparison complete.\n<!-- agent:boundary:response -->",
        );
        let editor_cut = baseline
            .replace(
                "<!-- agent:boundary:15ab685a -->",
                &format!("{live_prompt}\n<!-- agent:boundary:operator -->"),
            )
            .replace("- do [#haivenresume]\n", "")
            .replace("- do [#haivenapply]\n", "")
            .replace("- do [#haivenprofiles]\n", "")
            .replace("- [ ] [#haivenresume] resume\n", "")
            .replace("- [ ] [#haivenapply] apply\n", "")
            .replace("- [ ] [#haivenprofiles] profiles\n", "");

        let rebased =
            rebase_agent_candidate_over_editor_cut(baseline, &agent_target, &editor_cut).unwrap();

        assert_eq!(
            rebased.matches(live_prompt).count(),
            1,
            "rebased:\n{rebased}"
        );
        assert_eq!(
            rebased.matches("agent:boundary:").count(),
            1,
            "rebased:\n{rebased}"
        );
        assert_eq!(rebased.matches("### Re: Compare the profiles.").count(), 1);
        assert!(rebased.contains("[#kept]"));
        assert!(!rebased.contains("haivenresume"));
        assert!(!rebased.contains("haivenapply"));
        assert!(!rebased.contains("haivenprofiles"));
        assert!(agent_projection_integrity_valid(&rebased));
    }

    #[test]
    fn retained_response_rebase_is_noop_after_cell_is_already_live() {
        let baseline = concat!(
            "<!-- agent:exchange -->\n❯ Question.\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n- do [#deleted]\n<!-- /agent:queue -->\n",
        );
        let target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: Question. — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
        );
        let live = target
            .replace("❯ Question.\n", "❯ Question.\n❯ New prompt.\n")
            .replace("- do [#deleted]\n", "");

        let rebased = rebase_agent_candidate_over_editor_cut(baseline, &target, &live).unwrap();

        assert_eq!(rebased, live);
    }

    #[test]
    fn acknowledged_operator_cut_short_circuits_retained_response_reapply() {
        let baseline = concat!(
            "<!-- agent:exchange -->\n❯ Question.\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n- do [#deleted]\n<!-- /agent:queue -->\n",
        );
        let target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: Question. — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
        );
        let operator_cut = target
            .replace("❯ Question.\n", "❯ Question.\n❯ New prompt.\n")
            .replace("- do [#deleted]\n", "");
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-acknowledged-operator-cut-no-reapply";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(client_id, 0, baseline.chars().count() as u32, &operator_cut)
                .unwrap();
        })
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            baseline,
            &target,
            "acknowledged_operator_cut_seed",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        std::fs::write(&file, &operator_cut)
            .expect("simulate the acknowledged editor cut's native save");

        let receipt = apply_canonical_replace_if_attached(
            &file,
            baseline,
            &target,
            "acknowledged_operator_cut_test",
        )
        .unwrap()
        .expect("live editor authority should return an acknowledged receipt");

        assert!(!receipt.applied, "the accepted editor cut must be a no-op");
        assert_eq!(receipt.update_bytes, 0);
        assert_eq!(receipt.targets, 0);
        assert!(receipt.delivery_converged);
        assert_eq!(
            receipt.content_hash,
            agent_doc_hash::content_hash(&operator_cut)
        );
        let pending = pending_document_write(&file)
            .expect("native-save recovery must retain the accepted editor cut");
        assert_eq!(pending.target_content, operator_cut);
    }

    #[test]
    fn identical_target_waits_for_existing_delivery_without_reapply() {
        let baseline =
            "# Session\n\n<!-- agent:exchange -->\n❯ Question.\n<!-- /agent:exchange -->\n";
        let target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: Question. — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
        );
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-identical-target-existing-delivery";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let initial = agent_doc_crdt_relay_io::apply_cp_write_for_file(
            &file,
            baseline,
            &target,
            "identical_target_initial_write",
        )
        .unwrap()
        .expect("initial write should use the relay");
        assert!(initial.applied);
        assert!(!initial.delivery_converged);
        let ack = project_crdt_deliveries(
            file.clone(),
            identity,
            1,
            std::time::Duration::from_millis(100),
        );

        let receipt =
            apply_canonical_replace_if_attached(&file, baseline, &target, "identical_target_retry")
                .unwrap()
                .expect("existing delivery should converge");
        ack.join().unwrap();

        assert!(!receipt.applied);
        assert_eq!(receipt.update_bytes, 0);
        assert_eq!(receipt.targets, 0);
        assert!(receipt.delivery_converged);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
    }

    #[test]
    fn retained_response_rebase_exhausts_editor_cut_interleavings() {
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "❯ Original prompt.\n",
            "<!-- agent:boundary:one -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n- do [#operator-deleted]\n<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n- [ ] [#operator-deleted] work\n<!-- /agent:backlog -->\n",
        );
        let target = baseline.replace(
            "<!-- agent:boundary:one -->",
            "### Re: Original prompt. — gpt-5\n\nAnswered.\n<!-- agent:boundary:one -->",
        );

        for response_already_live in [false, true] {
            for operator_deletes in [false, true] {
                for operator_adds_prompt in [false, true] {
                    let mut editor_cut = if response_already_live {
                        target.clone()
                    } else {
                        baseline.to_string()
                    };
                    if operator_deletes {
                        editor_cut = editor_cut
                            .replace("- do [#operator-deleted]\n", "")
                            .replace("- [ ] [#operator-deleted] work\n", "");
                    }
                    if operator_adds_prompt {
                        editor_cut = editor_cut.replace(
                            "❯ Original prompt.\n",
                            "❯ Original prompt.\n❯ New prompt during delivery.\n",
                        );
                    }

                    let rebased =
                        rebase_agent_candidate_over_editor_cut(baseline, &target, &editor_cut)
                            .unwrap();
                    assert_eq!(rebased.matches("### Re: Original prompt.").count(), 1);
                    assert_eq!(rebased.matches("agent:boundary:").count(), 1);
                    assert_eq!(
                        rebased.contains("operator-deleted"),
                        !operator_deletes,
                        "queue/backlog deletion state changed for already_live={response_already_live}, adds_prompt={operator_adds_prompt}\n{rebased}",
                    );
                    assert_eq!(
                        rebased.contains("❯ New prompt during delivery."),
                        operator_adds_prompt,
                    );
                    assert!(agent_projection_integrity_valid(&rebased));
                    if response_already_live {
                        assert_eq!(rebased, editor_cut);
                    }
                }
            }
        }
    }

    #[test]
    fn stale_delivery_worker_retains_target_without_ack_wait_or_disk_write() {
        let baseline = "# Session\n\nvesting question\n";
        let target = "# Session\n\nvesting question\n\nagent response\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-stale-delivery-worker";
        seed_reliable_sync_open_without_registration(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        let started = std::time::Instant::now();
        let err = atomic_write_through_authority(&file, target).unwrap_err();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "a stale component must fail fast instead of burning the ACK deadline",
        );
        let message = format!("{err:#}");
        assert!(
            message.contains("delivery worker heartbeat is stale"),
            "{message}"
        );
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "the closeout classifier must retain the no-disk recovery branch: {message}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);

        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(dir.path())
                .unwrap();
        let document_id = agent_doc_hash::document_id_for_path(&file);
        let pending = projection
            .document(&document_id)
            .and_then(|document| document.document.pending_write.as_ref())
            .expect("the exact target must remain retained");
        assert_eq!(pending.target_content, target);
        assert_eq!(pending.reason, "editor_delivery_worker_stale");
    }

    #[test]
    fn zero_member_ack_quorum_is_not_editor_visible_without_durable_proof() {
        assert!(delivery_convergence_is_editor_visible(1, false));
        assert!(delivery_convergence_is_editor_visible(0, true));
        assert!(
            !delivery_convergence_is_editor_visible(0, false),
            "a disappeared replica must not make an unsaved IDE buffer safe to overwrite",
        );
    }

    #[test]
    fn accepted_repair_intent_is_retained_instead_of_failing_immediate_equality() {
        let baseline = "# Session\n\n### Re: topic — gpt-5\n\nAnswer.\n\n### Re: topic — gpt-5\n";
        let target = "# Session\n\n### Re: topic — gpt-5\n\nAnswer.\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let intent_id = retain_deferred_document_write_target(
            &file,
            baseline,
            target,
            "repair_retained_projection_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let err = settle_atomic_repair_projection(
            &file,
            target,
            baseline,
            "repair_retained_projection_test",
            baseline.to_string(),
            baseline.to_string(),
        )
        .unwrap_err();

        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "a controller-owned retained repair is a typed projection deferral, not a false write failure: {err:#}",
        );
        let message = format!("{err:#}");
        assert!(message.contains(&intent_id), "{message}");
        assert!(message.contains("Do not resubmit"), "{message}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        assert_eq!(
            pending_document_write(&file)
                .expect("reactive projection must retain its owner")
                .target_content,
            target,
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_projection_retained"), "{log}");
        assert!(
            log.contains("recovery=controller_reactive_projection operator_action=none"),
            "{log}",
        );
    }

    #[test]
    fn late_editor_attachment_retains_original_repair_lineage() {
        let baseline = "# Session\n\n### Re: topic — gpt-5\n\nAnswer.\n\nReplay.\n";
        let target = "# Session\n\n### Re: topic — gpt-5\n\nAnswer.\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        std::fs::write(&file, target).unwrap();

        let err = settle_atomic_repair_projection(
            &file,
            target,
            baseline,
            "late_editor_repair_test",
            baseline.to_string(),
            target.to_string(),
        )
        .unwrap_err();

        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "the late registration race must be a resumable projection deferral: {err:#}",
        );
        let message = format!("{err:#}");
        assert!(message.contains(RETAINED_FOR_RETRY_MARKER), "{message}");
        let pending = pending_document_write(&file).expect("repair lineage must remain durable");
        assert_eq!(pending.expected_content.as_deref(), Some(baseline));
        assert_eq!(pending.target_content, target);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
    }

    #[test]
    fn repair_cas_retains_target_when_editor_owner_has_zero_replicas() {
        let baseline = "# Session\n\nfragmented response\n<!-- agent:boundary:old --><!-- agent:boundary:old -->\n";
        let first_target = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-repair-zero-replica-projection";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_repair_write_if_current_through_authority(
            &file,
            first_target,
            baseline,
            "repair_zero_replica_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "repair must retain the same typed no-disk deferral as an ordinary write: {err:#}",
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        assert_eq!(
            try_resolve_current_document_content(&file, "repair_zero_replica_verify").unwrap(),
            baseline,
            "repair must not promote its retained target without a controller projection receipt",
        );
        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(
                file.parent().unwrap(),
            )
            .unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let pending = projection
            .document(&document_hash)
            .and_then(|document| document.document.pending_write.as_ref())
            .expect("reactive repair must preserve editor reconnect lineage");
        assert_eq!(pending.target_content, first_target);
        assert_eq!(
            pending.reason,
            DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica
        );
        assert!(
            projection
                .document(&document_hash)
                .and_then(|document| document.document.pending_external_disk.as_ref())
                .is_none()
        );
        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline)
                .unwrap()
                .as_deref(),
            Some(first_target),
            "the retained controller target should be delivered lazily on reconnect",
        );
        assert!(
            pending_document_write(&file).is_some(),
            "a reconnect read alone is not an exact controller projection receipt",
        );
    }

    #[test]
    fn deferred_repair_never_force_projects_over_newer_operator_cut() {
        let editor_base = concat!(
            "<!-- agent:exchange -->\n",
            "❯ Original prompt.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#keep]\n- do [#deleted]\n<!-- /agent:queue -->\n",
        );
        let response_target = editor_base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: Original prompt. — gpt-5\n\nAnswered.\n<!-- agent:boundary:base -->",
        );
        let operator_cut = response_target
            .replace(
                "❯ Original prompt.\n",
                "❯ Original prompt.\n❯ Prompt typed during repair.\n",
            )
            .replace("- do [#deleted]\n", "");
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-repair-zero-replica-semantic-rebase";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let race_file = file.clone();
        let race_response_target = response_target.clone();
        let race_operator_cut = operator_cut.clone();
        let race = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            loop {
                let pull = test_support_pull_replica_updates_for_file(&race_file, identity)
                    .expect("pull repair delivery")
                    .expect("test editor remains attached until the raced delivery arrives");
                if !pull.updates.is_empty() {
                    agent_doc_crdt_relay_io::with_hub(&race_file, |hub| {
                        hub.apply_local(
                            client_id,
                            0,
                            race_response_target.chars().count() as u32,
                            &race_operator_cut,
                        )
                        .unwrap();
                        hub.deregister(client_id);
                    })
                    .unwrap();
                    return;
                }
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(3),
                    "timed out waiting for the repair delivery race"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });

        let err = atomic_repair_write_if_current_through_authority(
            &file,
            &response_target,
            editor_base,
            "repair_zero_replica_semantic_rebase_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "a raced repair must stay retained until its controller projection is acknowledged: {err:#}",
        );
        race.join().unwrap();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            editor_base,
            "repair must not force-project either the retained target or the newer editor cut",
        );
        let canonical = try_resolve_current_document_content(
            &file,
            "deferred_repair_newer_operator_cut_verify",
        )
        .unwrap();
        assert_eq!(canonical, operator_cut);
        assert!(canonical.contains("Prompt typed during repair."));
        assert!(!canonical.contains("#deleted"));
        assert_eq!(
            canonical
                .matches("### Re: Original prompt. — gpt-5")
                .count(),
            1
        );
        assert_eq!(canonical.matches("agent:boundary:").count(), 1);
        let pending = pending_document_write(&file)
            .expect("unacknowledged controller target must remain available for exact replay");
        assert_eq!(pending.target_content, response_target);
    }

    #[test]
    fn committed_projection_settlement_clears_stale_deferred_lineage() {
        let editor_base = "# Session\n\ncomplete response\n";
        let stale_projection = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, stale_projection).unwrap();
        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            stale_projection,
            "committed_projection_stale_intent_test",
            DocumentWriteDeferredReason::RetainEditorReconnectLineageBeforeDiskProjection,
        )
        .unwrap();
        assert!(pending_document_write(&file).is_some());

        settle_committed_projection_if_current_through_authority(
            &file,
            committed,
            stale_projection,
            "committed_projection_settlement_test",
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), committed);
        assert_eq!(
            try_resolve_current_document_content(&file, "committed_projection_verify").unwrap(),
            committed,
        );
        assert!(
            pending_document_write(&file).is_none(),
            "a settled committed projection must not retain or uncover an older target"
        );
    }

    #[test]
    fn live_canonical_target_supersedes_stale_deferred_response_lineage() {
        let baseline = "<!-- agent:exchange -->\n### Re: committed — gpt-5\n\nCommitted.\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        let stale_target = "<!-- agent:exchange -->\n### Re: committed — gpt-5\n\nCommitted.\n\n### Re: stale — gpt-5\n\nStale response.\n<!-- agent:boundary:stale -->\n<!-- /agent:exchange -->\n";
        let latest_target = "<!-- agent:exchange -->\n### Re: committed — gpt-5\n\nCommitted.\n\n### Re: latest — gpt-5\n\nComplete response.\n<!-- agent:boundary:latest -->\n<!-- /agent:exchange -->\n";
        let (_dir, file, _canonical) = temp_doc(baseline);

        ensure_deferred_document_write_intent(
            &file,
            baseline,
            stale_target,
            "stale_response_intent_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            latest_target,
            latest_target,
            "latest_response_intent_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let pending = pending_document_write(&file).expect("latest target should remain retained");
        assert_eq!(pending.target_content, latest_target);
        assert_eq!(pending.expected_content.as_deref(), Some(stale_target));
        assert_eq!(
            deferred_document_write_reconnect_content(&file, stale_target)
                .unwrap()
                .as_deref(),
            Some(latest_target),
            "the stale editor cut must receive the newer canonical response without recomposition",
        );

        let editor_with_operator_note = stale_target.replace(
            "<!-- /agent:exchange -->",
            "\n❯ Operator note after the failed ACK.\n<!-- /agent:exchange -->",
        );
        let merged = deferred_document_write_reconnect_content(&file, &editor_with_operator_note)
            .unwrap()
            .expect("later operator text should merge over the superseded editor cut");
        assert!(merged.contains("### Re: latest — gpt-5"));
        assert!(!merged.contains("### Re: stale — gpt-5"));
        assert!(merged.contains("❯ Operator note after the failed ACK."));
        assert_eq!(merged.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn queue_consume_reconnect_keeps_latest_singleton_boundary() {
        let response_target = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ /goal Implement backlog item(s): #missing\n",
            "### Re: complete — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:response -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- /goal Implement backlog item(s): #missing\n",
            "<!-- /agent:queue -->\n",
        );
        let queue_consumed_target = response_target.replace(
            "- /goal Implement backlog item(s): #missing",
            "- ~~/goal Implement backlog item(s): #missing~~",
        );
        let editor_cut = response_target
            .replace(
                "### Re: complete — gpt-5",
                "<!-- agent:boundary:stale-editor -->\n### Re: complete — gpt-5",
            )
            .replace("<!-- agent:boundary:response -->\n", "");
        let (_dir, file, _canonical) = temp_doc(response_target);

        ensure_deferred_document_write_intent(
            &file,
            response_target,
            &queue_consumed_target,
            "queue_consume_boundary_reconnect_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let reconnected = deferred_document_write_reconnect_content(&file, &editor_cut)
            .unwrap()
            .expect("a newline-normalized editor cut should receive the queue-consumed target");
        assert!(reconnected.contains("- ~~/goal Implement backlog item(s): #missing~~"));
        assert!(
            reconnected.contains("agent:boundary:response"),
            "reconnected document:\n{reconnected}"
        );
        assert!(!reconnected.contains("agent:boundary:stale-editor"));
        assert_eq!(
            reconnected.matches("agent:boundary:").count(),
            1,
            "reconnected document:\n{reconnected}"
        );
    }

    #[test]
    fn deferred_target_composition_keeps_newest_singleton_boundary() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Apply the change.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let first_target = base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: first — gpt-5\n\nFirst target.\n<!-- agent:boundary:first -->",
        );
        let newest_target = base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: newest — gpt-5\n\nNewest target.\n<!-- agent:boundary:newest -->",
        );
        let (_dir, file, _canonical) = temp_doc(base);

        ensure_deferred_document_write_intent(
            &file,
            base,
            &first_target,
            "deferred_boundary_composition_first",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            base,
            &newest_target,
            "deferred_boundary_composition_newest",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let pending = pending_document_write(&file).expect("newest deferred target retained");
        assert!(pending.target_content.contains("agent:boundary:newest"));
        assert!(!pending.target_content.contains("agent:boundary:first"));
        assert!(!pending.target_content.contains("agent:boundary:base"));
        assert_eq!(pending.target_content.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn retained_target_retries_do_not_multiply_operator_cleaned_queue_item() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Apply the change.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#autoinstalldeferstale] once\n",
            "<!-- /agent:queue -->\n",
        );
        let response_target = base
            .replace(
                "<!-- agent:boundary:base -->",
                "### Re: retained — gpt-5\n\nRetained response.\n<!-- agent:boundary:response -->",
            )
            .replace(
                "- do [#autoinstalldeferstale] once\n",
                concat!(
                    "- do [#autoinstalldeferstale] once\n",
                    "- do [#autoinstalldeferstale] once\n",
                    "- do [#autoinstalldeferstale] once\n",
                ),
            );
        let live_editor = base.replace(
            "<!-- agent:boundary:base -->",
            "while I was typing the next queue item\n<!-- agent:boundary:base -->",
        );
        let (_dir, file, _canonical) = temp_doc(base);

        ensure_deferred_document_write_intent(
            &file,
            base,
            &response_target,
            "retained_queue_replay_first",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            base,
            &live_editor,
            "retained_queue_replay_merge",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let first = pending_document_write(&file).expect("composed target retained");
        assert_eq!(
            first
                .target_content
                .matches("- do [#autoinstalldeferstale] once")
                .count(),
            1,
            "the live operator cut governs duplicate durable queue ids"
        );

        ensure_deferred_document_write_intent(
            &file,
            base,
            &live_editor,
            "retained_queue_replay_retry",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        let retry = pending_document_write(&file).expect("retry target retained");
        assert_eq!(
            retry
                .target_content
                .matches("- do [#autoinstalldeferstale] once")
                .count(),
            1,
            "reusing the original merge base must not multiply the queue item"
        );
        assert_eq!(retry.target_content.matches("agent:boundary:").count(), 1);
        assert!(retry.target_content.contains("Retained response."));
    }

    #[test]
    fn post_cas_retry_keeps_queue_prompt_response_and_terminator_singleton() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Drain the queue head.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let first_target = base.replace(
            "<!-- agent:boundary:base -->",
            concat!(
                "> **Queue prompt:** Drain the queue head.\n\n",
                "### Re: queue head — gpt-5\n\n",
                "Done once.\n",
                "<!-- agent:boundary:response -->",
            ),
        );
        let raced_retry = first_target.replace(
            "<!-- /agent:exchange -->",
            concat!(
                "<!-- /agent:exchange -->\n",
                "Operator follow-up typed during the CAS retry.",
            ),
        );
        let (_dir, file, _canonical) = temp_doc(base);

        ensure_deferred_document_write_intent(
            &file,
            base,
            &first_target,
            "patchretryidem_first_attempt",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            base,
            &raced_retry,
            "patchretryidem_second_attempt",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let composed = pending_document_write(&file).expect("raced retry retained");
        let target = composed.target_content.clone();
        assert_eq!(target.matches("> **Queue prompt:**").count(), 1);
        assert_eq!(target.matches("### Re: queue head — gpt-5").count(), 1);
        assert_eq!(target.matches("<!-- agent:boundary:").count(), 1);
        assert_eq!(target.matches("<!-- /agent:exchange -->").count(), 1);
        assert!(
            target.contains("Operator follow-up typed during the CAS retry."),
            "concurrent operator text must survive the retry:\n{target}"
        );
        assert!(
            !target.lines().any(|line| line.trim() == "-->"),
            "the retry must not splice a bare partial terminator:\n{target}"
        );
        assert!(agent_projection_integrity_valid(&target));

        ensure_deferred_document_write_intent(
            &file,
            base,
            &raced_retry,
            "patchretryidem_third_attempt",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        let retried = pending_document_write(&file).expect("idempotent retry retained");
        assert_eq!(retried.target_content, target);
    }

    #[test]
    fn committed_boundary_refinement_replaces_retained_target_without_recomposition() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "> 📌 do [#dbj7]\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let committed_cut = base.replace(
            "<!-- agent:boundary:base -->",
            concat!(
                "<!-- agent:boundary:response -->\n",
                "### Re: #dbj7 — gpt-5\n\n",
                "Complete response."
            ),
        );
        let refined = committed_cut.replace(
            concat!(
                "<!-- agent:boundary:response -->\n",
                "### Re: #dbj7 — gpt-5\n\n",
                "Complete response."
            ),
            concat!(
                "### Re: #dbj7 — gpt-5\n\n",
                "Complete response.\n",
                "<!-- agent:boundary:response -->"
            ),
        );
        let (_dir, file, _canonical) = temp_doc(base);

        ensure_deferred_document_write_intent(
            &file,
            base,
            &committed_cut,
            "post_commit_refinement_seed",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        refine_deferred_document_write_target(
            &file,
            &committed_cut,
            &refined,
            "post_commit_reposition",
            DocumentWriteDeferredReason::ExtendPendingEditorReconnectTarget,
        )
        .unwrap();

        let pending = pending_document_write(&file).expect("refined target retained");
        assert_eq!(pending.target_content, refined);
        assert_eq!(pending.expected_content.as_deref(), Some(base));
        assert_eq!(pending.target_content.matches("> 📌 do [#dbj7]").count(), 1);
        assert_eq!(pending.target_content.matches("### Re: #dbj7").count(), 1);
        assert_eq!(pending.target_content.matches("agent:boundary:").count(), 1);
        assert!(
            !pending
                .target_content
                .lines()
                .any(|line| line.trim() == "-->")
        );
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&pending.target_content),
            None
        );
    }

    #[test]
    fn reconnect_replays_each_deferred_backlog_change_in_order() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#fundlink2] existing\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let added = base.replace(
            "<!-- /agent:backlog -->",
            "- [ ] [#fund-vesting-fk] answer vesting question\n<!-- /agent:backlog -->",
        );
        // This later target was independently composed from the same editor
        // base and therefore lacks the earlier add—the exact ACK-wedge failure
        // that used to lose `--backlog-add`.
        let marked = base.replace("- [ ] [#fundlink2]", "- [x] [#fundlink2]");
        let (_dir, file, _) = temp_doc(base);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for (event_id, intent_id, target) in [
            ("deferred-add", "intent-add", added.as_str()),
            ("deferred-mark", "intent-mark", marked.as_str()),
        ] {
            let event = agent_doc_state_backbone::StateEvent::new(
                event_id,
                agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                    document_hash: document_hash.clone(),
                    intent_id: intent_id.to_string(),
                    expected_hash: agent_doc_hash::content_hash(base),
                    expected_content: Some(base.to_string()),
                    target_hash: agent_doc_hash::content_hash(target),
                    target_content: target.to_string(),
                    source: agent_doc_state_backbone::DocumentWriteSource::from("test"),
                    reason: DocumentWriteDeferredReason::EditorProjectionPending,
                },
            );
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                file.parent().unwrap(),
                &event,
            )
            .unwrap();
        }

        let journal = pending_document_write_journal(&file);
        assert_eq!(journal.len(), 2);
        let reconnected = deferred_document_write_reconnect_content(&file, base)
            .unwrap()
            .expect("journal should replay over the editor base");
        assert!(reconnected.contains("[#fund-vesting-fk]"));
        assert!(reconnected.contains("- [x] [#fundlink2]"));
        assert_eq!(reconnected.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn complete_queue_prompt_supersedes_progressive_editor_reconnect_cut() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#fresh-project-supervisor-log]: agent-doc star\n",
            "<!-- /agent:queue -->\n",
        );
        let partial = base.replace("agent-doc star\n", "agent-doc start should create a c\n");
        let complete = base.replace(
            "agent-doc star\n",
            "agent-doc start should create a configurable path to the supervisor log file. Directories should be created as needed.\n",
        );
        let (_dir, file, _) = temp_doc(base);

        ensure_deferred_document_write_intent(
            &file,
            base,
            &partial,
            "editor_reconnect",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            base,
            &complete,
            "serialized_atomic_write",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();

        let pending = pending_document_write(&file).expect("complete target retained");
        assert!(pending.target_content.contains(
            "- do [#fresh-project-supervisor-log]: agent-doc start should create a configurable path to the supervisor log file. Directories should be created as needed.\n<!-- /agent:queue -->"
        ));
        assert!(
            !pending
                .target_content
                .contains("needed.t should create a c")
        );
        assert_eq!(pending_document_write_journal(&file).len(), 1);
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&pending.target_content),
            None,
        );
    }

    #[test]
    fn reconnect_filters_historical_progressive_cuts_but_keeps_independent_intents() {
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#x] sta\n",
            "<!-- /agent:queue -->\n",
        );
        let partial = base.replace("[#x] sta", "[#x] start partial");
        let complete = base.replace("[#x] sta", "[#x] start complete");
        let (_dir, file, _) = temp_doc(base);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for (event_id, intent_id, target, source) in [
            (
                "progressive-1",
                "progressive-1",
                partial.as_str(),
                "editor_reconnect",
            ),
            (
                "complete-2",
                "complete-2",
                complete.as_str(),
                "test-independent",
            ),
        ] {
            let event = agent_doc_state_backbone::StateEvent::new(
                event_id,
                agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                    document_hash: document_hash.clone(),
                    intent_id: intent_id.to_string(),
                    expected_hash: agent_doc_hash::content_hash(base),
                    expected_content: Some(base.to_string()),
                    target_hash: agent_doc_hash::content_hash(target),
                    target_content: target.to_string(),
                    source: agent_doc_state_backbone::DocumentWriteSource::from(source),
                    reason: if source == "editor_reconnect" {
                        DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
                    } else {
                        DocumentWriteDeferredReason::EditorProjectionPending
                    },
                },
            );
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                file.parent().unwrap(),
                &event,
            )
            .unwrap();
        }

        let reconnected = deferred_document_write_reconnect_content(&file, base)
            .unwrap()
            .expect("latest target should replay");
        assert!(reconnected.contains("[#x] start complete"));
        assert!(!reconnected.contains("[#x] start partial"));
    }

    #[test]
    fn retained_committed_projection_resumes_after_replica_reattach() {
        let editor_base = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-committed-projection-reattach";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_write_if_current_through_authority(
            &file,
            committed,
            editor_base,
            "retained_committed_projection_zero_replica_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some(),
            "zero-replica write should retain the canonical target without projecting disk: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), editor_base);
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_committed_projection_zero_replica_verify",
            )
            .unwrap(),
            editor_base,
            "a retained target is not current editor authority before reconnect replay",
        );
        let pending = pending_document_write(&file).expect("retained delivery intent");
        assert_eq!(pending.expected_content.as_deref(), Some(editor_base));
        assert_eq!(pending.target_content, committed);

        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should register before deferred replay");
        let replayed = deferred_document_write_reconnect_content(&file, editor_base)
            .unwrap()
            .expect("replacement editor should receive the retained committed projection");
        assert_eq!(replayed, committed);
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_committed_projection_post_register_verify",
            )
            .unwrap(),
            editor_base,
            "the editor must not publish the reconnect target upstream before controller delivery",
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), editor_base);
        let pending =
            pending_document_write(&file).expect("replay must retain native-save lineage");
        assert_eq!(pending.expected_content.as_deref(), Some(editor_base));
        assert_eq!(pending.target_content, committed);
        let ack = project_next_crdt_delivery(file.clone(), identity);
        assert!(
            !settle_retained_committed_projection_through_authority(
                &file,
                committed,
                editor_base,
                "retained_committed_projection_reattach_test",
            )
            .unwrap(),
            "a matching retained lineage stays pending until the editor projection receipt arrives"
        );
        ack.join().unwrap();
        assert!(
            settle_retained_committed_projection_through_authority(
                &file,
                committed,
                editor_base,
                "retained_committed_projection_after_ack_test",
            )
            .unwrap(),
            "the exact asynchronous receipt should settle matching committed lineage",
        );

        assert_eq!(std::fs::read_to_string(&file).unwrap(), committed);
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_committed_projection_reattach_verify",
            )
            .unwrap(),
            committed,
        );
        assert!(
            pending_document_write(&file).is_none(),
            "successful reconnect delivery must clear retained lineage"
        );
    }

    #[test]
    fn retained_captured_projection_settles_after_replacement_replica_bootstrap() {
        let editor_base =
            "# Session\n\n<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n";
        let captured_response = "### Re: investigate\n\nFixed the retained closeout.\n";
        let captured_target = format!(
            "# Session\n\n<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            captured_response.trim_end()
        );
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-captured-replacement-bootstrap";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_write_if_current_through_authority(
            &file,
            &captured_target,
            editor_base,
            "retained_captured_projection_zero_replica_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some()
        );
        assert!(
            format!("{err:#}").contains(RETAINED_FOR_RETRY_MARKER),
            "zero-replica deferral must retain the surrounding closeout mutations",
        );

        let (replacement_id, replacement_bootstrap) =
            test_support_register_replica_for_file(&file, identity)
                .unwrap()
                .expect("replacement editor replica should register before deferred replay");
        let replayed = deferred_document_write_reconnect_content(&file, editor_base)
            .unwrap()
            .expect("replacement editor should receive the retained capture after registration");
        assert_eq!(replayed, captured_target);
        let replacement_replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            replacement_id,
            &replacement_bootstrap,
        )
        .unwrap();
        let replacement_text = replacement_replica.text();
        replacement_replica.apply_local_edit(0, replacement_text.len() as u32, &replayed);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &file,
            identity,
            &replacement_replica.encode_state(),
        )
        .unwrap()
        .expect("editor replay should publish the retained capture");
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_captured_replacement_bootstrap_current",
            )
            .unwrap(),
            captured_target,
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            editor_base,
            "replacement bootstrap ACK can precede the editor's disk save"
        );

        assert!(
            !settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_captured_replacement_bootstrap_test",
            )
            .unwrap(),
            "an editor ACK without its native save must retain the capture"
        );
        assert!(pending_document_write(&file).is_some());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), editor_base);
        assert!(
            !_dir.path().join(".agent-doc/patches").exists(),
            "settlement must retain the typed save intent without emitting a file signal"
        );

        std::fs::write(&file, &captured_target).expect("simulate the editor's native save");
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_captured_replacement_bootstrap_after_editor_save_test",
            )
            .unwrap(),
            "the exact native editor save should settle the retained capture"
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_captured_replacement_bootstrap_verify",
            )
            .unwrap(),
            captured_target,
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), captured_target);
    }

    #[test]
    fn retained_capture_settles_when_current_authority_already_contains_response() {
        let editor_base =
            "# Session\n\n<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n";
        let captured_response = "### Re: investigate\n\nFixed the retained closeout.\n";
        let captured_current = format!(
            "# Session\n\n<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            captured_response.trim_end()
        );
        let (_dir, file, _canonical) = temp_doc(&captured_current);

        ensure_deferred_document_write_intent(
            &file,
            &captured_current,
            editor_base,
            "editor_reconnect",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .unwrap();
        assert!(pending_document_write(&file).is_some());
        assert!(
            matches!(
                retained_write_settlement(&file, "preflight_retained_write_verdict_test")
                    .recovery_action(),
                agent_doc_state_backbone::retained_write::RecoveryAction::ReplayStranded { .. }
            ),
            "authority and disk agree while the retained content delta is absent",
        );

        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_current_authority_test",
            )
            .unwrap(),
            "current canonical and disk authority already prove the durable captured response"
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), captured_current);
    }

    #[test]
    fn superseded_capture_lineage_reaches_native_save_sink_for_current_response() {
        let editor_base =
            "# Session\n\n<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n";
        let captured_response = "### Re: investigate\n\nFixed the retained closeout.\n";
        let captured_current = format!(
            "# Session\n\n<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            captured_response.trim_end()
        );
        let (_dir, file, _canonical) = temp_doc(&captured_current);
        let identity = "test-superseded-capture-native-save-sink";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        retain_deferred_document_write_target(
            &file,
            &captured_current,
            editor_base,
            "superseded_capture_stale_lineage_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        retain_deferred_document_write_target(
            &file,
            editor_base,
            &captured_current,
            "superseded_capture_current_lineage_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        std::fs::write(&file, editor_base).expect("simulate disk trailing Lazily authority");

        assert!(
            !settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "superseded_capture_native_save_sink_test",
            )
            .unwrap(),
            "an unavailable editor save sink must retain the effect"
        );
        assert!(
            pending_document_write(&file).is_some(),
            "the retained effect must survive until disk proves the exact Lazily cut"
        );
        let ops = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(
            ops.contains("editor_projection_persistence_pending")
                && ops.contains("source=projected_captured_response_settlement")
                && ops.contains("driver=state_projection"),
            "current response authority must retain the persistence continuation: {ops}"
        );

        std::fs::write(&file, &captured_current).expect("simulate exact native editor save");
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "superseded_capture_native_save_sink_after_save_test",
            )
            .unwrap()
        );
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn retained_capture_replays_response_when_reconnect_target_dropped_it() {
        let editor_cut =
            "# Session\n\n<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n";
        let captured_response = "### Re: investigate\n\nRecovered from the durable capture.\n";
        let replayed_target =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                editor_cut,
                captured_response,
            )
            .expect("response cell should materialize over the editor cut");
        let (_dir, file, _canonical) = temp_doc(editor_cut);
        let identity = "test-retained-capture-missing-from-reconnect-target";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        ensure_deferred_document_write_intent(
            &file,
            editor_cut,
            editor_cut,
            "editor_reconnect_incomplete_target_test",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .expect("the historical incomplete reconnect target should be retained");
        assert!(pending_document_write(&file).is_some());

        let ack = project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            !settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_missing_response_replay_test",
            )
            .unwrap(),
            "foreground recovery should retain while the lazy controller projection is unacknowledged"
        );
        ack.join().unwrap();
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_missing_response_after_projection_ack_test",
            )
            .unwrap(),
            "the exact asynchronous projection receipt should release the retained capture",
        );
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_capture_missing_response_replay_current",
            )
            .unwrap(),
            replayed_target,
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), replayed_target);
    }

    #[test]
    fn captured_response_without_retained_intent_repairs_reversed_heading_and_body() {
        let editor_cut = concat!(
            "# Session\n\n",
            "<!-- agent:exchange -->\n",
            "Please investigate.\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#repair]\n\n",
            "Recovered exactly once.\n",
            "<!-- no-pending-capture -->\n",
            "### Re: do [#repair] — test (HEAD)\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let captured_response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: do [#repair] — test\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#repair]\n\n",
            "Recovered exactly once.\n",
            "<!-- /patch:exchange -->\n",
        );
        let repaired_target =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                editor_cut,
                captured_response,
            )
            .expect("reversed response cell should materialize");
        let (_dir, file, _canonical) = temp_doc(editor_cut);
        let identity = "test-captured-response-without-retained-intent";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        assert!(
            pending_document_write(&file).is_none(),
            "fixture must exercise the post-retirement recovery path"
        );

        let ack = project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            settle_projected_captured_response_through_authority(
                &file,
                captured_response,
                "captured_response_without_retained_intent_test",
            )
            .unwrap()
            .is_none(),
            "foreground recovery should wait for the response-cell delivery receipt"
        );
        ack.join().unwrap();
        assert_eq!(
            settle_projected_captured_response_through_authority(
                &file,
                captured_response,
                "captured_response_without_retained_intent_after_ack_test",
            )
            .unwrap()
            .as_deref(),
            Some(repaired_target.as_str()),
        );
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "captured_response_without_retained_intent_current",
            )
            .unwrap(),
            repaired_target,
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), repaired_target);
        assert_eq!(
            repaired_target.matches("Recovered exactly once.").count(),
            1
        );
    }

    #[test]
    fn retained_capture_supersedes_prior_cycle_reconnect_target() {
        let prior_cut = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n- prior head\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n",
        );
        let stale_prior_target = format!(
            "{}\n-->\n",
            prior_cut.replace("- prior head\n", "- stale prior head\n")
        );
        let current_cut = prior_cut.replace("- prior head\n", "- operator-owned current head\n");
        let captured_response = "### Re: investigate\n\nRecovered the current response.\n";
        let replayed_target =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                &current_cut,
                captured_response,
            )
            .expect("response cell should materialize over the current authority cut");
        let (_dir, file, _canonical) = temp_doc(&current_cut);
        let identity = "test-retained-capture-prior-cycle-reconnect-target";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        ensure_deferred_document_write_intent(
            &file,
            &stale_prior_target,
            prior_cut,
            "post_commit_reposition",
            DocumentWriteDeferredReason::ExtendPendingEditorReconnectTarget,
        )
        .expect("the prior cycle reconnect target should be retained");
        ensure_deferred_document_write_intent(
            &file,
            &replayed_target,
            &current_cut,
            "retained_captured_response_cell_replay",
            DocumentWriteDeferredReason::EditorDeliveryWorkerStale,
        )
        .expect("the active capture retry should follow the prior-cycle target");
        assert_eq!(pending_document_write_journal(&file).len(), 2);

        let ack = project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            !settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_prior_cycle_reconnect_target_test",
            )
            .unwrap(),
            "the newer durable capture remains retained until its lazy projection is acknowledged"
        );
        ack.join().unwrap();
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_prior_cycle_after_ack_test",
            )
            .unwrap(),
            "the exact receipt should supersede stale prior-cycle reconnect lineage",
        );
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_capture_prior_cycle_reconnect_target_current",
            )
            .unwrap(),
            replayed_target,
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), replayed_target);
        assert!(replayed_target.contains("operator-owned current head"));
        assert!(!replayed_target.contains("stale prior head"));
    }

    #[test]
    fn retained_capture_projects_redundant_terminal_debris_repair_from_authority_state() {
        let captured_response = "### Re: investigate\n\nThe retained response is durable.\n";
        let valid = format!(
            concat!(
                "# Session\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#reactive] Retain queue reactive projection\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:exchange -->\n",
                "{}",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:done -->\n",
                "<!-- /agent:done -->\n",
            ),
            captured_response,
        );
        let invalid = format!(
            concat!(
                "{}",
                "queue reactive projection\n",
                "- [ ] [#reactive] Retain queue reactive projection\n",
                "<!-- /agent:backlog -->\n",
            ),
            valid,
        );
        assert!(agent_doc_element::element::structural_corruption_reason(&invalid).is_some());
        let (_dir, file, _canonical) = temp_doc(&invalid);
        let identity = "test-retained-capture-terminal-debris-projection";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach to the invalid authority cut");
        ensure_deferred_document_write_intent(
            &file,
            &invalid,
            &valid,
            "retained_capture_terminal_debris_projection_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .expect("the valid retained target should remain durable");
        std::fs::write(&file, &valid).expect("simulate disk already holding the valid projection");

        let delivery =
            project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_terminal_debris_projection_test",
            )
            .unwrap(),
            "the authority projection should prune proven debris and settle the retained capture",
        );
        delivery.join().unwrap();

        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_capture_terminal_debris_projection_current",
            )
            .unwrap(),
            valid,
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), valid);
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn retained_capture_repairs_legacy_welded_queue_close_before_validation() {
        let captured_response = "### Re: investigate\n\nThe retained response is durable.\n";
        let valid = format!(
            concat!(
                "# Session\n\n",
                "<!-- agent:queue -->\n",
                "- do [#fresh-project-supervisor-log]: agent-doc start should create a configurable path to the supervisor log file. Directories should be created as needed.\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:exchange -->\n",
                "{}",
                "<!-- /agent:exchange -->\n",
            ),
            captured_response,
        );
        let invalid = valid.replace(
            "Directories should be created as needed.\n<!-- /agent:queue -->",
            "Directories should be created as needed.t should create a c<!-- /agent:queue -->",
        );
        assert!(agent_doc_element::element::structural_corruption_reason(&invalid).is_some());
        let (_dir, file, _canonical) = temp_doc(&invalid);
        let identity = "test-retained-capture-welded-queue-close";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach to the legacy invalid authority cut");

        // Seed the historical invalid intent directly: current builds reject it
        // at retention, but recovery must still consume databases created by
        // older builds.
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let event = agent_doc_state_backbone::StateEvent::new(
            "legacy-welded-queue-close",
            agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                document_hash,
                intent_id: "legacy-welded-queue-close".to_string(),
                expected_hash: agent_doc_hash::content_hash(&valid),
                expected_content: Some(valid.clone()),
                target_hash: agent_doc_hash::content_hash(&invalid),
                target_content: invalid.clone(),
                source: agent_doc_state_backbone::DocumentWriteSource::SerializedAtomicWrite,
                reason: DocumentWriteDeferredReason::EditorProjectionPending,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event_for_test(
            file.parent().unwrap(),
            &event,
        )
        .unwrap();

        let delivery =
            project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_welded_queue_close_test",
            )
            .unwrap(),
            "the historical welded target should repair through authority without force disk",
        );
        delivery.join().unwrap();

        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_capture_welded_queue_close_current",
            )
            .unwrap(),
            valid,
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), valid);
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn retained_capture_rebases_over_operator_save_without_preflight_or_recapture() {
        let editor_base = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n- original item\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\nPlease investigate.\n<!-- /agent:exchange -->\n",
        );
        let captured_response = "### Re: investigate\n\nFixed the retained closeout.\n";
        let captured_target = format!(
            concat!(
                "# Session\n\n",
                "<!-- agent:queue -->\n- original item\n<!-- /agent:queue -->\n\n",
                "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            ),
            captured_response.trim_end()
        );
        let operator_saved_cut = editor_base.replace(
            "- original item\n",
            "- original item\n- operator item saved before recovery\n",
        );
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-capture-operator-save";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_write_if_current_through_authority(
            &file,
            &captured_target,
            editor_base,
            "retained_capture_operator_save_zero_replica_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some()
        );

        let (replacement_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should bootstrap");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            let current = hub.canonical_text();
            hub.apply_local(
                replacement_id,
                0,
                current.chars().count() as u32,
                &operator_saved_cut,
            )
            .unwrap();
        })
        .unwrap();
        std::fs::write(&file, &operator_saved_cut)
            .expect("simulate the operator saving the newer editor cut");

        let ack = project_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            !settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_operator_save_rebase_test",
            )
            .unwrap(),
            "the foreground repair must not wait for or synthesize an editor projection receipt",
        );
        ack.join().unwrap();
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_operator_save_after_projection_ack_test",
            )
            .unwrap(),
            "the exact asynchronous projection receipt should settle the same retained capture",
        );

        let rebased = try_resolve_current_document_content(
            &file,
            "retained_capture_operator_save_rebased_current",
        )
        .unwrap();
        assert!(rebased.contains("operator item saved before recovery"));
        assert_eq!(rebased.matches("Fixed the retained closeout.").count(), 1);
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), rebased);
    }

    #[test]
    fn retained_non_capture_projection_waits_for_persistence_then_settles_exactly() {
        let editor_base = "# Session\n\n<!-- agent:queue -->\nold\n<!-- /agent:queue -->\n";
        let normalized_target =
            "# Session\n\n<!-- agent:queue -->\nnormalized\n<!-- /agent:queue -->\n";
        let (dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-non-capture-projection";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_write_if_current_through_authority(
            &file,
            normalized_target,
            editor_base,
            "retained_non_capture_zero_replica_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some()
        );
        assert!(
            format!("{err:#}").contains(RETAINED_FOR_RETRY_MARKER),
            "zero-replica deferral must retain the surrounding closeout mutations",
        );
        assert!(
            retained_write_blocks_session_closeout(
                &file,
                "retained_non_capture_zero_replica_gate_test",
            ),
            "an undelivered exact target must keep session-check interrupted",
        );
        let retained_target_hash = agent_doc_hash::content_hash(normalized_target);
        assert!(
            !retire_retained_projection_superseded_by_authority(
                &file,
                &retained_target_hash,
                "retained_non_capture_undelivered_base_test",
            )
            .unwrap(),
            "the unchanged editor-authoritative base is not a newer superseding cut",
        );
        assert!(
            pending_document_write(&file).is_some(),
            "failed supersession must preserve the exact retained target",
        );

        let (replacement_id, replacement_bootstrap) =
            test_support_register_replica_for_file(&file, identity)
                .unwrap()
                .expect("replacement editor replica should register before deferred replay");
        let replayed = deferred_document_write_reconnect_content(&file, editor_base)
            .unwrap()
            .expect("replacement editor should receive the retained non-capture target");
        assert_eq!(replayed, normalized_target);
        let replacement_replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            replacement_id,
            &replacement_bootstrap,
        )
        .unwrap();
        let replacement_text = replacement_replica.text();
        replacement_replica.apply_local_edit(0, replacement_text.len() as u32, &replayed);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &file,
            identity,
            &replacement_replica.encode_state(),
        )
        .unwrap()
        .expect("editor replay should publish the retained non-capture target");
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "retained_non_capture_replacement_bootstrap_current",
            )
            .unwrap(),
            normalized_target,
        );
        assert!(
            !settle_retained_non_capture_projection_through_authority(
                &file,
                "retained_non_capture_before_editor_save_test",
            )
            .unwrap(),
            "canonical convergence without the native disk save must remain retained",
        );
        assert!(
            !dir.path().join(".agent-doc/patches").exists(),
            "an exact retained non-capture target must not emit a file signal",
        );
        assert!(pending_document_write(&file).is_some());

        std::fs::write(&file, normalized_target).expect("simulate the editor's native save");
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "retained_non_capture_after_editor_save_test",
            )
            .unwrap(),
            "an exact non-capture projection must settle without response proof",
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), normalized_target);
    }

    #[test]
    fn merged_editor_cut_exact_target_waits_for_persistence_then_settles() {
        let editor_base = "# Session\n\n<!-- agent:queue -->\nold\n<!-- /agent:queue -->\n";
        let merged_target =
            "# Session\n\n<!-- agent:queue -->\nmerged target\n<!-- /agent:queue -->\n";
        let (dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-merged-editor-cut-exact-target";
        seed_reliable_sync_open(&file, identity);
        let (client_id, bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let replica_text = replica.text();
        replica.apply_local_edit(0, replica_text.len() as u32, merged_target);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &file,
            identity,
            &replica.encode_state(),
        )
        .unwrap()
        .expect("editor should publish the exact merged target");

        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            merged_target,
            "serialized_atomic_write_projection_rebase",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .unwrap();
        assert!(matches!(
            pending_document_write(&file)
                .expect("typed post-delivery rebase intent should be retained")
                .source,
            agent_doc_state_backbone::DocumentWriteSource::SerializedAtomicWriteProjectionRebase
        ));
        assert_eq!(
            try_resolve_current_document_content(&file, "merged_editor_cut_exact_target_current",)
                .unwrap(),
            merged_target,
        );

        assert!(
            !settle_retained_non_capture_projection_through_authority(
                &file,
                "merged_editor_cut_before_native_save",
            )
            .unwrap(),
            "the retained target must wait for the editor's native save effect",
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("editor_projection_persistence_pending")
                && log.contains("driver=state_projection")
        );
        assert!(!log.contains("proof=missing_operator_cut_lineage"));
        assert!(pending_document_write(&file).is_some());

        std::fs::write(&file, merged_target).expect("simulate the editor's native save effect");
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "merged_editor_cut_after_native_save",
            )
            .unwrap(),
            "the exact merged reconnect target should settle from reactive authority proof",
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), merged_target);
    }

    #[test]
    fn preflight_replays_stranded_retained_write_over_newer_operator_cut() {
        let editor_base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n",
            "- existing work\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Prompt\n\n",
            "Recover the interrupted operation.\n",
            "<!-- /agent:exchange -->\n",
        );
        let retained_target = editor_base.replace(
            "- existing work\n",
            "- existing work\n- recovered retained write\n",
        );
        let operator_cut = editor_base.replace(
            "Recover the interrupted operation.\n",
            "Recover the interrupted operation.\n\nOperator text written after the interruption.\n",
        );
        let (dir, file, _canonical) = temp_doc(&operator_cut);

        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            &retained_target,
            "serialized_atomic_write",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        assert!(pending_document_write(&file).is_some());

        let recovered = recover_retained_document_write_before_new_cycle(
            &file,
            RetainedWriteCycleBoundary::RegressionTest,
        )
        .unwrap();
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            recovered,
            "every preflight should replay a proven-stranded retained write; ops={ops}",
        );

        let settled = std::fs::read_to_string(&file).unwrap();
        assert!(settled.contains("- recovered retained write"));
        assert!(settled.contains("Operator text written after the interruption."));
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn retained_recovery_requests_editor_save_before_replaying_over_unsaved_authority() {
        let editor_base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n",
            "- existing work\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Investigate the retained operation.\n",
            "<!-- /agent:exchange -->\n",
        );
        let retained_target = editor_base.replace(
            "- existing work\n",
            "- existing work\n- retained operation\n",
        );
        let operator_cut = editor_base.replace(
            "Investigate the retained operation.\n",
            "Investigate the retained operation.\n\nUnsaved editor-owned text.\n",
        );
        let (dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-recovery-editor-save";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            &retained_target,
            "serialized_atomic_write",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            let current = hub.canonical_text();
            hub.apply_local(client_id, 0, current.chars().count() as u32, &operator_cut)
                .unwrap();
        })
        .unwrap();

        assert!(
            !recover_retained_document_write_before_new_cycle(
                &file,
                RetainedWriteCycleBoundary::RegressionTest,
            )
            .unwrap(),
            "without a native editor listener, recovery must stay retained",
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            editor_base,
            "recovery must not write around the editor",
        );
        assert!(pending_document_write(&file).is_some());
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops.contains("editor_projection_persistence_pending")
                && ops.contains("source=preflight_retained_write_recovery_test")
                && ops.contains("reason=editor_native_save_pending"),
            "recovery must retain its continuation until persistence is observed: {ops}",
        );
    }

    #[test]
    fn retained_active_prompt_marker_projection_retires_without_native_save() {
        let editor_base = concat!(
            "# Session\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Prompt heading\n",
            "Prompt prose\n",
            "<!-- /agent:exchange -->\n",
        );
        let marked_target = concat!(
            "# Session\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### ❯ 🚧 Prompt heading\n",
            "❯ Prompt prose\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-retained-active-prompt-marker-projection";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        let err = atomic_write_if_current_through_authority(
            &file,
            marked_target,
            editor_base,
            "serialized_atomic_write",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some()
        );
        assert!(pending_document_write(&file).is_some());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), editor_base);

        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "retained_active_prompt_marker_projection_test",
            )
            .unwrap(),
            "a marker-only serialized write must not wait for a native save"
        );
        assert!(pending_document_write(&file).is_none());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            editor_base,
            "retiring the cosmetic intent must not rewrite disk"
        );
    }

    fn compact_projection_test_document(
        timestamp: &str,
        queue_entry: &str,
        exchange_tail: &str,
    ) -> String {
        format!(
            concat!(
                "# Session\n\n",
                "<!-- agent:queue -->\n{}\n<!-- /agent:queue -->\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted. Content archived to `.agent-doc/archives/doc-hash-{}.md`*\n\n",
                "{}",
                "<!-- /agent:exchange -->\n",
            ),
            queue_entry, timestamp, exchange_tail
        )
    }

    #[test]
    fn newer_compact_projection_retires_stale_composed_journal_without_replay() {
        let stale_reposition = compact_projection_test_document(
            "20260717-225006",
            "- [ ] stale queue item",
            "old prompt\n",
        );
        let stale_composite = compact_projection_test_document(
            "20260717-225007",
            "- [ ] stale queue item\n- [ ] another stale item",
            "old prompt\n",
        );
        let current = compact_projection_test_document(
            "20260717-225039",
            "- [x] retained current item",
            "❯ 🚧 current prompt\n",
        );
        let (_dir, file, _canonical) = temp_doc(&current);
        let identity = "test-superseded-compact-projection";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach at the newer compact projection");
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for (event_id, intent_id, target, source, reason) in [
            (
                "stale-compact-reposition",
                "intent-stale-reposition",
                stale_reposition.as_str(),
                "post_commit_reposition",
                DocumentWriteDeferredReason::ExtendPendingEditorReconnectTarget,
            ),
            (
                "stale-compact-composite",
                "intent-stale-composite",
                stale_composite.as_str(),
                "serialized_atomic_write",
                DocumentWriteDeferredReason::EditorProjectionPending,
            ),
        ] {
            let event = agent_doc_state_backbone::StateEvent::new(
                event_id,
                agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                    document_hash: document_hash.clone(),
                    intent_id: intent_id.to_string(),
                    expected_hash: agent_doc_hash::content_hash(&current),
                    expected_content: Some(current.clone()),
                    target_hash: agent_doc_hash::content_hash(target),
                    target_content: target.to_string(),
                    source: agent_doc_state_backbone::DocumentWriteSource::from(source),
                    reason,
                },
            );
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                file.parent().unwrap(),
                &event,
            )
            .unwrap();
        }

        assert_eq!(pending_document_write_journal(&file).len(), 2);
        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "superseded_compact_projection_test",
            )
            .unwrap()
        );
        assert!(pending_document_write_journal(&file).is_empty());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), current);
        assert!(
            !std::fs::read_to_string(&file)
                .unwrap()
                .contains("stale queue item")
        );
    }

    #[test]
    fn newer_compact_projection_does_not_retire_unrelated_write_source() {
        let stale = compact_projection_test_document(
            "20260717-225006",
            "- [ ] stale queue item",
            "old prompt\n",
        );
        let current = compact_projection_test_document(
            "20260717-225039",
            "- [x] current item",
            "current prompt\n",
        );
        let (_dir, file, _canonical) = temp_doc(&current);
        let identity = "test-unrelated-compact-projection";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach at the newer compact projection");
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let event = agent_doc_state_backbone::StateEvent::new(
            "unrelated-compact-target",
            agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                document_hash,
                intent_id: "intent-unrelated-source".to_string(),
                expected_hash: agent_doc_hash::content_hash(&current),
                expected_content: Some(current),
                target_hash: agent_doc_hash::content_hash(&stale),
                target_content: stale,
                source: agent_doc_state_backbone::DocumentWriteSource::from("queue_mutation"),
                reason: DocumentWriteDeferredReason::EditorProjectionPending,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event_for_test(
            file.parent().unwrap(),
            &event,
        )
        .unwrap();

        assert!(
            !settle_retained_non_capture_projection_through_authority(
                &file,
                "unrelated_compact_projection_test",
            )
            .unwrap()
        );
        assert_eq!(pending_document_write_journal(&file).len(), 1);
    }

    #[test]
    fn live_editor_projection_refuses_historical_divergence_without_retained_intent() {
        let baseline = "# Session\n\n<!-- agent:queue -->\nold\n<!-- /agent:queue -->\n";
        let editor_target =
            "# Session\n\n<!-- agent:queue -->\noperator cut\n<!-- /agent:queue -->\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-live-editor-projection-historical-ack";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();
        let err = atomic_write_if_current_through_authority(
            &file,
            editor_target,
            baseline,
            "live_editor_projection_historical_ack_test",
        )
        .unwrap_err();
        assert!(
            err.downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_some()
        );
        assert!(pending_document_write(&file).is_some());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should register before deferred replay");
        let replayed = deferred_document_write_reconnect_content(&file, baseline)
            .unwrap()
            .expect("replacement editor should receive the retained target after registration");
        assert_eq!(replayed, editor_target);

        clear_deferred_document_write_intent(
            &file,
            &agent_doc_hash::content_hash(editor_target),
            "simulate_historical_ack_only_convergence",
        )
        .unwrap();
        assert!(pending_document_write(&file).is_none());

        let err = adopt_verified_editor_text_through_relay_authority(
            &file,
            &replayed,
            "live_editor_projection_historical_ack_post_register",
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("refusing editor receipt that diverges from controller canonical"),
            "a recycled editor must not promote a historical whole-buffer projection: {err:#}",
        );
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "live_editor_projection_historical_ack_current",
            )
            .unwrap(),
            baseline,
            "the controller canonical must survive a recycled editor's stale projection",
        );
        assert!(
            settle_live_editor_projection_through_authority(
                &file,
                "live_editor_projection_before_native_save_test",
            )
            .unwrap(),
            "the unchanged controller projection is already exact on disk",
        );
        assert!(
            !dir.path().join(".agent-doc/patches").exists(),
            "a historical editor receipt must not emit file IPC",
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            baseline,
            "the stale editor projection must not delete canonical queue content",
        );
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn exact_cp_model_never_substitutes_for_editor_delivery_projection() {
        assert!(!editor_save_authority_is_sufficient(
            "canonical",
            "canonical",
            1,
            false,
        ));
        assert!(!editor_save_authority_is_sufficient(
            "canonical",
            "canonical",
            2,
            false,
        ));
        assert!(editor_save_authority_is_sufficient(
            "canonical",
            "canonical",
            2,
            true,
        ));
        assert!(!editor_save_authority_is_sufficient(
            "different",
            "canonical",
            1,
            false,
        ));
    }

    #[test]
    fn semantic_rebase_projection_does_not_settle_from_exact_bytes_alone() {
        let editor_base = "# Session\n\n<!-- agent:queue -->\n- do [#keep]\n- do [#deleted]\n<!-- /agent:queue -->\n";
        let stale_target = "# Session\n\n<!-- agent:queue -->\n- do [#keep]\n- do [#deleted]\n- do [#agent]\n<!-- /agent:queue -->\n";
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-semantic-rebase-non-capture-projection";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            stale_target,
            "semantic_rebase_exact_bytes_test",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .unwrap();
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(
                client_id,
                0,
                editor_base.chars().count() as u32,
                stale_target,
            )
            .unwrap();
        })
        .unwrap();
        std::fs::write(&file, stale_target).expect("simulate an exact but stale editor save");

        assert!(
            !settle_retained_non_capture_projection_through_authority(
                &file,
                "semantic_rebase_exact_bytes_settlement_test",
            )
            .unwrap(),
            "semantic rebase lineage needs operator-cut proof, not byte equality",
        );
        let pending = pending_document_write(&file).expect("semantic intent must remain retained");
        assert_eq!(
            pending.reason,
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
        );
    }

    #[test]
    fn semantic_response_projection_settles_over_exact_operator_cut_without_lineage() {
        let editor_base = concat!(
            "<!-- agent:exchange -->\n",
            "❯ Original prompt.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#keep]\n- do [#deleted]\n<!-- /agent:queue -->\n",
        );
        let response_target = editor_base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: Original prompt. — gpt-5\n\nAnswered.\n<!-- agent:boundary:base -->",
        );
        let operator_cut = response_target
            .replace(
                "❯ Original prompt.\n",
                "❯ Original prompt.\n❯ Prompt typed during delivery.\n",
            )
            .replace("- do [#deleted]\n", "");
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-semantic-response-no-lineage";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");

        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            &response_target,
            "semantic_response_no_lineage_test",
            DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
        )
        .unwrap();
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(
                client_id,
                0,
                editor_base.chars().count() as u32,
                &operator_cut,
            )
            .unwrap();
        })
        .unwrap();
        std::fs::write(&file, &operator_cut).expect("simulate the exact native editor save");

        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "semantic_response_no_lineage_settlement_test",
            )
            .unwrap(),
            "a semantic response target can settle over the current operator cut without whole-document lineage",
        );
        let settled = try_resolve_current_document_content(
            &file,
            "semantic_response_no_lineage_settled_current",
        )
        .unwrap();
        assert_eq!(settled, operator_cut);
        assert!(settled.contains("Prompt typed during delivery."));
        assert!(!settled.contains("#deleted"));
        assert_eq!(settled.matches("### Re: Original prompt.").count(), 1);
        assert_eq!(settled.matches("agent:boundary:").count(), 1);
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn ack_pending_response_projection_settles_over_newer_canonical_cut() {
        let editor_base = concat!(
            "<!-- agent:exchange -->\n",
            "❯ Original prompt.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#deleted]\n<!-- /agent:queue -->\n",
        );
        let response_target = editor_base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: Original prompt. — gpt-5\n\nAnswered.\n<!-- agent:boundary:base -->",
        );
        let newer_canonical = response_target
            .replace(
                "❯ Original prompt.\n",
                "❯ Original prompt.\n❯ Prompt typed after socket delivery.\n",
            )
            .replace("- do [#deleted]\n", "");
        let (_dir, file, _canonical) = temp_doc(editor_base);
        let identity = "test-ack-pending-semantic-response-settlement";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        ensure_deferred_document_write_intent(
            &file,
            editor_base,
            &response_target,
            "ack_pending_semantic_response_test",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(
                client_id,
                0,
                editor_base.chars().count() as u32,
                &newer_canonical,
            )
            .unwrap();
        })
        .unwrap();
        std::fs::write(&file, &newer_canonical)
            .expect("simulate the newer canonical cut's native save");

        assert!(
            settle_retained_non_capture_projection_through_authority(
                &file,
                "ack_pending_semantic_response_settlement_test",
            )
            .unwrap(),
            "an ACK-pending response intent must settle semantically over a newer canonical cut",
        );
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "ack_pending_semantic_response_settled_current",
            )
            .unwrap(),
            newer_canonical,
        );
        assert!(pending_document_write(&file).is_none());
    }

    #[test]
    fn committed_transient_split_without_retained_lineage_retains_until_ack() {
        let stale_disk = "# Session\n\ncomplete response\n<!-- agent:boundary:old -->\n";
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let (_dir, file, _canonical) = temp_doc(stale_disk);
        let identity = "test-historical-committed-transient-split";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), stale_disk);
        assert!(pending_document_write(&file).is_none());

        let ack = project_next_crdt_delivery(file.clone(), identity);
        assert!(
            !settle_retained_committed_projection_through_authority(
                &file,
                committed,
                stale_disk,
                "historical_committed_transient_split_test",
            )
            .unwrap(),
            "the historical committed target stays pending until its controller projection receipt",
        );
        ack.join().unwrap();
        assert!(
            settle_retained_committed_projection_through_authority(
                &file,
                committed,
                stale_disk,
                "historical_committed_transient_split_after_ack_test",
            )
            .unwrap(),
            "the exact asynchronous receipt should settle the historical transient split",
        );

        assert_eq!(std::fs::read_to_string(&file).unwrap(), committed);
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "historical_committed_transient_split_verify",
            )
            .unwrap(),
            committed,
        );
    }

    #[test]
    fn force_disk_is_a_pending_candidate_and_reappearing_editor_supersedes_it() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: agent\n\nresponse\n";
        let (dir, file, _canonical) = temp_doc(baseline);

        atomic_write_force_disk_through_authority(&file, target).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);

        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(dir.path())
                .unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let pending = projection
            .document(&document_hash)
            .and_then(|document| document.document.pending_external_disk.as_ref())
            .expect("force-disk must retain an independent external-disk candidate");
        assert_eq!(pending.expected_content.as_deref(), Some(baseline));
        assert_eq!(pending.target_content, target);
        assert_eq!(pending.source, "force_disk");

        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline).unwrap(),
            None,
            "unchanged editor must wait for the user's cache-conflict decision"
        );
        assert!(pending_external_disk_candidate(&file).is_some());

        let editor_with_unsaved_note = format!("{baseline}\noperator note after relay loss\n");
        assert_eq!(
            deferred_document_write_reconnect_content(&file, &editor_with_unsaved_note).unwrap(),
            None,
            "the editor already owns its newer bytes; reconnect must not merge the disk candidate"
        );
        assert!(pending_external_disk_candidate(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
    }

    #[test]
    fn external_disk_decision_is_independent_replaced_exactly_and_cleared_by_save() {
        let baseline = "# Session\n\nbody\n";
        let response = "# Session\n\nbody\n\n### Re: agent\n\nresponse\n";
        let disk_one = "# Session\n\nexternal one\n";
        let disk_two = "# Session\n\nexternal two\n";
        let (_dir, file, _canonical) = temp_doc(baseline);

        retain_deferred_document_write_target(
            &file,
            baseline,
            response,
            "independent_agent_response",
            DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        retain_external_disk_candidate(&file, baseline, disk_one, "external_one").unwrap();
        retain_external_disk_candidate(&file, baseline, disk_two, "external_two").unwrap();

        assert_eq!(
            pending_document_write(&file)
                .expect("agent response lineage")
                .target_content,
            response
        );
        let external = pending_external_disk_candidate(&file).expect("external candidate");
        assert_eq!(external.expected_content.as_deref(), Some(baseline));
        assert_eq!(external.target_content, disk_two);
        assert!(!external.target_content.contains("external one"));

        std::fs::write(&file, baseline).unwrap();
        assert!(
            clear_pending_external_disk_decision_on_editor_save(
                &file,
                baseline,
                "editor_save_test"
            )
            .unwrap()
        );
        assert!(pending_external_disk_candidate(&file).is_none());
        assert!(pending_document_write(&file).is_some());
    }

    #[test]
    fn accepted_disk_candidate_clears_only_after_editor_replica_propagates() {
        let baseline = "# Session\n\nbody\n";
        let disk = "# Session\n\naccepted external edit\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        retain_external_disk_candidate(&file, baseline, disk, "external_accept").unwrap();

        assert_eq!(
            deferred_document_write_reconnect_content(&file, disk).unwrap(),
            Some(disk.to_string())
        );
        assert!(pending_external_disk_candidate(&file).is_some());
        assert!(
            clear_pending_external_disk_decision_after_editor_propagation(
                &file,
                disk,
                "editor_propagated_test"
            )
            .unwrap()
        );
        assert!(pending_external_disk_candidate(&file).is_none());
    }

    #[test]
    fn unproven_multi_editor_cut_stays_pending_until_explicit_editor_action() {
        let baseline = "# Session\n\nbody\n";
        let disk = "# Session\n\nexternal edit\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        retain_external_disk_candidate_without_editor_cut(&file, disk, "multi_editor_sync")
            .unwrap();

        let pending = pending_external_disk_candidate(&file).expect("external candidate");
        assert!(pending.expected_hash.is_empty());
        assert!(pending.expected_content.is_none());
        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline).unwrap(),
            None,
            "an arbitrary reconnecting replica must not resolve an unproven editor cut"
        );
        assert!(
            clear_pending_external_disk_decision_on_editor_edit(
                &file,
                "# Session\n\nnew operator edit\n",
                "operator_edit_test"
            )
            .unwrap()
        );
        assert!(pending_external_disk_candidate(&file).is_none());
    }

    #[test]
    fn last_editor_close_clears_external_disk_decision_for_disk_fallback() {
        let baseline = "# Session\n\nbody\n";
        let disk = "# Session\n\nexternal edit\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        retain_external_disk_candidate(&file, baseline, disk, "external_close").unwrap();

        assert!(
            clear_pending_external_disk_decision_on_last_editor_close(
                &file,
                "last_editor_close_test"
            )
            .unwrap()
        );
        assert!(pending_external_disk_candidate(&file).is_none());
    }

    #[test]
    fn force_disk_aborts_when_editor_replica_reconnects_before_mutation() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: agent\n\nresponse\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        let _force_disk_authority_scope = begin_force_disk_authority_scope(
            &file,
            "force_disk_reconnect_fence_test_authorization",
        )
        .unwrap();
        let identity = "test-force-disk-reconnect-fence";
        seed_reliable_sync_open(&file, identity);
        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach before the mutation fence");

        let err = atomic_write_force_disk_through_authority(&file, target).unwrap_err();

        assert!(
            err.downcast_ref::<ForceDiskAuthorityChanged>().is_some(),
            "force-disk must fail with the typed authority-change error: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
        assert!(pending_document_write(&file).is_none());
    }

    fn seed_visible_write_commit_candidate_proof(
        file: &std::path::Path,
        patch_id: &str,
        candidate_content: &str,
        source: &str,
    ) {
        let project_root = agent_doc_project_root_io::project_root_containing(file)
            .expect("test document should live under .agent-doc project root");
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let candidate_hash = agent_doc_hash::content_hash(
            &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                candidate_content,
            ),
        );
        let model_revision = 1;
        for (event_id, fact) in [
            (
                format!("test-visible-write-applied-{patch_id}-{candidate_hash}"),
                agent_doc_state_backbone::StateFact::EditorPatchApplied {
                    document_hash: document_hash.clone(),
                    patch_id: patch_id.to_string(),
                    actor_generation: model_revision,
                },
            ),
            (
                format!("test-visible-write-candidate-{patch_id}-{candidate_hash}"),
                agent_doc_state_backbone::StateFact::VisibleWriteCommitCandidateObserved {
                    document_hash: document_hash.clone(),
                    patch_id: patch_id.to_string(),
                    model_revision,
                    editor_visible_hash: candidate_hash.clone(),
                    commit_candidate_hash: candidate_hash.clone(),
                    commit_candidate_content: Some(candidate_content.to_string()),
                    source: source.to_string(),
                },
            ),
        ] {
            let event = agent_doc_state_backbone::StateEvent::new(event_id, fact);
            agent_doc_controller_io::project_controller::append_state_event_for_test(
                &project_root,
                &event,
            )
            .expect("append visible-write proof event");
        }
    }

    #[test]
    fn durable_buffer_state_reads_cp_crdt_current_text() {
        let disk = "## Queue\n- do [#a]\n";
        let buffer = "## Queue\n- do [#a]\n- do [#rtwatch]\n";
        let (_dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-cp-authority");
        let (_client_id, _bootstrap) =
            test_support_register_replica_for_file(&file, "test-cp-authority")
                .unwrap()
                .expect("editor-attached replica registers");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            let client_id =
                agent_doc_document_realtime::crdt_relay::mint_client_id("test-cp-authority");
            hub.apply_local(client_id, 0, disk.chars().count() as u32, buffer)
                .unwrap();
        })
        .unwrap();

        let state = durable_buffer_state(&file, disk).expect("CP relay buffer wins");
        assert_eq!(state.content, buffer);
        let r = try_resolve_current_doc(&file, disk).unwrap();
        assert_eq!(
            r.authority,
            agent_doc_document_realtime::DocAuthority::EditorBuffer
        );
        assert!(r.content.contains("#rtwatch"));
    }

    #[test]
    fn durable_buffer_state_none_when_no_editor_feed() {
        // No CP model (no editor attached) → disk is the only source.
        let disk = "plain disk body\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, disk).unwrap();
        assert!(durable_buffer_state(&file, disk).is_none());
        assert_eq!(
            try_resolve_current_doc(&file, disk).unwrap().authority,
            agent_doc_document_realtime::DocAuthority::Disk
        );
    }

    /// An attached editor with an exhausted missing-replica recovery fails closed.
    /// Disk may contain an older saved projection, so treating it as current text
    /// can resurrect operator-deleted nodes before the separate commit guard runs.
    #[test]
    fn current_resolve_refuses_disk_when_editor_model_is_still_missing() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-editor-authority-message");

        let err = try_resolve_current_doc_from_file(&file)
            .expect_err("an attached editor must block disk read authority");
        assert!(
            format!("{err:#}").contains("disk read authority is refused"),
            "unexpected refusal: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("realtime_doc_resolve_disk_read_refused")
                && log.contains("reason=missing_replica")
                && log.contains("upstream_replica_refresh_exhausted"),
            "the refusal must name the exhausted recovery:\n{log}"
        );
        assert!(
            !log.contains("realtime_doc_resolve_disk_read_fallback"),
            "an attached editor must never descend to disk:\n{log}"
        );
    }

    #[test]
    fn authority_observations_coalesce_only_consecutive_duplicates() {
        use agent_doc_state_backbone::DocumentAuthority::{DiskReplica, EditorRelay};

        let (dir, file, _canonical) = temp_doc("authority transition\n");
        record_document_authority(
            &file,
            "disk_probe",
            DiskReplica,
            "headless",
            Some("disk-hash".to_string()),
            None,
        );
        record_document_authority(
            &file,
            "disk_probe",
            DiskReplica,
            "headless",
            Some("disk-hash".to_string()),
            None,
        );
        record_document_authority(
            &file,
            "editor_probe",
            EditorRelay,
            "attached",
            Some("editor-hash".to_string()),
            Some("editor-1".to_string()),
        );
        record_document_authority(
            &file,
            "disk_probe",
            DiskReplica,
            "headless",
            Some("disk-hash".to_string()),
            None,
        );

        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let authority_events =
            agent_doc_sqlite::state_store::load_state_events_from_db(&conn, Some(&document_hash))
                .unwrap()
                .into_iter()
                .filter(|event| event.fact_type == "document_authority_observed")
                .collect::<Vec<_>>();
        assert_eq!(
            authority_events.len(),
            3,
            "one duplicate should coalesce while disk/editor/disk transitions remain durable"
        );
        assert!(authority_events[0].payload_json.contains("disk_probe"));
        assert!(authority_events[1].payload_json.contains("editor_probe"));
        assert!(authority_events[2].payload_json.contains("disk_probe"));
    }

    #[test]
    fn current_resolve_rebuilds_editor_model_when_sync_pending() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        let owner = "test-editor-authority-sync-pending";
        seed_reliable_sync_open(&file, owner);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, owner)
            .unwrap()
            .expect("editor-attached replica registers");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.local_edit(client_id, 0, 0, "LIVE ").unwrap();
            assert!(
                !hub.canonical_text().starts_with("LIVE "),
                "test setup should leave the relay in sync-pending state"
            );
        })
        .unwrap();

        // Rebuild BEFORE descent: an available editor buffer is rebuilt into a
        // live model rather than dropping a tier. This is the better outcome —
        // the editor's unconverged edit stays authoritative instead of the read
        // silently seeing past it to a stale disk image.
        let resolved = try_resolve_current_doc_from_file(&file).unwrap();
        assert_eq!(
            resolved.authority,
            agent_doc_document_realtime::DocAuthority::EditorBuffer,
            "a sync-pending editor with an available buffer is REBUILT, not demoted to disk"
        );
        // Resolution is still read-only: nothing is written to the document.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("realtime_doc_resolve_editor_model_rebuild_attempt")
                && log.contains("reason=sync_pending"),
            "the rebuild attempt must be recorded:\n{log}"
        );
        assert!(
            !log.contains("realtime_doc_resolve_disk_read_fallback"),
            "a successful rebuild must NOT fall through to the disk tier:\n{log}"
        );
    }

    /// The precedence chain has exactly TWO tiers — editor buffer, then disk.
    /// There is no git tier: a read never reaches for committed content.
    #[test]
    fn read_fallback_has_no_git_tier() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-no-git-tier");

        let err = try_resolve_current_doc_from_file(&file)
            .expect_err("an attached editor stops before every lower authority tier");
        assert!(format!("{err:#}").contains("disk read authority is refused"));

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.to_lowercase().contains("tier=git") && !log.contains("authority=git"),
            "read resolution must never descend to a git tier:\n{log}"
        );
        assert!(!log.contains("realtime_doc_resolve_disk_read_fallback"));
    }

    #[test]
    fn current_resolve_reads_reliable_sync_close_after_deregister() {
        let relay_text = "\
## Exchange

<!-- agent:exchange patch=append -->
### Session Summary
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let disk_prompt = "\
## Exchange

<!-- agent:exchange patch=append -->
### Session Summary
- `make check` passed.
/goal keep the saved disk prompt visible
<!-- agent:boundary:new -->
<!-- /agent:exchange -->
";
        let (dir, file, _canonical) = temp_doc(relay_text);
        let owner = "test-zero-live-current-resolve";
        seed_reliable_sync_open(&file, owner);
        test_support_register_replica_for_file(&file, owner)
            .unwrap()
            .expect("editor-attached replica registers (marks reactive authority open)");
        assert!(
            test_support_deregister_replica_for_file(&file, owner).unwrap(),
            "editor deregister (close event) marks the reactive authority closed"
        );
        seed_reliable_sync_close(&file, owner);
        std::fs::write(&file, disk_prompt).unwrap();

        let resolved = try_resolve_current_doc_from_file(&file)
            .expect("deregistered editor resolves to disk via the reactive authority");
        assert_eq!(
            resolved.authority,
            agent_doc_document_realtime::DocAuthority::Disk,
            "reliable-sync authority is closed (editor deregistered) → disk"
        );
        assert_eq!(resolved.reason, "editor_absent");
        assert_eq!(resolved.content, disk_prompt);
        assert!(
            resolved
                .content
                .contains("/goal keep the saved disk prompt visible")
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("crdt_relay_hub_evicted")
                && log.contains("realtime_doc_resolve authority=disk reason=editor_absent"),
            "closed-editor disk demotion should be auditable:\n{log}"
        );
    }

    /// `#preflightprojpass`: every `realtime_doc_resolve` must name the CALLER.
    ///
    /// The whole point of that item is "re-measure first" — find which callers
    /// still resolve the full document redundantly. The instrument could not
    /// answer it: four emit sites hardcoded `source=crdt_relay` (which is what
    /// *answered* the resolve, not who asked) and two omitted `source`
    /// entirely, so 100% of resolutions were unattributable. The item was
    /// re-narrowed twice off that reading and named `crdt_relay` "the largest
    /// single source" both times — it is a constant, and it was always going to
    /// win. Measured 2026-08-09 after 0.35.195: 489 of 494 resolutions carried
    /// that constant, and the sources the item queued up to check next did not
    /// appear at all.
    #[test]
    fn every_resolve_names_the_caller_not_just_what_answered_it() {
        let (dir, file, _canonical) = temp_doc("<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n");
        let owner = "test-resolve-attribution";
        seed_reliable_sync_open(&file, owner);
        test_support_register_replica_for_file(&file, owner)
            .unwrap()
            .expect("editor-attached replica registers");
        assert!(test_support_deregister_replica_for_file(&file, owner).unwrap());
        seed_reliable_sync_close(&file, owner);

        let caller = "unmistakable_caller_under_test";
        try_resolve_current_document_content(&file, caller)
            .expect("a deregistered editor still resolves");

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let resolves: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("realtime_doc_resolve authority="))
            .collect();
        assert!(
            !resolves.is_empty(),
            "the resolve must be logged at all:\n{log}"
        );
        for line in &resolves {
            assert!(
                line.contains(&format!("source={caller}")),
                "a resolve that cannot name its caller cannot be measured: {line}"
            );
            assert!(
                !line.contains("source=crdt_relay"),
                "`crdt_relay` is what answered the resolve, not who asked — it must not \
                 occupy the caller field: {line}"
            );
        }
    }

    /// The complement of the runtime test above, and the one that actually
    /// covers the defect: SIX emit sites, one runtime path per test.
    ///
    /// `#preflightprojpass`: four of them hardcoded `source=crdt_relay` and two
    /// omitted `source` altogether. A runtime test reaches whichever branch its
    /// fixture happens to take — the first draft of the test above passed
    /// unchanged when `resolve_disk_only_current_doc` was mutated back to the
    /// constant, because that fixture resolves through the detached branch. A
    /// measurement is only as good as its worst-attributed site, so the
    /// property has to be checked across all of them at once.
    #[test]
    fn no_resolve_emit_site_hardcodes_the_caller_field() {
        let source = include_str!("lib.rs");
        // Guard the shipped emit sites only. This module asserts on the very
        // strings it forbids, so scanning itself is how the first draft
        // reported two findings against its own assertions.
        let source = source
            .split_once("\n#[cfg(test)]\nmod tests {")
            .map(|(before, _)| before)
            .unwrap_or(source);
        let lines: Vec<&str> = source.lines().collect();
        let mut checked = 0usize;
        let mut findings = Vec::new();

        for (index, line) in lines.iter().enumerate() {
            if !line.contains("OpsLogEvent::RealtimeDocResolve,") {
                continue;
            }
            // Walk back to the `format!(` that owns this argument list; the
            // lines between are the format string literal. Bounded, so a site
            // that does not match this shape is skipped rather than swallowing
            // the file into one bogus finding.
            let Some(start) = (index.saturating_sub(8)..index)
                .rev()
                .find(|candidate| lines[*candidate].contains("format!("))
            else {
                continue;
            };
            let literal: String = lines[start + 1..index].concat();
            if literal.trim().is_empty() {
                continue;
            }
            checked += 1;
            if !literal.contains("source={}") {
                findings.push(format!(
                    "line {}: emits a resolve with no `source={{}}` caller field: {literal}",
                    index + 1
                ));
            }
            if literal.contains("source=crdt_relay") {
                findings.push(format!(
                    "line {}: pins the caller field to a constant: {literal}",
                    index + 1
                ));
            }
        }

        assert!(
            checked >= 4,
            "guard found only {checked} resolve emit sites — it stopped matching the code it guards"
        );
        assert!(
            findings.is_empty(),
            "`#preflightprojpass`: every `realtime_doc_resolve` must name its CALLER in \
             `source=`. `crdt_relay` / `detached` / `disk_only` describe what ANSWERED the \
             resolve and belong in `answered_by=`. A site that pins `source=` to a constant \
             makes every resolution it emits unattributable, which is how this item was \
             re-narrowed twice onto a value that could not lose.\n\n{}",
            findings.join("\n")
        );
    }

    // The visible-write reconcile consumes the same reliable-sync Close authority as the
    // current-document resolver after the editor deregisters its replica.
    #[test]
    fn visible_write_reconcile_reads_reliable_sync_close_after_deregister() {
        let relay_text = "\
<!-- agent:exchange patch=append -->
### Session Summary
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let disk_prompt = "\
<!-- agent:exchange patch=append -->
### Session Summary
- `make check` passed.
/goal keep the saved disk prompt visible
<!-- agent:boundary:new -->
<!-- /agent:exchange -->
";
        let (dir, file, _canonical) = temp_doc(relay_text);
        let owner = "test-zero-live-visible-write";
        seed_reliable_sync_open(&file, owner);
        test_support_register_replica_for_file(&file, owner)
            .unwrap()
            .expect("editor-attached replica registers (marks reactive authority open)");
        assert!(
            test_support_deregister_replica_for_file(&file, owner).unwrap(),
            "editor deregister (close event) marks the reactive authority closed"
        );
        seed_reliable_sync_close(&file, owner);
        std::fs::write(&file, disk_prompt).unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &file,
            "test_zero_live_visible_write",
            relay_text,
            None,
        )
        .expect("deregistered editor visible-write reconciles against disk");
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, disk_prompt);
            }
            VisibleWriteReconcile::Clean => {
                panic!("expected disk drift from the saved prompt after editor deregister")
            }
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_disk_drift_reconcilable"),
            "closed-editor visible-write disk reconcile should be auditable:\n{log}"
        );
    }

    #[test]
    fn visible_write_guard_recovers_after_delayed_replica_registration() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        let owner = "test-visible-write-model-ensure";
        seed_reliable_sync_open(&file, owner);

        let file_for_register = file.clone();
        let register = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            test_support_register_replica_for_file(
                &file_for_register,
                "intellij:visible-write-ensure",
            )
            .expect("delayed register should not fail")
            .expect("editor-attached register should allocate model");
        });

        let reconcile = guard_visible_write_reconcile_with_target(
            &file,
            "test_visible_write_ensure",
            disk,
            None,
        )
        .expect("visible-write guard should recover through document-model ensure");
        register.join().unwrap();
        assert!(matches!(reconcile, VisibleWriteReconcile::Clean));

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("status=editor_attached_model_missing")
                && ops_log.contains("crdt_replica_register")
                && ops_log.contains("visible_write_crdt_current_clean"),
            "visible-write guard should recover after delayed replica registration:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("visible_write_editor_authority_unavailable"),
            "visible-write guard must not fail before shared recovery:\n{ops_log}"
        );
    }

    #[test]
    fn visible_write_guard_blocks_when_current_changed_after_merge() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

<!--
scratch
-->
";
        std::fs::write(&doc, expected).unwrap();
        std::fs::write(
            &doc,
            expected.replace("scratch", "scratch\nstill typing this line"),
        )
        .unwrap();

        let err = guard_visible_write_expected_current(&doc, "test_current_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("document changed after the response merge was computed")
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_current_changed"));
        assert!(log.contains("visible_write_deferred_current_changed"));
    }

    #[test]
    fn visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift() {
        // #nm1x: the editor reported a buffer that diverges from `expected` but
        // *matches the current on-disk content* (an independent document edit the
        // editor already saved). That is not a pending unsaved user edit, so the
        // guard must not fail closed — it reports the reconcilable DiskDrifted case
        // instead, letting the response re-merge against the fresh disk content.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        // Disk carries an independent queue edit not present in `expected`.
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#sibling]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, &drifted).unwrap();
        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_editor_matches_disk",
            expected,
            None,
        )
        .unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("source=test_editor_matches_disk"),
            "marker must identify the write source: {log}"
        );
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
        assert!(!log.contains("visible_write_live_buffer_matches_disk"));
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "must not record a fail-closed live-buffer block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_reports_clean_when_disk_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, expected).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_clean", expected, None).unwrap();
        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
    }

    #[test]
    fn visible_write_reconcile_accepts_proven_commit_candidate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
❯ Fix the repair retry
<!-- /agent:exchange -->
";
        let target = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: Fix the repair retry - gpt-5\n\nDone.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, expected).unwrap();
        seed_visible_write_commit_candidate_proof(
            &doc,
            "patch-target-proof",
            &target,
            "test_live_buffer_matches_target",
        );

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_live_buffer_matches_target",
            expected,
            Some(&target),
        )
        .unwrap();

        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            expected,
            "the guard only classifies proof; the caller still owns the disk write"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(!log.contains("visible_write_commit_candidate_applied_reconcile"));
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "a proven commit candidate must not trip the drift guard:\n{log}"
        );
    }

    #[test]
    fn visible_write_reconcile_reports_disk_drift_without_live_buffer_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        // Disk grew under us with a foreign agent-doc append (no live editor buffer
        // sidecar = no pending user edit), so the guard must report it as a
        // reconcilable drift rather than failing closed (#ipc-drift-visbuf-reconcile).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &drifted).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_drift", expected, None).unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
    }

    #[test]
    fn visible_write_reconcile_uses_crdt_relay_current_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        std::fs::write(&doc, expected).unwrap();
        seed_reliable_sync_open(&doc, "test-crdt-relay-current");
        let editor =
            agent_doc_document_realtime::crdt_relay::mint_client_id("test-crdt-relay-current");
        agent_doc_crdt_relay_io::with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.local_edit(editor, 0, 0, "operator relay text\n")
                .unwrap();
        })
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_crdt_relay", expected, None)
                .expect("relay current text should be authoritative");
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert!(fresh_current.starts_with("operator relay text\n"));
            }
            VisibleWriteReconcile::Clean => panic!("expected CRDT relay drift, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_crdt_current_drift"),
            "relay-current authority should be logged: {log}"
        );
        assert!(
            !log.contains("visible_write_legacy_live_buffer_ignored"),
            "relay authority should run before legacy sidecar fallback: {log}"
        );
    }

    // ── `#rtheal` — auto-heal a stale-behind committed editor buffer ──

    /// Build a temp git repo containing `doc.md`, apply each `commits` snapshot
    /// as its own commit in order, and return `(TempDir, file, canonical)` with
    /// the working tree at the LAST snapshot (== HEAD). The canonical path string
    /// keys the Lazily document cell, matching what the editor plugin reports.
    fn temp_git_doc(commits: &[&str]) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        let file = root.join("doc.md");
        for (i, body) in commits.iter().enumerate() {
            std::fs::write(&file, body).unwrap();
            git(&["add", "doc.md"]);
            git(&["commit", "-m", &format!("commit {i}")]);
        }
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    const HEAL_A: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] first\n",
        "- [ ] [#b] second\n",
        "- [ ] [#c] third\n",
        "<!-- /agent:backlog -->\n",
    );
    const HEAL_B: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] first\n",
        "- [ ] [#b] second\n",
        "- [ ] [#c] third\n",
        "- [ ] [#d] fourth\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: work — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn content_matches_recent_committed_blob_true_for_prior_commit_false_for_uncommitted() {
        // Two commits: A then B (HEAD). Both A and B are committed blobs; a string
        // that was never committed must not match.
        let (_dir, file, _canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        assert!(
            content_matches_recent_committed_blob(&file, HEAL_A, 15),
            "the older committed version A must be recognized as a committed blob"
        );
        assert!(
            content_matches_recent_committed_blob(&file, HEAL_B, 15),
            "the HEAD committed version B must be recognized as a committed blob"
        );
        assert!(
            !content_matches_recent_committed_blob(&file, "never committed content\n", 15),
            "content that matches no commit must return false"
        );
        // A tiny limit still finds the most recent commit (B) but can miss the
        // older one; the never-committed content stays false regardless.
        assert!(content_matches_recent_committed_blob(&file, HEAL_B, 1));
        assert!(!content_matches_recent_committed_blob(
            &file,
            "still never\n",
            1
        ));
    }

    #[test]
    fn content_matches_recent_committed_blob_false_outside_git_repo() {
        // No git repo (best-effort): any git error → false, never a panic.
        let (_dir, file, _canonical) = temp_doc("plain body\n");
        assert!(!content_matches_recent_committed_blob(
            &file,
            "plain body\n",
            15
        ));
    }

    // #live-editor-reactive (S2b/S3): zero-live-replica authority is decided by the
    // ground-truth open-file set, not the raw `live_editors == 0` poll.
    #[test]
    fn zero_live_editors_keeps_editor_authority_when_open_else_disk() {
        assert_eq!(
            resolve_zero_live_editors(true),
            ZeroLiveResolution::KeepEditorAuthority,
            "editor open → relay canonical stays editor authority (no demote)"
        );
        assert_eq!(
            resolve_zero_live_editors(false),
            ZeroLiveResolution::DiskAuthority,
            "editor closed → disk is the authoritative replica"
        );
    }

    // SimWorld-style: drive the reactive open-docs registry through an open/close/reopen
    // sequence and assert the derived zero-live resolution flips deterministically. Uses
    // a unique path key so the process-global registry singleton cannot cross-contaminate
    // other tests (deferral-not-dealloc keeps keys present).
    #[test]
    fn reactive_open_docs_drives_zero_live_resolution() {
        let reg = agent_doc_document_realtime::editor_open_docs::editor_open_docs();
        let path = "/tmp/agent-doc-s2b-reactive-open-drive-7f3a.md";
        reg.mark_open_with(path, || true);
        assert_eq!(
            resolve_zero_live_editors(reg.is_open(path)),
            ZeroLiveResolution::KeepEditorAuthority,
            "open editor keeps editor authority even with zero live replicas"
        );
        reg.mark_closed(path);
        assert_eq!(
            resolve_zero_live_editors(reg.is_open(path)),
            ZeroLiveResolution::DiskAuthority,
            "closing the editor demotes to disk authority"
        );
        reg.mark_open_with(path, || true);
        assert_eq!(
            resolve_zero_live_editors(reg.is_open(path)),
            ZeroLiveResolution::KeepEditorAuthority,
            "reopening flips back to editor authority (repair, not permanent demote)"
        );
    }

    // #live-editor-reactive (S4): the directive — leases/sidecars are NEVER on the
    // steady-state hot path. Once the reactive authority has recorded the doc (via an
    // in-process editor event), `observe_editor_open_in` reads it WITHOUT consulting the
    // durable lease backup. The probe panics to prove it is not called.
    #[test]
    fn observe_editor_open_steady_state_reads_reactive_without_lease() {
        use agent_doc_document_realtime::editor_open_docs::EditorOpenDocs;
        let reg = EditorOpenDocs::new();

        // Reactive authority says OPEN (e.g. from a replica register/update event).
        reg.mark_open("plan.md", true);
        assert!(
            observe_editor_open_in(&reg, "plan.md", || panic!(
                "lease must not be read when tracked"
            )),
            "tracked-open doc reads reactive authority (open) with no lease read"
        );

        // Reactive authority says CLOSED (e.g. from an editor deregister event).
        reg.mark_closed("plan.md");
        assert!(
            !observe_editor_open_in(&reg, "plan.md", || panic!(
                "lease must not be read when tracked"
            )),
            "tracked-closed doc reads reactive authority (closed) with no lease read"
        );
    }

    // #live-editor-reactive (S4): cold miss (never-seen doc, e.g. right after a controller
    // recycle before any editor event re-seeded the authority) recovers ONCE from the
    // durable lease backup and writes the result back, so later reads are purely reactive.
    #[test]
    fn observe_editor_open_cold_miss_recovers_from_lease_then_caches() {
        use agent_doc_document_realtime::editor_open_docs::EditorOpenDocs;
        use std::cell::Cell;

        // Cold miss + lease attached (editor genuinely still open across a recycle) →
        // keep editor authority, and the lease is consulted exactly once.
        let reg = EditorOpenDocs::new();
        let calls = Cell::new(0);
        assert!(observe_editor_open_in(&reg, "recycled.md", || {
            calls.set(calls.get() + 1);
            true
        }));
        assert_eq!(calls.get(), 1, "lease consulted once on the cold miss");
        // Now tracked → second read is purely reactive (probe would panic if called).
        assert!(observe_editor_open_in(&reg, "recycled.md", || panic!(
            "lease must not be read after cold-miss recovery seeds the authority"
        )));

        // Cold miss + lease detached (editor truly gone) → disk authority.
        let reg2 = EditorOpenDocs::new();
        assert!(!observe_editor_open_in(&reg2, "gone.md", || false));
        assert!(
            reg2.is_tracked("gone.md"),
            "cold miss records the recovered state"
        );
    }
}
