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
//! relay/disk adapter and ops-log/CPC effects. Cycle read sites (`preflight.rs` / `write.rs` /
//! `session_check.rs`) source current-doc through
//! [`try_resolve_current_doc_from_file`].
//!
//! ## Evals
//! - `durable_buffer_state_none_when_buffer_in_sync_with_disk`
//! - `durable_buffer_state_wins_when_unsaved_buffer_ahead_of_disk`
//! - `durable_buffer_state_none_when_no_editor_feed`
//! - `repair_cas_projects_retained_target_when_editor_owner_has_zero_replicas`

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
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

/// Wall-clock seconds (since UNIX epoch) of the last controller RPC failure
/// observed by [`observe_live_editor_authority`] (the controller-timeout path).
/// Hot polling paths — notably the supervisor idle-queue watch — read this via
/// [`controller_failed_within`] to back off a degraded controller instead of
/// paying the full read timeout on every poll and saturating it further
/// (`#idlewatchctrlbackoff`). 0 means "no failure observed yet".
static LAST_CONTROLLER_DEGRADED_SECS: AtomicI64 = AtomicI64::new(0);

thread_local! {
    /// True only while a CPC runtime effect is executing a document mutation.
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

#[cfg(test)]
fn install_post_delivery_proof_hook(file: PathBuf, hook: impl FnOnce(&Path) + Send + 'static) {
    *POST_DELIVERY_PROOF_HOOK.lock().unwrap() = Some((file, Box::new(hook)));
}

fn run_post_delivery_proof_hook(file: &Path) {
    #[cfg(not(test))]
    let _ = file;
    #[cfg(test)]
    {
        let hook = {
            let mut slot = POST_DELIVERY_PROOF_HOOK.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|(hook_file, _)| hook_file == file)
            {
                slot.take().map(|(_, hook)| hook)
            } else {
                None
            }
        };
        if let Some(hook) = hook {
            hook(file);
        }
    }
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
    ack_replica_update_for_file as test_support_ack_replica_update_for_file,
    pull_replica_updates_for_file as test_support_pull_replica_updates_for_file,
};

static DOCUMENT_AUTHORITY_EPOCH: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_DISK_WRITE_EPOCH: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_WRITE_INTENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_AUTHORITY_OBSERVATIONS: LazyLock<
    Mutex<HashMap<PathBuf, DocumentAuthorityObservation>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(test)]
type PostDeliveryProofHook = Box<dyn FnOnce(&Path) + Send>;
#[cfg(test)]
static POST_DELIVERY_PROOF_HOOK: LazyLock<Mutex<Option<(PathBuf, PostDeliveryProofHook)>>> =
    LazyLock::new(|| Mutex::new(None));
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
const CRDT_WRITE_CONVERGENCE_TIMEOUT_MS: u64 = 2_500;
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
const CRDT_ACK_REPLAY_SIGNAL_INTERVAL_MS: u64 = 250;
#[cfg(test)]
const CRDT_ACK_FORCE_REFRESH_AFTER_MS: u64 = 500;
#[cfg(not(test))]
const CRDT_ACK_FORCE_REFRESH_AFTER_MS: u64 = 2_000;
#[cfg(test)]
const CRDT_ACK_RECOVERY_TIMEOUT_MS: u64 = 1_800;
#[cfg(not(test))]
const CRDT_ACK_RECOVERY_TIMEOUT_MS: u64 = 8_000;
const _: () = assert!(
    CRDT_WRITE_CONVERGENCE_TIMEOUT_MS > CRDT_ACK_RECOVERY_TIMEOUT_MS + CRDT_WRITE_BACKOFF_MAX_MS
);

#[derive(Debug)]
struct AwaitEditorReplicaNoDiskWrite(String);

impl std::fmt::Display for AwaitEditorReplicaNoDiskWrite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AwaitEditorReplicaNoDiskWrite {}

fn await_editor_replica_no_disk_write(message: String) -> anyhow::Error {
    AwaitEditorReplicaNoDiskWrite(message).into()
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
        match FORCE_DISK_MUTATION_BASELINES.lock() {
            Ok(mut baselines) => {
                if let Some(active) = baselines.get_mut(&self.file) {
                    if active.holders > 1 {
                        active.holders -= 1;
                    } else {
                        baselines.remove(&self.file);
                    }
                }
            }
            Err(err) => eprintln!(
                "[agent-doc] force-disk authority baseline lock poisoned while clearing {}: {err}",
                self.file.display()
            ),
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
        let mut baselines = FORCE_DISK_MUTATION_BASELINES
            .lock()
            .map_err(|_| anyhow::anyhow!("force-disk authority baseline lock poisoned"))?;
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
    let mut baselines = FORCE_DISK_MUTATION_BASELINES
        .lock()
        .map_err(|_| anyhow::anyhow!("force-disk authority baseline lock poisoned"))?;
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
        .map_err(|_| anyhow::anyhow!("force-disk authority baseline lock poisoned"))?
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
    DeliveryAckPending,
    OperatorAdvancedAfterApply,
    CompareAndSwapRaced,
}

impl CrdtConvergenceState {
    const fn token(self) -> &'static str {
        match self {
            Self::TypingQuiescence => "typing_quiescence",
            Self::ControllerModelBackpressure => "controller_model_backpressure",
            Self::EditorAttachedModelMissing => "editor_attached_model_missing",
            Self::EditorSyncPending => "editor_sync_pending",
            Self::DeliveryAckPending => "delivery_ack_pending",
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

#[derive(Default)]
struct AckRecoveryState {
    started: Option<std::time::Instant>,
    last_signal: Option<std::time::Instant>,
    force_refresh_sent: bool,
    recovery_signal_observed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckRecoveryWait {
    Continue,
    ForegroundDeadline,
}

impl AckRecoveryState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn wait(&mut self, file: &Path, source: &str, live_editors: usize) -> Result<AckRecoveryWait> {
        let now = std::time::Instant::now();
        let started = *self.started.get_or_insert(now);
        let elapsed_ms = now
            .duration_since(started)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let force_refresh =
            elapsed_ms >= CRDT_ACK_FORCE_REFRESH_AFTER_MS && !self.force_refresh_sent;
        let signal_due = force_refresh
            || self.last_signal.is_none_or(|last| {
                now.duration_since(last)
                    >= std::time::Duration::from_millis(CRDT_ACK_REPLAY_SIGNAL_INTERVAL_MS)
            });
        if signal_due {
            let reason = if force_refresh {
                self.force_refresh_sent = true;
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh
            } else {
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckReplay
            };
            if let Err(err) =
                agent_doc_crdt_relay_io::signal_crdt_replica_event(file, reason, live_editors)
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_crdt_ack_recovery_signal_failed file={} reason={} error={err}",
                        file.display(),
                        reason,
                    ),
                );
            } else {
                self.recovery_signal_observed = true;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_crdt_ack_recovery_signal file={} reason={} elapsed_ms={} live_editors={} strategy=retained_ack_replay_then_bounded_reregister",
                        file.display(),
                        reason,
                        elapsed_ms,
                        live_editors,
                    ),
                );
            }
            self.last_signal = Some(now);
        }
        if elapsed_ms >= CRDT_ACK_RECOVERY_TIMEOUT_MS {
            return Ok(AckRecoveryWait::ForegroundDeadline);
        }
        Ok(AckRecoveryWait::Continue)
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
    ) -> Result<Option<agent_doc_crdt_relay_io::CpcRelayWrite>> {
        apply_canonical_replace_if_attached(file, expected_current, content, source)
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

fn observe_fresh_lazily_current_text(file: &Path, source: &str) -> Result<Option<String>> {
    #[cfg(any(test, feature = "test-support"))]
    const PUBLISH_TIMEOUT_MS: u64 = 100;
    #[cfg(not(any(test, feature = "test-support")))]
    const PUBLISH_TIMEOUT_MS: u64 = 1_000;

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let timeout = std::time::Duration::from_millis(PUBLISH_TIMEOUT_MS);
    agent_doc_crdt_relay_io::request_lazily_current_observation_with_timeout(
        &canonical, source, timeout,
    )?;

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

    if visible_document {
        // Inside the per-document write actor, make the CRDT canonical the
        // mutation plane and materialize disk only after every live editor has
        // ACKed the same canonical frontier. The existing disk projection is
        // the best available merge base for this legacy no-CAS API; the CRDT
        // convergence path rebases `content` over a newer operator cut.
        let projection_base = std::fs::read_to_string(path).unwrap_or_default();
        let mut post_proof_rebases = 0usize;
        loop {
            let Some(relay_write) = apply_canonical_replace_if_attached(
                path,
                &projection_base,
                content,
                "serialized_atomic_write",
            )?
            else {
                break;
            };

            if !relay_write.delivery_converged {
                return Err(await_editor_replica_no_disk_write(format!(
                    "serialized_atomic_write: binary-owned write for {} remains retained after the foreground editor ACK deadline (content_hash={}); the same intent will resume through session-check/supervisor recovery. Do not recapture or rerun finalize/write --commit, and do not force disk",
                    path.display(),
                    relay_write.content_hash,
                )));
            }

            // Deterministic unit coverage injects the exact race observed in the
            // Haiven live session: an editor delta lands after the delivery proof
            // but before the disk projection observation.
            run_post_delivery_proof_hook(path);

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
                    if !request_native_editor_save_for_canonical_projection(
                        path,
                        &text,
                        "serialized_atomic_write_projection",
                    )? {
                        let intent_id = ensure_deferred_document_write_intent(
                            path,
                            &projection_base,
                            content,
                            "serialized_atomic_write_editor_save_pending",
                            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
                        )?;
                        return Err(await_editor_replica_no_disk_write(format!(
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
                        &projection_base,
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
                    return Err(await_editor_replica_no_disk_write(format!(
                        "serialized_atomic_write: editor authority for {} kept advancing after delivery proof; binary-owned intent {intent_id} remains retained and will merge the unsaved editor cut before commit. Do not recapture or rerun finalize/write --commit, and do not force disk; session-check/supervisor recovery resumes this same intent",
                        path.display(),
                    )));
                }
            }
        }
    }

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
/// disk after a CRDT delivery ACK races the IDE's file-cache conflict handling
/// and can resurrect the older disk snapshot. The native save is therefore a
/// distinct protocol transition: it is successful only when disk contains the
/// exact canonical version and that same version remains the converged live
/// editor authority.
fn request_native_editor_save_for_canonical_projection(
    path: &Path,
    canonical: &str,
    source: &str,
) -> Result<bool> {
    if canonical_disk_projection_is_exact(path, canonical) {
        return Ok(true);
    }

    let canonical_path = path.canonicalize().with_context(|| {
        format!(
            "{source}: failed to canonicalize {} for native editor save",
            path.display()
        )
    })?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical_path);
    let path_str = canonical_path.to_string_lossy().to_string();
    let patch_id = format!("canonical-save-{}", uuid::Uuid::new_v4());
    let registration =
        agent_doc_controller_io::project_controller::live_editor_registration_for_file(path)
            .ok()
            .flatten();
    let socket_active = registration.as_ref().is_some_and(|registration| {
        agent_doc_ipc_io::is_listener_active_for_pid(&project_root, registration.pid)
    });
    let requested = match registration {
        Some(registration) if socket_active => agent_doc_ipc_io::send_save_document_to_editor(
            &project_root,
            registration.pid,
            &registration.editor_id,
            &path_str,
            &patch_id,
        ),
        _ => Ok(false),
    };
    if !matches!(requested, Ok(true)) {
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "native_editor_save_pending file={} source={} patch_id={} transport={} reason=request_failed",
                path.display(),
                source,
                patch_id,
                if socket_active {
                    "socket"
                } else {
                    "unavailable"
                },
            ),
        );
        return Ok(false);
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1_000);
    loop {
        if canonical_disk_projection_is_exact(path, canonical) {
            let current = observe_live_editor_authority_after_model_ensure(
                path,
                "native_editor_save_projection_proof",
            )?;
            if matches!(
                current,
                agent_doc_crdt_relay_io::CurrentText::Current {
                    ref text,
                    live_editors,
                    delivery_converged: true,
                } if live_editors > 0 && text == canonical
            ) {
                agent_doc_ops_log_io::log_op(
                    path,
                    &format!(
                        "native_editor_save_settled file={} source={} patch_id={} transport={} content_hash={} editor_version_exact=true disk_version_exact=true",
                        path.display(),
                        source,
                        patch_id,
                        "socket",
                        agent_doc_hash::content_hash(canonical),
                    ),
                );
                return Ok(true);
            }
            return Ok(false);
        }
        if std::time::Instant::now() >= deadline {
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "native_editor_save_pending file={} source={} patch_id={} transport={} reason=exact_disk_projection_timeout content_hash={}",
                    path.display(),
                    source,
                    patch_id,
                    "socket",
                    agent_doc_hash::content_hash(canonical),
                ),
            );
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Project the exact live editor authority through the editor's native save
/// path without changing the editor buffer.
///
/// This is the terminal recovery path for a valid, delivery-converged editor
/// cut whose historical deferred-write event was incorrectly retired after an
/// ACK but before disk-save proof. The editor remains the source of truth: no
/// disk candidate is merged into it and no force-disk write is permitted.
pub fn settle_live_editor_projection_through_authority(path: &Path, source: &str) -> Result<bool> {
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    if canonical == disk {
        return Ok(true);
    }
    validate_canonical_document_target(path, &canonical, source)?;
    let agent_doc_crdt_relay_io::CurrentText::Current {
        text,
        live_editors,
        delivery_converged: true,
    } = observe_live_editor_authority_after_model_ensure(path, source)?
    else {
        return Ok(false);
    };
    if live_editors == 0 || text != canonical {
        return Ok(false);
    }
    if !request_native_editor_save_for_canonical_projection(path, &canonical, source)? {
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
            "live_editor_projection_settled file={} source={} content_hash={} editor_authority=true delivery_converged=true disk_version_exact=true",
            path.display(),
            source,
            agent_doc_hash::content_hash(&canonical),
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
    let resolved = try_resolve_current_document_content(path, source)?;
    if !visible_write_content_matches(&resolved, content) {
        apply_canonical_replace_if_attached(path, expected_current, content, source)?;
    }
    atomic_write_through_authority(path, content)
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
        "{source}: refusing committed projection settlement for {} without exact authority/disk current-content proof (expected_hash={}, canonical_hash={}, disk_hash={})",
        path.display(),
        agent_doc_hash::content_hash(expected_current),
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    atomic_write_if_current_through_authority(path, committed_content, expected_current, source)?;
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        canonical == committed_content && disk == committed_content,
        "{source}: committed projection settlement for {} did not converge exactly (committed_hash={}, canonical_hash={}, disk_hash={})",
        path.display(),
        agent_doc_hash::content_hash(committed_content),
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
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
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    if canonical != committed_content || disk != expected_disk {
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

    atomic_write_if_current_through_authority(path, committed_content, expected_disk, source)?;
    let canonical = try_resolve_current_document_content(path, source)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        canonical == committed_content && disk == committed_content,
        "{source}: retained committed projection for {} did not converge exactly (committed_hash={}, canonical_hash={}, disk_hash={})",
        path.display(),
        committed_hash,
        agent_doc_hash::content_hash(&canonical),
        agent_doc_hash::content_hash(&disk),
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

    // The retained target was composed against an older authority cut.  A live
    // editor may have advanced before its ACK, or the operator may have saved
    // that newer cut before asynchronous recovery resumed.  Replay the same
    // retained journal over the current canonical text; never require Ctrl+S,
    // preflight repair, recapture, or a force-disk reset to make progress.
    let mut canonical = try_resolve_current_document_content(path, source)?;
    if canonical != pending.target_content {
        let Some(rebased_target) = deferred_document_write_reconnect_content(path, &canonical)?
        else {
            return Ok(false);
        };
        if !agent_doc_turn::response_replay::response_materialized_in_content(
            captured_response,
            &rebased_target,
        ) {
            return Ok(false);
        }
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
        let Some(projected) = settle_acknowledged_captured_projection_through_authority(
            path,
            captured_response,
            source,
        )?
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
    let transient_active_prompt_marker_intent =
        pending.source.starts_with("serialized_atomic_write")
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
            | DocumentWriteDeferredReason::CrdtDeliveryAckPending
    );
    let semantic_response_base = matches!(
        pending.reason,
        DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget
    )
    .then_some(pending.expected_content.as_deref())
    .flatten()
    .filter(|expected| {
        !write_policy::buffer_presents_reference_response(&pending.target_content, expected)
    });
    if !exact_projection_reason && semantic_response_base.is_none() {
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
            if !request_native_editor_save_for_canonical_projection(
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

    let target_hash = agent_doc_hash::content_hash(&pending.target_content);
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
        if !request_native_editor_save_for_canonical_projection(
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
        let eligible_source = intent.source == "post_commit_reposition"
            || intent.source.starts_with("serialized_atomic_write");
        let eligible_reason = matches!(
            intent.reason,
            DocumentWriteDeferredReason::CrdtDeliveryAckPending
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
pub fn settle_acknowledged_captured_projection_through_authority(
    path: &Path,
    captured_response: &str,
    source: &str,
) -> Result<Option<String>> {
    let current = observe_live_editor_authority_after_model_ensure(path, source)?;
    let agent_doc_crdt_relay_io::CurrentText::Current {
        text,
        live_editors,
        delivery_converged: true,
        ..
    } = current
    else {
        return Ok(None);
    };
    if live_editors == 0
        || !agent_doc_turn::response_replay::response_materialized_in_content(
            captured_response,
            &text,
        )
    {
        return Ok(None);
    }

    if !request_native_editor_save_for_canonical_projection(
        path,
        &text,
        "acknowledged_captured_projection_settlement",
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
            "acknowledged_captured_projection_settled file={} content_hash={} live_editors={} delivery_converged=true disk_rewritten=false native_editor_save=true captured_response_materialized=true",
            path.display(),
            agent_doc_hash::content_hash(&text),
            live_editors,
        ),
    );
    Ok(Some(text))
}

/// Repair-only zero-replica recovery.
///
/// Ordinary editor-owned writes must never fall back to disk when the owner has
/// no registered relay replica. Explicit repair is different: after the CAS
/// write has durably retained the exact canonical target, it may project that
/// same target to disk through the audited force-disk authority. This closes the
/// recovery transaction instead of leaving clean CRDT authority paired with a
/// corrupt disk projection that a retry would misclassify as "nothing to do".
pub fn atomic_repair_write_if_current_through_authority(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<String> {
    match atomic_write_if_current_through_authority(path, content, expected_current, source) {
        Ok(()) => {
            let canonical = try_resolve_current_document_content(path, source)?;
            let disk = resolve_disk_current_document_content(path, source)?;
            anyhow::ensure!(
                canonical == content && disk == content,
                "{source}: successful repair write for {} did not converge exactly before settling deferred lineage (expected_hash={}, canonical_hash={}, disk_hash={})",
                path.display(),
                agent_doc_hash::content_hash(content),
                agent_doc_hash::content_hash(&canonical),
                agent_doc_hash::content_hash(&disk),
            );
            clear_all_deferred_document_write_intents(path, source)?;
            return Ok(canonical);
        }
        Err(err)
            if err
                .downcast_ref::<AwaitEditorReplicaNoDiskWrite>()
                .is_none() =>
        {
            return Err(err);
        }
        Err(_) => {}
    }

    let canonical = try_resolve_current_document_content(path, source)?;
    let retained_target = if visible_write_content_matches(&canonical, content) {
        canonical
    } else {
        // The editor may advance between repair composition and the serialized
        // CRDT apply. `apply_canonical_replace_if_attached` rebases the repair
        // candidate over that newer operator cut. Prove that the canonical
        // frontier is exactly that semantic rebase before treating it as the
        // retained repair target; byte equality with the stale candidate would
        // reject a valid monotonic merge and strand the closeout.
        let editor_cut =
            editor_operator_cut_for_agent_rebase(path, expected_current, &canonical, source);
        let merged = rebase_agent_candidate_over_editor_cut(expected_current, content, &editor_cut)
            .with_context(|| {
                format!(
                    "{source}: failed to verify the retained repair rebase for {}",
                    path.display()
                )
            })?;
        let verified = canonicalize_and_validate_agent_rebase(&merged, content, path, source)?;
        anyhow::ensure!(
            visible_write_content_matches(&canonical, &verified),
            "{source}: zero-replica repair target for {} was not retained as the requested semantic rebase (requested_hash={}, verified_hash={}, canonical_hash={}); refusing force-disk projection",
            path.display(),
            agent_doc_hash::content_hash(content),
            agent_doc_hash::content_hash(&verified),
            agent_doc_hash::content_hash(&canonical),
        );
        agent_doc_ops_log_io::log_op(
            path,
            &format!(
                "{source}_retained_rebased_repair_target file={} requested_hash={} canonical_hash={} authority=editor_cut",
                path.display(),
                agent_doc_hash::content_hash(content),
                agent_doc_hash::content_hash(&canonical),
            ),
        );
        canonical
    };
    let pre_force_disk = resolve_disk_current_document_content(path, source)?;
    let reconnect_base = pending_document_write(path)
        .and_then(|pending| {
            pending.expected_content.filter(|expected| {
                agent_doc_hash::content_hash(expected).eq_ignore_ascii_case(&pending.expected_hash)
            })
        })
        .unwrap_or_else(|| pre_force_disk.clone());
    atomic_write_force_disk_through_authority(path, &retained_target)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        disk == retained_target,
        "{source}: zero-replica repair projection for {} did not materialize exactly (expected_hash={}, disk_hash={})",
        path.display(),
        agent_doc_hash::content_hash(&retained_target),
        agent_doc_hash::content_hash(&disk),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    if reconnect_base != retained_target {
        ensure_deferred_document_write_intent(
            path,
            &reconnect_base,
            &retained_target,
            "repair_force_disk",
            DocumentWriteDeferredReason::RetainEditorReconnectLineageBeforeDiskProjection,
        )?;
    }
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "{source}_zero_replica_repair_projected file={} content_hash={} authority=retained_crdt_cas transport=audited_force_disk",
            path.display(),
            agent_doc_hash::content_hash(&retained_target),
        ),
    );
    Ok(retained_target)
}

/// Apply a binary/CPC-authored document update to the live CRDT relay.
///
/// When an editor owns the document, this is the write-side companion to
/// [`try_resolve_current_document_content`]: the controller canonical replica is
/// updated first, with `expected_current` proving that the response was merged
/// against the current editor-buffer state. The real markdown file may then be
/// materialized as a projection of this relay state.
pub fn apply_cpc_write_through_relay_authority(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CpcRelayWrite>> {
    validate_canonical_document_target(file, content, source)?;
    if controller_document_mutation_in_progress()
        || agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file)
    {
        return agent_doc_crdt_relay_io::apply_cpc_write_for_file(
            file,
            expected_current,
            content,
            source,
        );
    }
    agent_doc_controller_io::project_controller::apply_cpc_write_via_controller_model_for_doc(
        file,
        expected_current,
        content,
        source,
    )
}

/// Fold text proven visible by an editor receipt into the canonical relay.
///
/// Production requests cross the controller boundary so subsequent canonical
/// reads and commit barriers observe the update. Unit fixtures that explicitly
/// opt into a process-local relay retain their isolated model.
pub fn adopt_verified_editor_text_through_relay_authority(
    file: &Path,
    text: &str,
    source: &str,
) -> Result<Option<bool>> {
    if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return agent_doc_crdt_relay_io::adopt_editor_text_for_file(file, text);
    }
    agent_doc_controller_io::project_controller::adopt_editor_text_via_controller_model_for_doc(
        file, text, source,
    )
}

pub fn apply_canonical_replace_if_attached(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CpcRelayWrite>> {
    let started = std::time::Instant::now();
    // Keep the write budget as a portable lazily timeline. Individual controller
    // RPC timeouts are congestion signals inside this larger deadline, not a
    // reason to abandon an already-accepted compact/finalize mutation.
    let mut deadline = DeadlineCore::new(CRDT_WRITE_CONVERGENCE_TIMEOUT_MS);
    let mut backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
    // `#crdtcasraced`: the canonical hash of the last non-converged apply, so the
    // backoff policy can tell a write that is genuinely advancing from one that
    // is re-applying against a moving frontier and racing forever.
    let mut last_applied_hash: Option<String> = None;
    let mut pending_target: Option<String> = None;
    let mut pending_write: Option<agent_doc_crdt_relay_io::CpcRelayWrite> = None;
    let mut ack_recovery = AckRecoveryState::default();
    let mut wait_state = CrdtConvergenceState::TypingQuiescence;
    let mut last_notice = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(2))
        .unwrap_or_else(std::time::Instant::now);

    loop {
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        deadline.tick(elapsed_ms);
        if deadline.is_expired() {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_crdt_convergence_timeout file={} reason={} timeout_ms={} recovery=retry_crdt_merge_no_legacy_replay",
                    file.display(),
                    wait_state,
                    CRDT_WRITE_CONVERGENCE_TIMEOUT_MS,
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

        // A CPC write is issued only from a quiescent editor cut. Waiting here
        // happens in the caller, outside the controller RPC loop, so editor
        // deltas and delivery ACKs remain responsive while typing settles.
        if pending_target.is_none() {
            let remaining_ms = CRDT_WRITE_CONVERGENCE_TIMEOUT_MS.saturating_sub(elapsed_ms);
            guard_visible_write_current_transition_with_budget(
                file,
                source,
                CRDT_WRITE_SETTLE_MS,
                remaining_ms.max(1),
            )
            .with_context(|| {
                format!(
                    "{source}: waiting for editor typing to settle before CRDT write for {}",
                    file.display()
                )
            })?;
        }

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
                } => {
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
                                        schedule_stale_editor_replica_pcp_recycle(file, source);
                                return Err(await_editor_replica_no_disk_write(format!(
                                    "{source}: retained canonical target for {} after its editor replica disappeared (content_hash={}): zero-member delivery convergence is not visible-write proof; disk was not written; recycle_status={recycle_status}",
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
                                    "{source}_crdt_relay_acknowledged file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true disk_projection=pending wait_ms={} transport=crdt_only",
                                    file.display(),
                                    relay_write.content_hash,
                                    relay_write.update_bytes,
                                    relay_write.targets,
                                    live_editors,
                                    started.elapsed().as_millis(),
                                ),
                            );
                            return Ok(Some(relay_write));
                        }
                        if write_policy::decide_crdt_write_admission(delivery_converged)
                            == write_policy::CrdtWriteAdmission::WaitForDeliveryAck
                        {
                            // Do not stack a second write behind an unacknowledged
                            // one. The editor pulls a coalesced canonical frontier;
                            // this poll applies backpressure until that frontier is
                            // visible and ACKed.
                            wait_state = CrdtConvergenceState::DeliveryAckPending;
                            if ack_recovery.wait(file, source, live_editors)?
                                == AckRecoveryWait::ForegroundDeadline
                            {
                                let relay_write = pending_write
                                    .take()
                                    .expect("pending CRDT target must retain its write receipt");
                                let exact_target_retained = relay_write.applied
                                    && relay_text == *applied_target
                                    && relay_write.content_hash
                                        == agent_doc_hash::content_hash(applied_target);
                                let completion = write_policy::decide_crdt_write_completion(
                                    write_policy::CrdtWriteCompletionEvidence {
                                        exact_target_retained,
                                        async_delivery_recovery_active: ack_recovery
                                            .recovery_signal_observed,
                                        delivery_converged,
                                    },
                                );
                                match completion {
                                    write_policy::CrdtWriteCompletion::RetainedForAsyncDelivery => {
                                        agent_doc_ops_log_io::log_op(
                                            file,
                                            &format!(
                                                "{source}_crdt_delivery_deferred file={} content_hash={} timeout_ms={} recovery=retained_async_editor_delivery operator_action=none",
                                                file.display(),
                                                relay_write.content_hash,
                                                CRDT_ACK_RECOVERY_TIMEOUT_MS,
                                            ),
                                        );
                                        return Ok(Some(relay_write));
                                    }
                                    write_policy::CrdtWriteCompletion::BlockMissingRetention => {
                                        anyhow::bail!(
                                            "{source}: editor delivery ACK recovery for {} did not settle within {}ms and the exact canonical target lacks active retained-delivery proof; refusing closeout",
                                            file.display(),
                                            CRDT_ACK_RECOVERY_TIMEOUT_MS,
                                        );
                                    }
                                    write_policy::CrdtWriteCompletion::VisibleAndAcknowledged => {
                                        unreachable!(
                                            "delivery-converged writes return before ACK recovery"
                                        );
                                    }
                                }
                            }
                        } else {
                            // Operator text arrived after our write. Recompute from
                            // the original base/agent candidate against the newest
                            // converged operator cut, then issue one new CRDT delta.
                            pending_target = None;
                            pending_write = None;
                            ack_recovery.reset();
                            backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
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
                agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file)
                    && agent_doc_controller_io::project_controller::live_editor_registration_for_file(
                        file,
                    )
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
                                    schedule_stale_editor_replica_pcp_recycle(file, source);
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_editor_delivery_worker_stale file={} intent_id={} content_hash={} authority=live_ide_pid delivery=fresh_heartbeat_missing recovery=pcp_recycle_no_disk_write recycle_status={recycle_status}",
                                    file.display(),
                                    intent_id,
                                    agent_doc_hash::content_hash(&effective_target),
                                ),
                            );
                            return Err(await_editor_replica_no_disk_write(format!(
                                "{source}: retained the canonical write for {} in CRDT + Lazily state (intent_id={intent_id}), but the live editor delivery worker heartbeat is stale; disk was not written; recycle_status={recycle_status}",
                                file.display(),
                            )));
                        }

                        let zero_replica_visible_write_proven = live_editors == 0
                            && relay_text == effective_target
                            && durable_visible_write_content_proves_target(file, &effective_target);
                        if live_editors == 0 && relay_text == effective_target {
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
                                let intent_id = ensure_deferred_document_write_intent(
                                    file,
                                    &relay_text,
                                    &effective_target,
                                    source,
                                    DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
                )?;
                                let recycle_status = agent_doc_controller_io::project_controller::
                    schedule_stale_editor_replica_pcp_recycle(file, source);
                                return Err(await_editor_replica_no_disk_write(format!(
                                    "{source}: deferred write for {} in Lazily state (intent_id={intent_id}): the editor owns the document but no relay replica is registered; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_session_check; run only agent-doc session-check for the existing binary-owned capture; do not resubmit finalize, write --commit, or --force-disk",
                                    file.display(),
                                )));
                            }
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
                            if live_editors == 0 {
                                DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica
                            } else {
                                DocumentWriteDeferredReason::CrdtDeliveryAckPending
                            },
                        )?;

                        let zero_replica_intent =
                            if live_editors == 0 && !zero_replica_visible_write_proven {
                                Some(retained_intent_id)
                            } else {
                                None
                            };

                        match apply_cpc_write_through_relay_authority(
                            file,
                            &relay_text,
                            &effective_target,
                            source,
                        ) {
                            Ok(None) => return Ok(None),
                            Ok(Some(relay_write))
                                if relay_write.applied && relay_write.targets == 0 =>
                            {
                                let intent_id = zero_replica_intent.unwrap_or_else(|| {
                                    "durable-intent-recorded-before-cpc-write".to_string()
                                });
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_write_deferred file={} intent_id={} content_hash={} targets=0 live_editors=0 recovery=await_editor_replica_no_disk_write",
                                        file.display(),
                                        intent_id,
                                        relay_write.content_hash,
                                    ),
                                );
                                let recycle_status = agent_doc_controller_io::project_controller::
                                    schedule_stale_editor_replica_pcp_recycle(file, source);
                                return Err(await_editor_replica_no_disk_write(format!(
                                    "{source}: retained the canonical write for {} in CRDT + Lazily state (intent_id={intent_id}), but no editor replica was registered to receive it; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_session_check; run only agent-doc session-check for the existing binary-owned capture; do not resubmit finalize, write --commit, or --force-disk",
                                    file.display(),
                                )));
                            }
                            Ok(Some(mut relay_write)) if relay_write.delivery_converged => {
                                relay_write.live_editors = live_editors;
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_relay_acknowledged file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true disk_projection=pending wait_ms={} transport=crdt_only",
                                        file.display(),
                                        relay_write.content_hash,
                                        relay_write.update_bytes,
                                        relay_write.targets,
                                        live_editors,
                                        started.elapsed().as_millis(),
                                    ),
                                );
                                return Ok(Some(relay_write));
                            }
                            Ok(Some(relay_write)) => {
                                wait_state = CrdtConvergenceState::DeliveryAckPending;
                                pending_target = Some(effective_target);
                                // `#crdtcasraced`: an apply is only progress if it
                                // actually moved the canonical content. Resetting the
                                // floor on every apply kept a contended write pinned at
                                // 25ms for the whole 60s budget (~2400 merge+CAS
                                // attempts, each worsening the contention it retried).
                                let advanced =
                                    last_applied_hash.as_deref() != Some(relay_write.content_hash.as_str());
                                last_applied_hash = Some(relay_write.content_hash.clone());
                                backoff_ms = CRDT_WRITE_BACKOFF_POLICY.next_ms(backoff_ms, advanced);
                                pending_write = Some(relay_write);
                                ack_recovery.reset();
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
                                            backoff_ms,
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
                    backoff_ms,
                ),
            );
            last_notice = std::time::Instant::now();
        }
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let remaining_ms = CRDT_WRITE_CONVERGENCE_TIMEOUT_MS.saturating_sub(elapsed_ms);
        let sleep_for = std::time::Duration::from_millis(backoff_ms.min(remaining_ms));
        if !sleep_for.is_zero() {
            std::thread::sleep(sleep_for);
        }
        backoff_ms = CRDT_WRITE_BACKOFF_POLICY.next_ms(backoff_ms, false);
    }
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
    let editor_reconciled = agent_doc_merge::response_cell::reconcile_superseded_response_targets(
        editor_cut,
        merge_base,
        agent_target,
    )?
    .unwrap_or_else(|| editor_cut.to_string());
    let target_introduces_response =
        !write_policy::buffer_presents_reference_response(agent_target, merge_base);
    if target_introduces_response {
        if write_policy::buffer_presents_reference_response(agent_target, &editor_reconciled) {
            return Ok(editor_reconciled);
        }
        if let Some(recovered) = write_policy::live_prompt_drift_recovery_target(
            agent_target,
            &editor_reconciled,
            write_policy::normalize_visible_recovery_compare,
        ) {
            return Ok(recovered);
        }
    }

    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(merge_base).encode_state();
    agent_doc_merge::crdt::merge_by_component(Some(&base_state), agent_target, &editor_reconciled)
}

/// Remove one whole-line component close that is provably an unmatched replay
/// duplicate. The scan mirrors the component parser's stack discipline and
/// ignores markers in code/quoted ranges. We only repair when there is exactly
/// one unmatched close, the same component already had a balanced close, and
/// deleting that line restores the complete projection integrity contract.
fn remove_single_unmatched_duplicate_component_close(content: &str) -> Option<String> {
    let ignored = agent_doc_element::element::find_code_ranges(content)
        .into_iter()
        .chain(agent_doc_element::element::find_quoted_ranges(content))
        .collect::<Vec<_>>();
    let mut stack: Vec<String> = Vec::new();
    let mut balanced_closes = std::collections::HashMap::<String, usize>::new();
    let mut unmatched: Option<(usize, usize, String)> = None;
    let mut offset = 0usize;

    for line in content.split_inclusive('\n') {
        let line_start = offset;
        let line_end = line_start + line.len();
        offset = line_end;
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let marker_start = line_start + leading;
        if ignored
            .iter()
            .any(|&(start, end)| marker_start >= start && marker_start < end)
        {
            continue;
        }

        let trimmed = line.trim();
        let Some(inner) = trimmed
            .strip_prefix("<!--")
            .and_then(|value| value.strip_suffix("-->"))
            .map(str::trim)
        else {
            continue;
        };
        if inner.starts_with("agent:boundary:") {
            continue;
        }
        if let Some(name) = inner.strip_prefix("/agent:") {
            if stack.last().is_some_and(|open| open == name) {
                stack.pop();
                *balanced_closes.entry(name.to_string()).or_default() += 1;
            } else if stack.is_empty()
                && balanced_closes.get(name).copied().unwrap_or_default() > 0
                && unmatched.is_none()
            {
                unmatched = Some((line_start, line_end, name.to_string()));
            } else {
                return None;
            }
        } else if let Some(rest) = inner.strip_prefix("agent:") {
            let name = rest.split_whitespace().next().unwrap_or_default();
            if name.is_empty() {
                return None;
            }
            stack.push(name.to_string());
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let (start, end, _) = unmatched?;
    let mut repaired = String::with_capacity(content.len() - (end - start));
    repaired.push_str(&content[..start]);
    repaired.push_str(&content[end..]);
    agent_projection_integrity_valid(&repaired).then_some(repaired)
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
    if agent_projection_integrity_valid(content) {
        return None;
    }
    // `#boundarysplice`: restore a boundary terminator a cell merge welded into
    // the following line BEFORE the other repairs, since a malformed agent
    // comment makes the document unparseable and blocks every one of them. The
    // boundary is transient binary-owned scaffolding and the repair is lossless,
    // so this cannot touch operator prose.
    let mut normalized = agent_doc_element::element::repair_malformed_boundary_comment(content)
        .unwrap_or_else(|| content.to_string());
    normalized = agent_doc_merge::response_cell::deduplicate_response_cells(&normalized)
        .ok()
        .flatten()
        .unwrap_or(normalized);
    if !agent_projection_integrity_valid(&normalized)
        && let Some(repaired) = remove_stale_standalone_exchange_boundary(&normalized)
    {
        normalized = repaired;
    }
    if !agent_projection_integrity_valid(&normalized) {
        normalized = remove_single_unmatched_duplicate_component_close(&normalized)?;
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
                "{source}_concatenated_editor_generations_recovered file={} intent_id={} observed_hash={} recovered_hash={} strategy=pending_intent_semantic_rebase",
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
    Ok(())
}

/// Resolve the operator-authored editor cut independently from agent projection
/// bytes. A live IDE buffer normally wins as-is. If it is structurally poisoned
/// by a prior non-operator CPC projection (duplicate boundary/exchange), and the
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
    if operator_cut == observed_editor || agent_projection_integrity_valid(observed_editor) {
        return observed_editor.to_string();
    }
    if !agent_projection_integrity_valid(&operator_cut) {
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
    let repaired = agent_doc_element::element::repair_malformed_boundary_comment(content)?;
    Some(agent_doc_template::reposition_boundary_to_end_clean(
        &repaired,
    ))
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
    let canonical = heal_welded_boundary(&canonical).unwrap_or(canonical);
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
    if let Err(e) =
        agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)
    {
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

// ── Rung 2 (`#rtwfeed`): CPC-owned CRDT current-document feed ──
//
// Rung 1 above is the pure authority decision over a trusted `BufferState`.
// Rung 2 is the durable source of that state: the CPC-owned CRDT/lazily model.
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
    let mut observations = DOCUMENT_AUTHORITY_OBSERVATIONS
        .lock()
        .expect("document authority observation cache poisoned");
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
    match agent_doc_controller_io::project_controller::append_state_event(&project_root, &event) {
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
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    agent_doc_controller_io::project_controller::load_state_backbone_projection(&project_root)
        .ok()?
        .document(&document_hash)?
        .document
        .pending_write
        .clone()
}

/// Ordered deferred agent changes for `file`. Newer targets are normally
/// cumulative, but retaining each intent lets reconnect replay an earlier
/// same-component mutation (for example `--backlog-add`) even if a later
/// whole-document merge accidentally omitted it.
pub fn pending_document_write_journal(
    file: &Path,
) -> Vec<agent_doc_state_backbone::DocumentWriteIntentProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Vec::new();
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let Some(document) =
        agent_doc_controller_io::project_controller::load_state_backbone_projection(&project_root)
            .ok()
            .and_then(|projection| projection.document(&document_hash).cloned())
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
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    agent_doc_controller_io::project_controller::load_state_backbone_projection(&project_root)
        .ok()?
        .document(&document_hash)?
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
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return false;
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let candidate_hash = agent_doc_hash::content_hash(
        &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content),
    );
    agent_doc_controller_io::project_controller::load_state_backbone_projection(&project_root)
        .ok()
        .and_then(|projection| {
            projection
                .document(&document_hash)
                .and_then(|document| document.applied_visible_write_candidate(&candidate_hash))
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
    validate_canonical_document_target(file, content, source)?;
    let mut expected_content = expected_current.to_string();
    let mut target_content = content.to_string();
    let requested_target_hash = agent_doc_hash::content_hash(content);
    let external_disk_candidate =
        reason == DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor;
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

        // An external disk candidate is a replaceable user-decision value, not
        // a CRDT branch. A later filesystem event replaces the candidate while
        // preserving the exact current editor cut; never component-merge two
        // successive disk versions into the live buffer.
        if external_disk_candidate {
            // Boundary/marker cleanup after one operator-authorized force-disk
            // write refines that same candidate. Keep the original editor cut
            // as its comparison base; the bytes currently on disk are the
            // prior force-disk target, not a newer editor decision.
            if source.starts_with("force_disk")
                && pending.source.starts_with("force_disk")
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
                // The requested target is already the live CPC authority. A prior
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
            target_hash,
            target_content,
            source: source.to_string(),
            reason,
        },
    );
    agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)
        .with_context(|| {
            format!(
                "failed to retain deferred document write in Lazily state for {}",
                file.display()
            )
        })?;
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
    let pending_journal = pending_document_write_journal(file);
    let Some(pending) = pending_journal.last().cloned() else {
        return Ok(None);
    };
    if editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
        validate_canonical_document_target(
            file,
            &pending.target_content,
            "editor_reconnect_retained_target",
        )?;
        return Ok(Some(pending.target_content));
    }

    let disk_content = std::fs::read_to_string(file).ok();
    // `#boundarysplice`: heal a welded boundary marker in the editor canonical at
    // intake. The corruption is in `editor_content` itself, so every downstream
    // merge and `validate_canonical_document_target` call inherits it — which is
    // the permanent reconnect wedge, since the validator refuses before any repair
    // seam runs and nothing else rewrites the text. Repairing here is lossless and
    // touches only binary-owned scaffolding.
    let mut merged =
        heal_welded_boundary(editor_content).unwrap_or_else(|| editor_content.to_string());
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
            heal_welded_boundary(&pending.target_content).unwrap_or(pending.target_content);
        validate_canonical_document_target(file, &retained, "editor_reconnect_retained_target")?;
        return Ok(Some(retained));
    }
    let merged = heal_welded_boundary(&merged).unwrap_or(merged);
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
            source: source.to_string(),
            reason: DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor,
        },
    );
    agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)
        .with_context(|| {
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
        },
    );
    agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)
        .with_context(|| {
            format!(
                "failed to settle deferred document write in Lazily state for {}",
                file.display()
            )
        })?;
    Ok(())
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
    if let Some(current) = query_embedded_relay(file, source)? {
        return Ok(current);
    }
    if !agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(file) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "document_model_controller_lookup_skipped file={} source={} reason=lazily_editor_absent",
                file.display(),
                source,
            ),
        );
        return Ok(agent_doc_crdt_relay_io::CurrentText::Detached);
    }
    #[cfg(test)]
    {
        return agent_doc_crdt_relay_io::current_text_for_file(file);
    }
    #[cfg(not(test))]
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
        file, source,
    ) {
        Ok(Some(current)) => Ok(current),
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_controller_lookup_unavailable file={} source={} fallback=none",
                    file.display(),
                    source,
                ),
            );
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
        }
        Err(e) => {
            // Record controller degradation so hot polling paths (idle-queue
            // watch) can back off and stop flooding a wedged controller.
            record_controller_degraded();
            #[cfg(not(test))]
            {
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
    let start = std::time::Instant::now();
    loop {
        let (ready, state) = match query_live_editor_authority_after_model_ensure(file, source) {
            Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => (true, "detached"),
            Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                delivery_converged: true,
                ..
            }) => (true, "lazily_current"),
            Ok(agent_doc_crdt_relay_io::CurrentText::Current { .. }) => (false, "delivery_pending"),
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => {
                (false, "missing_replica")
            }
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => {
                (false, "current_pending")
            }
            Err(_) => (false, "authority_unavailable"),
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
            anyhow::bail!(
                "visible document write for {} deferred: Lazily current transition remained {} for {}ms; retry after it settles",
                file.display(),
                state,
                timeout_ms
            );
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

/// Source the durable editor-buffer feed for `file` from the CPC-owned CRDT
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
                    "durable_buffer_state_cpc_unavailable file={} error={}",
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
    try_resolve_current_doc_from_file_with_source(file, source)
        .map(|reconciliation| CurrentDocument::new(file.to_path_buf(), reconciliation))
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
const EDITOR_REPLICA_REOBSERVE_BACKOFF: std::time::Duration =
    std::time::Duration::from_millis(250);

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

fn reobserve_missing_editor_replica_with_reregistration(
    file: &std::path::Path,
    source: &str,
    require_model_ensure: bool,
    observed: Result<agent_doc_crdt_relay_io::CurrentText>,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if !observation_is_missing_replica_family(file, &observed) {
        return observed;
    }
    let attempts = editor_replica_reobserve_attempts();
    let mut current = observed;
    for attempt in 1..=attempts {
        let reregister = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
            file,
            agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
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
            return current;
        }
    }
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
                    "realtime_doc_resolve_crdt_error file={} source={} error={}",
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
                            "realtime_doc_resolve authority={} reason={} diverged={} file={} source=crdt_relay live_editors={} delivery_converged={} editor_open=true recovery=keep_editor_authority_no_live_replica",
                            reconciliation.authority.as_str(),
                            reconciliation.reason,
                            reconciliation.diverged,
                            file.display(),
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
                return Ok(resolve_disk_only_current_doc(file, &disk, "editor_absent"));
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
                    "realtime_doc_resolve authority={} reason={} diverged={} file={} source=crdt_relay live_editors={} delivery_converged={}",
                    reconciliation.authority.as_str(),
                    reconciliation.reason,
                    reconciliation.diverged,
                    file.display(),
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
            Ok(resolve_detached_current_doc(file, &disk))
        }
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "realtime_doc_resolve_deferred file={} source=crdt_relay reason=missing_replica",
                    file.display(),
                ),
            );
            anyhow::bail!(
                "document model startup/reconciliation for {} returned editor_attached_model_missing; disk is a non-authoritative replica and was not read",
                file.display()
            );
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "realtime_doc_resolve_deferred file={} source=crdt_relay reason=sync_pending",
                    file.display(),
                ),
            );
            anyhow::bail!(
                "document model startup/reconciliation for {} returned editor_sync_pending; disk is a non-authoritative replica and was not read",
                file.display()
            );
        }
    }
}

/// Resolve the detached-editor fallback path.
///
fn resolve_detached_current_doc(file: &std::path::Path, disk: &str) -> Reconciliation {
    let buffer = durable_buffer_state(file, disk);
    let reconciliation = reconcile_current_doc(disk, buffer.as_ref());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "realtime_doc_resolve authority={} reason={} diverged={} file={}",
            reconciliation.authority.as_str(),
            reconciliation.reason,
            reconciliation.diverged,
            file.display(),
        ),
    );
    reconciliation
}

fn resolve_disk_only_current_doc(
    file: &std::path::Path,
    disk: &str,
    reason: &'static str,
) -> Reconciliation {
    let reconciliation = reconcile_current_doc(disk, None);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "realtime_doc_resolve authority={} reason={} diverged={} file={}",
            reconciliation.authority.as_str(),
            reason,
            reconciliation.diverged,
            file.display(),
        ),
    );
    Reconciliation {
        reason,
        ..reconciliation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ── Rung 2 (`#rtwfeed`) CPC-owned CRDT feed ──

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

        assert!(!agent_projection_integrity_valid(&poisoned));
        assert!(!agent_projection_integrity_valid(&duplicate_boundary));
        assert!(!agent_projection_integrity_valid(&unclosed_exchange));
        let recovered =
            editor_operator_cut_for_agent_rebase(&file, base, &poisoned, "test_reconnect");
        assert_eq!(recovered, operator_cut);
        assert!(recovered.contains("queue: stop"));
        assert_eq!(recovered.matches("agent:boundary:").count(), 1);
        assert_eq!(
            editor_operator_cut_for_agent_rebase(&file, base, &unclosed_exchange, "test_reconnect"),
            operator_cut
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

        let err = apply_cpc_write_through_relay_authority(
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

    fn push_test_liveness(
        _file: &std::path::Path,
        _document_hash: &str,
        ops: &[agent_doc_reliable_sync_io::liveness::LivenessOp],
    ) {
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .unwrap()
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

    fn ack_next_crdt_delivery(
        file: std::path::PathBuf,
        identity: &'static str,
    ) -> std::thread::JoinHandle<()> {
        ack_crdt_deliveries(file, identity, 1, std::time::Duration::ZERO)
    }

    fn ack_crdt_deliveries(
        file: std::path::PathBuf,
        identity: &'static str,
        count: usize,
        initial_delay: std::time::Duration,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            std::thread::sleep(initial_delay);
            let mut acked = 0usize;
            loop {
                let pull = test_support_pull_replica_updates_for_file(&file, identity)
                    .expect("pull CRDT delivery")
                    .expect("test editor remains attached");
                if let Some(update) = pull.updates.last() {
                    assert_eq!(
                        test_support_ack_replica_update_for_file(
                            &file,
                            identity,
                            &update.patch_id,
                            update.generation,
                        )
                        .expect("ACK CRDT delivery"),
                        Some(true),
                    );
                    // Model the editor's native post-ACK save. Delivery ACK and
                    // disk projection are separate protocol transitions in
                    // production; most convergence fixtures want both, while the
                    // retained-capture regression below exercises the gap between
                    // them explicitly.
                    let canonical = agent_doc_crdt_relay_io::with_hub(&file, |hub| {
                        hub.canonical_text().to_string()
                    })
                    .expect("read canonical editor buffer after ACK");
                    std::fs::write(&file, canonical)
                        .expect("simulate native editor save after ACK");
                    acked += 1;
                    if acked == count {
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
    fn canonical_replace_waits_for_visible_replica_ack() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n### Re: settled\n\nDone.\n";
        let (dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-crdt-visible-ack";
        seed_reliable_sync_open(&file, identity);
        test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        let ack = ack_next_crdt_delivery(file.clone(), identity);

        let write =
            apply_canonical_replace_if_attached(&file, baseline, target, "test_crdt_visible_ack")
                .unwrap()
                .expect("attached CRDT write");
        ack.join().unwrap();

        assert!(write.delivery_converged);
        assert_eq!(write.content_hash, agent_doc_hash::content_hash(target));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), target);
        assert_eq!(
            pending_document_write(&file)
                .expect("delivery ACK alone must keep the write intent durable")
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
                .contains("transport=crdt_only")
        );
    }

    #[test]
    fn compact_exchange_coalesces_prior_ack_backpressure_without_secondary_recovery() {
        // Regression for the live JB failure: a response was already visible in
        // the editor but its ACK was lost, so Compact Exchange sat behind
        // `prior_delivery_ack_pending` for a full minute. The next target is safe
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
            apply_cpc_write_through_relay_authority(&file, baseline, source, "seed_prior_delivery")
                .unwrap()
                .expect("seed write should use attached CRDT relay");
        assert!(
            !prior.delivery_converged,
            "the prior frontier must await ACK"
        );

        let ack = ack_crdt_deliveries(
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
            "fixture must exercise a delayed prior delivery ACK"
        );
        assert!(write.delivery_converged);
        assert_eq!(write.content_hash, agent_doc_hash::content_hash(compacted));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_crdt_convergence_wait")
                && log.contains("compact_crdt_relay_acknowledged")
                && !log.contains("compact_crdt_ack_recovery_signal"),
            "compact should converge the retained target through the cumulative Lazily ACK:\n{log}"
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
            started.elapsed() >= std::time::Duration::from_millis(CRDT_ACK_RECOVERY_TIMEOUT_MS),
            "the fixture must cross the foreground ACK deadline"
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
        assert!(!log.contains("did not settle within"), "{log}");
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
        let ack = ack_next_crdt_delivery(file.clone(), identity);

        let write =
            apply_canonical_replace_if_attached(&file, baseline, &agent, "test_crdt_rebase")
                .unwrap()
                .expect("attached CRDT write");
        ack.join().unwrap();
        let current = match agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("expected current CRDT text, got {other:?}"),
        };

        assert!(write.delivery_converged);
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
        let ack = ack_next_crdt_delivery(file.clone(), identity);

        atomic_write_through_authority(&file, target).unwrap();
        ack.join().unwrap();

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
        assert!(log.contains("transport=crdt_editor_native_save"));
        assert!(log.contains("disk_rewritten=false"));
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

        install_post_delivery_proof_hook(file.clone(), move |hook_file| {
            agent_doc_crdt_relay_io::with_hub(hook_file, |hub| {
                let current = hub.canonical_text();
                let operator = current.replace(
                    "- existing queue\n",
                    "- existing queue\n- operator typed after delivery proof\n",
                );
                hub.apply_local(client_id, 0, current.chars().count() as u32, &operator)
                    .unwrap();
            })
            .unwrap();
        });
        let ack = ack_crdt_deliveries(file.clone(), identity, 2, std::time::Duration::ZERO);

        atomic_write_through_authority(&file, &target).unwrap();
        ack.join().unwrap();

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
        assert!(log.contains("serialized_atomic_write_post_proof_rebase"));
        assert!(log.contains("action=rebase_same_intent"));
        assert!(log.contains("post_proof_rebases=1"));
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
        assert!(
            format!("{err:#}").contains("editor_replica_reregister=requested"),
            "zero-replica recovery must request editor replica re-registration: {err:#}"
        );
        let recycle_request =
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .expect("zero-replica write must request automatic supervisor recovery");
        assert_eq!(
            recycle_request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE,
        );
        assert!(!dir.path().join(".agent-doc/crdt-replica-events").exists());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            baseline,
            "the editor-owned file projection must not change behind JetBrains"
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
    fn repair_cas_projects_retained_target_when_editor_owner_has_zero_replicas() {
        let baseline = "# Session\n\nfragmented response\n<!-- agent:boundary:old --><!-- agent:boundary:old -->\n";
        let first_target = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        let final_target = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let (_dir, file, _canonical) = temp_doc(baseline);
        let identity = "test-repair-zero-replica-projection";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.deregister(client_id)).unwrap();

        atomic_repair_write_if_current_through_authority(
            &file,
            first_target,
            baseline,
            "repair_zero_replica_test",
        )
        .unwrap();
        atomic_repair_write_if_current_through_authority(
            &file,
            final_target,
            first_target,
            "repair_zero_replica_final_normalization_test",
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), final_target);
        assert_eq!(
            try_resolve_current_document_content(&file, "repair_zero_replica_verify").unwrap(),
            final_target,
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
            .expect("force-disk repair must preserve editor reconnect lineage");
        assert_eq!(pending.target_content, final_target);
        assert_eq!(
            pending.reason,
            DocumentWriteDeferredReason::RetainEditorReconnectLineageBeforeDiskProjection
        );
        let external = projection
            .document(&document_hash)
            .and_then(|document| document.document.pending_external_disk.as_ref())
            .expect("zero-replica disk projection must remain a separate editor decision");
        assert_eq!(external.target_content, final_target);
        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline)
                .unwrap()
                .as_deref(),
            None,
            "a stale editor cut must remain mutation-free while the disk projection is pending",
        );
        assert_eq!(
            deferred_document_write_reconnect_content(&file, final_target)
                .unwrap()
                .as_deref(),
            Some(final_target),
            "the editor-visible accepted target may propagate exactly",
        );
        assert!(
            clear_pending_external_disk_decision_after_editor_propagation(
                &file,
                final_target,
                "repair_zero_replica_editor_propagated_test",
            )
            .unwrap()
        );
        assert!(pending_external_disk_candidate(&file).is_none());

        clear_deferred_document_write_intent(
            &file,
            &agent_doc_hash::content_hash(final_target),
            "repair_zero_replica_delivery_ack_test",
        )
        .unwrap();
        assert!(
            pending_document_write(&file).is_none(),
            "settling the final reconnect target must not uncover an older intermediate repair"
        );
    }

    #[test]
    fn zero_replica_repair_projects_semantic_rebase_over_newer_operator_cut() {
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

        let materialized = atomic_repair_write_if_current_through_authority(
            &file,
            &response_target,
            editor_base,
            "repair_zero_replica_semantic_rebase_test",
        )
        .unwrap();
        race.join().unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), materialized);
        assert!(materialized.contains("Prompt typed during repair."));
        assert!(!materialized.contains("#deleted"));
        assert_eq!(
            materialized
                .matches("### Re: Original prompt. — gpt-5")
                .count(),
            1
        );
        assert_eq!(materialized.matches("agent:boundary:").count(), 1);
        let pending = pending_document_write(&file)
            .expect("rebased repair target must remain available for editor reconnect");
        assert_eq!(pending.target_content, materialized);
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
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            latest_target,
            latest_target,
            "latest_response_intent_test",
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
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
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
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
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
        )
        .unwrap();
        ensure_deferred_document_write_intent(
            &file,
            base,
            &newest_target,
            "deferred_boundary_composition_newest",
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
        )
        .unwrap();

        let pending = pending_document_write(&file).expect("newest deferred target retained");
        assert!(pending.target_content.contains("agent:boundary:newest"));
        assert!(!pending.target_content.contains("agent:boundary:first"));
        assert!(!pending.target_content.contains("agent:boundary:base"));
        assert_eq!(pending.target_content.matches("agent:boundary:").count(), 1);
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
                    source: "test".to_string(),
                    reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
                },
            );
            agent_doc_controller_io::project_controller::append_state_event(
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
            committed,
        );
        let pending = pending_document_write(&file).expect("retained delivery intent");
        assert_eq!(pending.expected_content.as_deref(), Some(editor_base));
        assert_eq!(pending.target_content, committed);

        let (_client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should attach");
        let ack = ack_next_crdt_delivery(file.clone(), identity);
        assert!(
            settle_retained_committed_projection_through_authority(
                &file,
                committed,
                editor_base,
                "retained_committed_projection_reattach_test",
            )
            .unwrap(),
            "matching retained committed lineage should resume after replica reattach"
        );
        ack.join().unwrap();

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

        let (_replacement_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should bootstrap from retained canonical");
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

        let ack = ack_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        assert!(
            settle_retained_captured_projection_through_authority(
                &file,
                captured_response,
                "retained_capture_missing_response_replay_test",
            )
            .unwrap(),
            "the binary should replay, ACK, project, and settle the missing response cell"
        );
        ack.join().unwrap();
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

        let ack = ack_crdt_deliveries(file.clone(), identity, 1, std::time::Duration::ZERO);
        let settled_synchronously = settle_retained_captured_projection_through_authority(
            &file,
            captured_response,
            "retained_capture_operator_save_rebase_test",
        )
        .unwrap();
        ack.join().unwrap();

        let rebased = try_resolve_current_document_content(
            &file,
            "retained_capture_operator_save_rebased_current",
        )
        .unwrap();
        assert!(rebased.contains("operator item saved before recovery"));
        assert_eq!(rebased.matches("Fixed the retained closeout.").count(), 1);
        if !settled_synchronously {
            assert_eq!(std::fs::read_to_string(&file).unwrap(), operator_saved_cut);
            std::fs::write(&file, &rebased).expect("simulate the requested native editor save");
            assert!(
                settle_retained_captured_projection_through_authority(
                    &file,
                    captured_response,
                    "retained_capture_operator_save_after_native_save_test",
                )
                .unwrap(),
                "the binary should settle the same retained capture without preflight repair"
            );
        }
        assert!(pending_document_write(&file).is_none());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), rebased);
    }

    #[test]
    fn retained_non_capture_projection_requests_native_save_then_settles_exactly() {
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

        let (_replacement_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("replacement editor replica should bootstrap from retained canonical");
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
                DocumentWriteDeferredReason::CrdtDeliveryAckPending,
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
                    source: source.to_string(),
                    reason,
                },
            );
            agent_doc_controller_io::project_controller::append_state_event(
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
                source: "queue_mutation".to_string(),
                reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(
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
    fn live_editor_projection_recovers_historical_ack_without_retained_intent() {
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
            .expect("replacement editor replica should bootstrap from retained canonical");
        assert_eq!(
            try_resolve_current_document_content(
                &file,
                "live_editor_projection_historical_ack_current",
            )
            .unwrap(),
            editor_target,
        );

        clear_deferred_document_write_intent(
            &file,
            &agent_doc_hash::content_hash(editor_target),
            "simulate_historical_ack_only_convergence",
        )
        .unwrap();
        assert!(pending_document_write(&file).is_none());
        assert!(
            !settle_live_editor_projection_through_authority(
                &file,
                "live_editor_projection_before_native_save_test",
            )
            .unwrap(),
            "requesting a native save is not itself disk proof",
        );
        assert!(
            !dir.path().join(".agent-doc/patches").exists(),
            "historical ACK recovery must retain a typed save intent without file IPC",
        );

        std::fs::write(&file, editor_target).expect("simulate the editor's native save");
        assert!(
            settle_live_editor_projection_through_authority(
                &file,
                "live_editor_projection_after_native_save_test",
            )
            .unwrap(),
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), editor_target);
        assert!(pending_document_write(&file).is_none());
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
    fn committed_transient_split_without_retained_lineage_settles_after_upgrade() {
        let stale_disk = "# Session\n\ncomplete response\n<!-- agent:boundary:old -->\n";
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let (_dir, file, _canonical) = temp_doc(stale_disk);
        let identity = "test-historical-committed-transient-split";
        seed_reliable_sync_open(&file, identity);
        let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, identity)
            .unwrap()
            .expect("editor replica should attach");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            hub.apply_local(client_id, 0, stale_disk.chars().count() as u32, committed)
                .unwrap();
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), stale_disk);
        assert!(pending_document_write(&file).is_none());

        let ack = ack_next_crdt_delivery(file.clone(), identity);
        assert!(
            settle_retained_committed_projection_through_authority(
                &file,
                committed,
                stale_disk,
                "historical_committed_transient_split_test",
            )
            .unwrap(),
            "a historical authority/disk split that differs only by transient markers should settle without repair",
        );
        ack.join().unwrap();

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
            DocumentWriteDeferredReason::CrdtDeliveryAckPending,
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
            agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)
                .expect("append visible-write proof event");
        }
    }

    #[test]
    fn durable_buffer_state_reads_cpc_crdt_current_text() {
        let disk = "## Queue\n- do [#a]\n";
        let buffer = "## Queue\n- do [#a]\n- do [#rtwatch]\n";
        let (_dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-cpc-authority");
        let (_client_id, _bootstrap) =
            test_support_register_replica_for_file(&file, "test-cpc-authority")
                .unwrap()
                .expect("editor-attached replica registers");
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            let client_id =
                agent_doc_document_realtime::crdt_relay::mint_client_id("test-cpc-authority");
            hub.apply_local(client_id, 0, disk.chars().count() as u32, buffer)
                .unwrap();
        })
        .unwrap();

        let state = durable_buffer_state(&file, disk).expect("CPC relay buffer wins");
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
        // No CPC model (no editor attached) → disk is the only source.
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

    #[test]
    fn current_resolve_refuses_disk_when_editor_model_missing() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-editor-authority-message");

        let error = try_resolve_current_doc_from_file(&file).unwrap_err();
        assert!(error.to_string().contains("editor_attached_model_missing"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("realtime_doc_resolve_disk_fallback"),
            "an attached editor with a missing model must never demote to disk:\n{log}"
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
    fn current_resolve_refuses_disk_when_editor_sync_pending() {
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

        let error = try_resolve_current_doc_from_file(&file).unwrap_err();
        assert!(error.to_string().contains("editor_sync_pending"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("realtime_doc_resolve_disk_fallback"),
            "an attached editor with a sync-pending model must never demote to disk:\n{log}"
        );
        assert!(
            !log.contains("document_model_ensure_start"),
            "current-doc sync-pending resolution must not enter model ensure:\n{log}"
        );
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
            log.contains("realtime_doc_resolve_crdt_no_live_editors_disk_authority"),
            "closed-editor disk demotion should be auditable:\n{log}"
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
