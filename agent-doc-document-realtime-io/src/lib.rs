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
//! relay/disk adapter, legacy sidecar compatibility, and ops-log/IPC side
//! effects. Cycle read sites (`preflight.rs` / `write.rs` /
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
/// wedged, falling back to disk authority instead of timing out every poll.
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
static DOCUMENT_WRITE_INTENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DOCUMENT_AUTHORITY_OBSERVATIONS: LazyLock<
    Mutex<HashMap<PathBuf, DocumentAuthorityObservation>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
const CURRENT_DOC_DISK_FALLBACK_DEBOUNCE_MS: u64 = 500;

#[cfg(test)]
const CRDT_WRITE_SETTLE_MS: u64 = 10;
#[cfg(not(test))]
const CRDT_WRITE_SETTLE_MS: u64 = 500;
#[cfg(test)]
const CRDT_WRITE_CONVERGENCE_TIMEOUT_MS: u64 = 2_000;
#[cfg(not(test))]
const CRDT_WRITE_CONVERGENCE_TIMEOUT_MS: u64 = 60_000;
const CRDT_WRITE_BACKOFF_INITIAL_MS: u64 = 25;
const CRDT_WRITE_BACKOFF_MAX_MS: u64 = 250;
const CRDT_ACK_REPLAY_SIGNAL_INTERVAL_MS: u64 = 250;
#[cfg(test)]
const CRDT_ACK_FORCE_REFRESH_AFTER_MS: u64 = 500;
#[cfg(not(test))]
const CRDT_ACK_FORCE_REFRESH_AFTER_MS: u64 = 2_000;
#[cfg(test)]
const CRDT_ACK_RECOVERY_TIMEOUT_MS: u64 = 1_800;
#[cfg(not(test))]
const CRDT_ACK_RECOVERY_TIMEOUT_MS: u64 = 8_000;

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
}

impl AckRecoveryState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn wait(&mut self, file: &Path, source: &str, live_editors: usize) -> Result<()> {
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
            anyhow::bail!(
                "{source}: editor delivery ACK recovery for {} did not settle within {}ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)",
                file.display(),
                CRDT_ACK_RECOVERY_TIMEOUT_MS,
            );
        }
        Ok(())
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

    fn publish_live_buffer_content(&self, file: &Path, source: &str) -> Result<Option<String>> {
        publish_fresh_live_buffer_content(file, source)
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

    fn guard_visible_write_idle_and_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()> {
        guard_visible_write_idle_and_current(file, source, expected_current)
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

    fn cycle_already_committed(&self, file: &Path) -> Option<String> {
        agent_doc_flow_io::closeout::cycle_already_committed(file)
    }

    fn log_file_ipc_already_committed(&self, file: &Path, _cycle_id: &str) {
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::AlreadyCommitted,
        );
    }

    fn cleanup_fallback_patch_files(&self, file: &Path) {
        agent_doc_flow_io::closeout::cleanup_fallback_patch_files(file);
    }

    fn file_ipc_patch_rejected(&self, file: &Path, patch_id: &str) -> Option<String> {
        let project_root = agent_doc_project_root_io::project_root_containing(file)?;
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(
                &project_root,
            )
            .ok()?;
        let document = projection.document(&document_hash)?;
        let patch = document.transport.patches.get(patch_id)?;
        if patch.phase == agent_doc_state_backbone::TransportPatchPhase::Rejected {
            return Some(
                document
                    .transport
                    .last_rejected_reason
                    .clone()
                    .unwrap_or_else(|| "editor_patch_rejected".to_string()),
            );
        }
        None
    }

    fn log_file_ipc_proof_failure(
        &self,
        file: &Path,
        patch_id: Option<&str>,
        invariant: &str,
        recovery: &str,
        detail: &str,
    ) {
        eprintln!(
            "[write] IPC proof insufficient for {}: source=file_ipc patch_id={} invariant={} recovery={}{}{}",
            file.display(),
            patch_id.unwrap_or("-"),
            invariant,
            recovery,
            if detail.is_empty() { "" } else { " " },
            detail
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_proof_insufficient file={} source=file_ipc patch_id={} invariant={} recovery={}{}{}",
                file.display(),
                patch_id.unwrap_or("-"),
                invariant,
                recovery,
                if detail.is_empty() { "" } else { " " },
                detail
            ),
        );
    }
}

fn publish_fresh_live_buffer_content(file: &Path, source: &str) -> Result<Option<String>> {
    #[cfg(any(test, feature = "test-support"))]
    const PUBLISH_TIMEOUT_MS: u64 = 100;
    #[cfg(not(any(test, feature = "test-support")))]
    const PUBLISH_TIMEOUT_MS: u64 = 1_000;

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let path = canonical.to_string_lossy().to_string();
    let before: std::collections::HashMap<Option<String>, u64> =
        agent_doc_debounce::live_buffer_snapshots(&path)
            .into_iter()
            .map(|snapshot| (snapshot.editor_id, snapshot.edit_epoch))
            .collect();
    let timeout = std::time::Duration::from_millis(PUBLISH_TIMEOUT_MS);
    agent_doc_crdt_relay_io::request_document_model_live_buffer_publish_with_timeout(
        &canonical, source, timeout,
    )?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        let fresh = agent_doc_debounce::live_buffer_snapshots(&path)
            .into_iter()
            .filter(|snapshot| {
                snapshot.content.is_some()
                    && snapshot.len == snapshot.content.as_deref().map(str::len).unwrap_or(0)
                    && before
                        .get(&snapshot.editor_id)
                        .is_none_or(|epoch| snapshot.edit_epoch > *epoch)
            })
            .max_by_key(|snapshot| (snapshot.edit_epoch, snapshot.timestamp_ms));
        if let Some(snapshot) = fresh {
            let content = snapshot.content.expect("filtered content-bearing snapshot");
            if agent_doc_hash::content_hash(&content).eq_ignore_ascii_case(&snapshot.hash) {
                clear_deferred_document_write_intent(
                    &canonical,
                    &snapshot.hash,
                    "fresh_live_buffer_publish",
                )?;
                return Ok(Some(content));
            }
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
        if let Some(relay_write) = apply_canonical_replace_if_attached(
            path,
            &projection_base,
            content,
            "serialized_atomic_write",
        )? {
            let canonical = match observe_live_editor_authority_after_model_ensure(
                path,
                "serialized_atomic_write_projection",
            )? {
                agent_doc_crdt_relay_io::CurrentText::Current {
                    text,
                    delivery_converged: true,
                    ..
                } if agent_doc_hash::content_hash(&text) == relay_write.content_hash => text,
                current => {
                    anyhow::bail!(
                        "refusing disk projection for {}: CRDT canonical advanced after its delivery proof ({current:?}); retry through the document actor",
                        path.display(),
                    );
                }
            };
            atomic_write_authority_raw(path, &canonical)?;
            agent_doc_ops_log_io::log_op(
                path,
                &format!(
                    "write_authority action=materialized transport=crdt_then_disk_projection len={} hash={} delivery_converged=true",
                    canonical.len(),
                    relay_write.content_hash,
                ),
            );
            return Ok(());
        }
    }

    atomic_write_authority_raw(path, content)
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

    retain_force_disk_reconnect_intent(path, content)?;
    atomic_write_authority_raw(path, content)
}

pub fn atomic_write_if_current_through_authority(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<()> {
    guard_visible_write_idle_current_or_target(path, source, expected_current, Some(content))?;
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
) -> Result<()> {
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
            return Ok(());
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
    anyhow::ensure!(
        canonical == content,
        "{source}: zero-replica repair target for {} was not retained exactly (expected_hash={}, canonical_hash={}); refusing force-disk projection",
        path.display(),
        agent_doc_hash::content_hash(content),
        agent_doc_hash::content_hash(&canonical),
    );
    let pre_force_disk = resolve_disk_current_document_content(path, source)?;
    let reconnect_base = pending_document_write(path)
        .and_then(|pending| {
            pending.expected_content.filter(|expected| {
                agent_doc_hash::content_hash(expected).eq_ignore_ascii_case(&pending.expected_hash)
            })
        })
        .unwrap_or_else(|| pre_force_disk.clone());
    atomic_write_force_disk_through_authority(path, content)?;
    let disk = resolve_disk_current_document_content(path, source)?;
    anyhow::ensure!(
        disk == content,
        "{source}: zero-replica repair projection for {} did not materialize exactly (expected_hash={}, disk_hash={})",
        path.display(),
        agent_doc_hash::content_hash(content),
        agent_doc_hash::content_hash(&disk),
    );
    clear_all_deferred_document_write_intents(path, source)?;
    if reconnect_base != content {
        ensure_deferred_document_write_intent(
            path,
            &reconnect_base,
            content,
            "repair_force_disk",
            DocumentWriteDeferredReason::RetainEditorReconnectLineageBeforeDiskProjection,
        )?;
    }
    agent_doc_ops_log_io::log_op(
        path,
        &format!(
            "{source}_zero_replica_repair_projected file={} content_hash={} authority=retained_crdt_cas transport=audited_force_disk",
            path.display(),
            agent_doc_hash::content_hash(content),
        ),
    );
    Ok(())
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
    if controller_document_mutation_in_progress() || test_local_crdt_relay_enabled(file) {
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
    if test_local_crdt_relay_enabled(file) {
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
    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(expected_current).encode_state();
    let mut backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
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
                "{source}: CRDT convergence for {} did not settle within {}ms (reason={}); pending change retained for retry",
                file.display(),
                CRDT_WRITE_CONVERGENCE_TIMEOUT_MS,
                wait_state,
            );
        }

        // A CPC write is issued only from a quiescent editor cut. Waiting here
        // happens in the caller, outside the controller RPC loop, so editor
        // deltas and delivery ACKs remain responsive while typing settles.
        if pending_target.is_none() {
            let remaining_ms = CRDT_WRITE_CONVERGENCE_TIMEOUT_MS.saturating_sub(elapsed_ms);
            guard_visible_write_idle_with_budget(
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
                            let mut relay_write = pending_write
                                .take()
                                .expect("pending CRDT target must retain its write receipt");
                            relay_write.delivery_converged = true;
                            relay_write.live_editors = live_editors;
                            clear_deferred_document_write_intent(
                                file,
                                &relay_write.content_hash,
                                source,
                            )?;
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "{source}_crdt_relay_materialized file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true wait_ms={} transport=crdt_only",
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
                            ack_recovery.wait(file, source, live_editors)?;
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
                        let effective_target =
                            if relay_text == expected_current || relay_text == content {
                                content.to_string()
                            } else {
                                agent_doc_merge::crdt::merge_by_component(
                            Some(&base_state),
                            content,
                            &relay_text,
                        )
                        .with_context(|| {
                            format!(
                                "{source}: failed to CRDT-merge the settled editor version for {}",
                                file.display()
                            )
                            })?
                            };

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
                                    "{source}: deferred write for {} in Lazily state (intent_id={intent_id}): the editor owns the document but no relay replica is registered; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_retry_finalize",
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
                                    "{source}: retained the canonical write for {} in CRDT + Lazily state (intent_id={intent_id}), but no editor replica was registered to receive it; disk was not written; supervisor_recycle={recycle_status}; recovery=await_editor_replica_no_disk_write_then_retry_finalize",
                                    file.display(),
                                )));
                            }
                            Ok(Some(mut relay_write)) if relay_write.delivery_converged => {
                                relay_write.live_editors = live_editors;
                                clear_deferred_document_write_intent(
                                    file,
                                    &relay_write.content_hash,
                                    source,
                                )?;
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "{source}_crdt_relay_materialized file={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged=true wait_ms={} transport=crdt_only",
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
                                pending_write = Some(relay_write);
                                ack_recovery.reset();
                                backoff_ms = CRDT_WRITE_BACKOFF_INITIAL_MS;
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
        backoff_ms = backoff_ms.saturating_mul(2).min(CRDT_WRITE_BACKOFF_MAX_MS);
    }
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
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let write_id = uuid::Uuid::new_v4().to_string();
    let hash = agent_doc_hash::content_hash(content);
    if let Err(e) = agent_doc_debounce::record_write_provenance(
        &canonical,
        content.len(),
        &hash,
        &write_id,
        "agent",
    ) {
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
// Plugin reports are transport inputs only; file-backed `.agent-doc/live-buffer`
// sidecars are not authoritative recovery state and are not promoted here.

fn next_document_authority_epoch() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    loop {
        let current = DOCUMENT_AUTHORITY_EPOCH.load(Ordering::Relaxed);
        let next = now.max(current.saturating_add(1));
        match DOCUMENT_AUTHORITY_EPOCH.compare_exchange(
            current,
            next,
            Ordering::SeqCst,
            Ordering::Relaxed,
        ) {
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

fn ensure_deferred_document_write_intent(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
    reason: DocumentWriteDeferredReason,
) -> Result<String> {
    let mut expected_content = expected_current.to_string();
    let mut target_content = content.to_string();
    let requested_target_hash = agent_doc_hash::content_hash(content);
    if let Some(pending) = pending_document_write(file) {
        if pending
            .target_hash
            .eq_ignore_ascii_case(&requested_target_hash)
        {
            return Ok(pending.intent_id);
        }

        let expected_hash = agent_doc_hash::content_hash(expected_current);
        if !pending.target_hash.eq_ignore_ascii_case(&expected_hash) {
            let legacy_disk_base = std::fs::read_to_string(file).ok().filter(|disk| {
                agent_doc_hash::content_hash(disk).eq_ignore_ascii_case(&pending.expected_hash)
            });
            let merge_base = pending
                .expected_content
                .clone()
                .filter(|base| {
                    agent_doc_hash::content_hash(base).eq_ignore_ascii_case(&pending.expected_hash)
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
            let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(&merge_base).encode_state();
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
    let target_hash = agent_doc_hash::content_hash(&target_content);
    if let Some(pending) = pending_document_write_for_target(file, &target_hash) {
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
    let Some(pending) = pending_document_write(file) else {
        return Ok(None);
    };
    let editor_hash = agent_doc_hash::content_hash(editor_content);
    if editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
        return Ok(Some(pending.target_content));
    }

    let legacy_disk_base = std::fs::read_to_string(file).ok().filter(|disk| {
        agent_doc_hash::content_hash(disk).eq_ignore_ascii_case(&pending.expected_hash)
    });
    let base = pending
        .expected_content
        .clone()
        .filter(|content| {
            agent_doc_hash::content_hash(content).eq_ignore_ascii_case(&pending.expected_hash)
        })
        .or(legacy_disk_base)
        .with_context(|| {
            format!(
                "deferred write {} for {} has no content-bearing merge base",
                pending.intent_id,
                file.display()
            )
        })?;
    if editor_hash.eq_ignore_ascii_case(&pending.expected_hash) {
        return Ok(Some(pending.target_content));
    }

    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(&base).encode_state();
    let merged = agent_doc_merge::crdt::merge_by_component(
        Some(&base_state),
        &pending.target_content,
        editor_content,
    )
    .with_context(|| {
        format!(
            "failed to merge reappearing editor content with deferred write {} for {}",
            pending.intent_id,
            file.display()
        )
    })?;
    if agent_doc_hash::content_hash(&merged).eq_ignore_ascii_case(&pending.target_hash) {
        return Ok(Some(pending.target_content));
    }
    ensure_deferred_document_write_intent(
        file,
        &pending.target_content,
        &merged,
        "editor_reconnect",
        DocumentWriteDeferredReason::MergeUnsavedEditorCutWithDeferredTarget,
    )?;
    Ok(Some(merged))
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
        DocumentWriteDeferredReason::RetainEditorReconnectLineageBeforeDiskProjection,
    )?;
    Ok(())
}

fn clear_deferred_document_write_intent(
    file: &Path,
    target_hash: &str,
    source: &str,
) -> Result<()> {
    let Some(pending) = pending_document_write_for_target(file, target_hash) else {
        return Ok(());
    };
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
fn clear_all_deferred_document_write_intents(file: &Path, source: &str) -> Result<()> {
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
    if let Some(current) = query_test_local_crdt_relay(file, source)? {
        return Ok(current);
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

fn query_test_local_crdt_relay(
    file: &std::path::Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    if !test_local_crdt_relay_enabled(file) {
        return Ok(None);
    }
    let current = agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "document_model_test_local_relay_observed file={} source={} status={} reason=simworld_test_override",
            file.display(),
            source,
            current_text_status(&current)
        ),
    );
    Ok(Some(current))
}

fn test_local_crdt_relay_enabled(file: &std::path::Path) -> bool {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
        .or_else(|| file.parent().map(std::path::Path::to_path_buf))
    else {
        return false;
    };
    project_root
        .join(".agent-doc/test-local-crdt-relay")
        .is_file()
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

fn typing_indicator_status_label(
    status: agent_doc_debounce::TypingIndicatorStatus,
) -> &'static str {
    match status {
        agent_doc_debounce::TypingIndicatorStatus::Absent => "absent",
        agent_doc_debounce::TypingIndicatorStatus::Active => "active",
        agent_doc_debounce::TypingIndicatorStatus::Idle => "idle",
    }
}

fn resolve_idle_disk_fallback_current_doc(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
    reason: &str,
    detail: Option<&str>,
) -> Result<Reconciliation> {
    let file_str = file.to_string_lossy();
    let typing_status = agent_doc_debounce::typing_indicator_status(
        &file_str,
        CURRENT_DOC_DISK_FALLBACK_DEBOUNCE_MS,
    );
    if typing_status == agent_doc_debounce::TypingIndicatorStatus::Active {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_doc_resolve_deferred file={} source={} reason={} typing_status={} fallback=none",
                file.display(),
                source,
                reason,
                typing_indicator_status_label(typing_status),
            ),
        );
        anyhow::bail!(
            "editor authority unavailable for {}; editor typing is active, so disk is not consulted as a fallback",
            file.display()
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
            "realtime_doc_resolve_disk_fallback file={} source={} reason={} typing_status={} detail={}",
            file.display(),
            source,
            reason,
            typing_indicator_status_label(typing_status),
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
    agent_doc_crdt_relay_io::defer_if_document_model_ensure_suppressed(file, source)?;
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
    if test_local_crdt_relay_enabled(file) {
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

/// Canonical path string used to key the editor-buffer sidecar lookup. Mirrors
/// the convention the visible-write reconcile guard uses
/// (`guard_visible_write_reconcile`) so the path hashed here matches the
/// absolute path the editor plugin reported the buffer under; a relative vs
/// absolute mismatch would silently miss the sidecar.
fn indicator_path(file: &std::path::Path) -> String {
    file.canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string()
}

const VISIBLE_WRITE_TYPING_DEBOUNCE_MS: u64 = 500;
const VISIBLE_WRITE_TYPING_TIMEOUT_MS: u64 = 5_000;

/// Max re-merge attempts when reconciling the visible-write guard with a
/// foreign disk write that landed after the merge was computed
/// (#ipc-drift-visbuf-reconcile). After this many drifting re-reads, fall back
/// to the fail-closed guard so the operator retries.
pub const VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS: usize = 3;

pub fn guard_visible_write_idle(file: &std::path::Path, source: &str) -> Result<()> {
    guard_visible_write_idle_with_budget(
        file,
        source,
        VISIBLE_WRITE_TYPING_DEBOUNCE_MS,
        VISIBLE_WRITE_TYPING_TIMEOUT_MS,
    )
}

pub fn guard_visible_write_idle_with_budget(
    file: &std::path::Path,
    source: &str,
    debounce_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let indicator_path = indicator_path(file);
    let idle_reached =
        agent_doc_debounce::await_idle_via_file(&indicator_path, debounce_ms, timeout_ms);
    let facts = write_policy::VisibleWriteTypingFacts {
        idle_reached,
        timeout_ms,
    };
    let decision = write_policy::decide_visible_write_after_typing(facts);
    agent_doc_flow_io::log_flow_event(
        file,
        write_policy::visible_write_guard_event(decision, source),
        agent_doc_ops_log_io::log_op,
    );
    if decision == write_policy::VisibleWriteDecision::Apply {
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_deferred_active_typing file={} source={} debounce_ms={} timeout_ms={}",
            file.display(),
            source,
            debounce_ms,
            timeout_ms
        ),
    );
    anyhow::bail!(
        "visible document write for {} deferred: editor typing did not settle within {}ms; retry after typing stops",
        file.display(),
        timeout_ms
    )
}

pub fn guard_visible_write_idle_and_current(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    guard_visible_write_idle_current_or_target(file, source, expected_current, None)
}

pub fn guard_visible_write_idle_current_or_target(
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

/// Like [`guard_visible_write_idle_and_current`] but, instead of failing closed
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
    guard_visible_write_idle(file, source)?;
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
/// editor model is current. File-backed live-buffer sidecars are intentionally
/// ignored: they are plugin projections/telemetry, not recovery authority.
pub fn durable_buffer_state(file: &std::path::Path, disk: &str) -> Option<BufferState> {
    let current = if test_local_crdt_relay_enabled(file) {
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
/// safety predicate the WRITE/FINALIZE compatibility gate keys off of. It never
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
/// "disk replica" means the real session document file on disk, not the legacy
/// `.agent-doc/live-buffer` sidecar. Disk is used only after the relay reports
/// the editor is detached.
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
/// read before active editor authority is checked. The legacy live-buffer
/// sidecar is compatibility/diagnostic state only and is not a disk replica.
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

fn try_resolve_current_doc_with_disk_inner(
    file: &std::path::Path,
    disk: Option<&str>,
    source: &str,
    require_model_ensure: bool,
) -> Result<Reconciliation> {
    let current = match if require_model_ensure {
        query_live_editor_authority_after_model_ensure(file, source)
    } else {
        query_live_editor_authority(file, source)
    } {
        Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica) => {
            return resolve_idle_disk_fallback_current_doc(
                file,
                disk,
                source,
                if require_model_ensure {
                    "model_ensure_missing_replica"
                } else {
                    "missing_replica"
                },
                None,
            );
        }
        Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => {
            return resolve_idle_disk_fallback_current_doc(
                file,
                disk,
                source,
                if require_model_ensure {
                    "model_ensure_sync_pending"
                } else {
                    "sync_pending"
                },
                None,
            );
        }
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
            return resolve_idle_disk_fallback_current_doc(
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
                // through the reactive open-docs projection (reconciled controller-side
                // from the durable live-buffer sidecar ground truth) instead of demoting
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
                return Ok(resolve_disk_only_current_doc(
                    file,
                    &disk,
                    "crdt_relay_no_live_editors",
                ));
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

/// Compatibility wrapper for tests and legacy callers that cannot yet surface a
/// resolver error. Production cycle reads should call
/// [`try_resolve_current_doc_from_file`] so active-editor relay gaps are not
/// masked by disk fallback.
pub fn resolve_current_doc(file: &std::path::Path, disk: &str) -> Reconciliation {
    match try_resolve_current_doc(file, disk) {
        Ok(reconciliation) => reconciliation,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "realtime_doc_resolve_legacy_fallback file={} error={}",
                    file.display(),
                    e,
                ),
            );
            resolve_detached_current_doc(file, disk)
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
    /// string — the sidecar must be recorded under the same canonical key the
    /// feed canonicalizes to, exactly as the live editor plugin reports it.
    fn temp_doc(disk: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/test-local-crdt-relay"), "").unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, disk).unwrap();
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    fn seed_reliable_sync_open(file: &std::path::Path, tag: &str) {
        let document_hash = agent_doc_hash::document_id_for_path(file);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .unwrap()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: tag.to_string(),
            }]);
    }

    fn seed_reliable_sync_close(file: &std::path::Path, tag: &str) {
        let document_hash = agent_doc_hash::document_id_for_path(file);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .unwrap()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Close {
                document_hash,
                pid: std::process::id().into(),
                observed_tags: vec![tag.to_string()],
            }]);
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
        assert!(
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap()
                .contains("transport=crdt_only")
        );
    }

    #[test]
    fn compact_exchange_coalesces_prior_ack_backpressure_and_nudges_recovery() {
        // Regression for the live JB failure: a response was already visible in
        // the editor but its ACK was lost, so Compact Exchange sat behind
        // `prior_delivery_ack_pending` for a full minute. The next target is safe
        // to queue once: the relay's final-content ACK drains the cumulative
        // prefix. While waiting, the binary actively nudges ACK replay and then a
        // bounded client re-register instead of requiring controller recycling.
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
                && log.contains("compact_crdt_ack_recovery_signal")
                && log.contains("reason=ack_recovery_force_refresh"),
            "compact should retain its target and actively recover the ACK path:\n{log}"
        );
    }

    #[test]
    fn canonical_replace_crdt_rebases_over_settled_operator_text_once() {
        let baseline = concat!(
            "# Session\n\n",
            "<!-- agent:queue -->\n- baseline\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\nReady.\n<!-- /agent:exchange -->\n",
        );
        let operator = baseline.replace("- baseline\n", "- baseline\n- operator edit\n");
        let agent = baseline.replace(
            "Ready.\n<!-- /agent:exchange -->",
            "Ready.\n\n### Re: agent\n\nApplied once.\n<!-- /agent:exchange -->",
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
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("transport=crdt_then_disk_projection"));
    }

    #[test]
    fn serialized_atomic_write_defers_zero_replica_editor_owner_without_touching_disk() {
        let baseline = "# Session\n\nbody\n";
        let target = "# Session\n\nbody\n\n<!-- agent:boundary id=deferred -->\n";
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
        assert!(
            format!("{err:#}").contains("await_editor_replica_no_disk_write"),
            "unexpected error: {err:#}"
        );
        let recycle_request =
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .expect("zero-replica write must request automatic supervisor recovery");
        assert_eq!(
            recycle_request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE,
        );
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
        assert!(merged.contains("agent:boundary id=deferred"));
        assert!(merged.contains("operator note"));
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
        assert_eq!(
            deferred_document_write_reconnect_content(&file, baseline)
                .unwrap()
                .as_deref(),
            Some(final_target),
            "a stale editor must receive only the final normalized repair target",
        );

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
    fn committed_projection_settlement_clears_stale_deferred_lineage() {
        let editor_base = "# Session\n\ncomplete response\n";
        let stale_projection = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let (_dir, file, _canonical) = temp_doc(stale_projection);
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
    fn force_disk_retains_reconnect_lineage_and_merges_reappearing_editor_text() {
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
            .and_then(|document| document.document.pending_write.as_ref())
            .expect("force-disk must retain reconnect lineage");
        assert_eq!(pending.expected_content.as_deref(), Some(baseline));
        assert_eq!(pending.target_content, target);
        assert_eq!(pending.source, "force_disk");

        let editor_with_unsaved_note = format!("{baseline}\noperator note after relay loss\n");
        let merged = deferred_document_write_reconnect_content(&file, &editor_with_unsaved_note)
            .unwrap()
            .expect("reappearing editor should reconcile against force-disk target");
        assert!(merged.contains("### Re: agent"));
        assert!(merged.contains("operator note after relay loss"));
    }

    fn wait_for_active_typing_indicator(file: &str) {
        for _ in 0..100 {
            if agent_doc_debounce::typing_indicator_status(
                file,
                CURRENT_DOC_DISK_FALLBACK_DEBOUNCE_MS,
            ) == agent_doc_debounce::TypingIndicatorStatus::Active
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("typing indicator did not become active for {file}");
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
    fn durable_buffer_state_ignores_live_buffer_sidecar_when_disk_differs() {
        let disk = "## Queue\n- do [#a]\n";
        let (_dir, file, canonical) = temp_doc(disk);
        agent_doc_debounce::record_live_buffer_digest_content(
            &canonical,
            "## Queue\n- do [#a]\n- do [#sidecar]\n",
        )
        .unwrap();
        assert!(durable_buffer_state(&file, disk).is_none());
        let r = resolve_current_doc(&file, disk);
        assert_eq!(r.authority, agent_doc_document_realtime::DocAuthority::Disk);
        assert_eq!(r.content, disk);
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
        let r = resolve_current_doc(&file, disk);
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
        let (_dir, file, _canonical) = temp_doc(disk);
        assert!(durable_buffer_state(&file, disk).is_none());
        assert_eq!(
            resolve_current_doc(&file, disk).authority,
            agent_doc_document_realtime::DocAuthority::Disk
        );
    }

    #[test]
    fn current_resolve_uses_disk_when_editor_model_missing_and_typing_idle() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        seed_reliable_sync_open(&file, "test-editor-authority-message");

        let resolved = try_resolve_current_doc_from_file(&file)
            .expect("idle missing editor model should use the disk session document");
        let repeated = try_resolve_current_doc_from_file(&file)
            .expect("unchanged idle missing editor model should remain disk-authoritative");
        assert_eq!(
            resolved.authority,
            agent_doc_document_realtime::DocAuthority::Disk
        );
        assert_eq!(resolved.content, disk);
        assert_eq!(repeated, resolved);
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
            1,
            "unchanged idle authority observations must be coalesced"
        );
        assert!(
            authority_events[0]
                .payload_json
                .contains("\"authority\":\"disk_replica\"")
        );
        assert!(
            !authority_events[0]
                .payload_json
                .contains("editor_attached_missing_replica"),
            "the durable event must record the final disk fallback, not the transient missing model"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("realtime_doc_resolve_disk_fallback")
                && log.contains("reason=missing_replica"),
            "idle missing-model resolve should log the disk fallback:\n{log}"
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
    fn current_resolve_uses_disk_when_editor_sync_pending_and_typing_idle() {
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

        let resolved = try_resolve_current_doc_from_file(&file)
            .expect("idle sync-pending editor model should use the disk session document");
        assert_eq!(
            resolved.authority,
            agent_doc_document_realtime::DocAuthority::Disk
        );
        assert_eq!(resolved.content, disk);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("realtime_doc_resolve_disk_fallback")
                && log.contains("reason=sync_pending"),
            "idle sync-pending resolve should log the disk fallback:\n{log}"
        );
        assert!(
            !log.contains("document_model_ensure_start"),
            "current-doc sync-pending resolution must not enter model ensure:\n{log}"
        );
    }

    #[test]
    fn current_resolve_active_typing_blocks_missing_model_disk_fallback() {
        let disk = "plain disk body\n";
        let (dir, file, _canonical) = temp_doc(disk);
        let file_str = file.display().to_string();
        seed_reliable_sync_open(&file, "test-editor-authority-suppression");
        agent_doc_debounce::document_changed(&file_str);
        wait_for_active_typing_indicator(&file_str);

        let first = try_resolve_current_doc_from_file(&file)
            .expect_err("active typing should block disk fallback")
            .to_string();
        assert!(
            first.contains("editor typing is active"),
            "active typing error should name the blocker: {first}"
        );
        let log_path = dir.path().join(".agent-doc/logs/ops.log");
        let first_log = std::fs::read_to_string(&log_path).unwrap();
        let first_missing_count = first_log.matches("crdt_current_text_unavailable").count();
        assert!(
            first_missing_count > 0,
            "first resolve should prove the missing replica once:\n{first_log}"
        );
        assert!(
            first_log.contains("fallback=none")
                && !first_log.contains("document_model_ensure_start"),
            "active typing should fail before model-ensure or disk fallback:\n{first_log}"
        );

        agent_doc_debounce::document_changed(&file_str);
        wait_for_active_typing_indicator(&file_str);
        let second = try_resolve_current_doc_from_file(&file)
            .expect_err("active typing should continue to block disk fallback")
            .to_string();
        assert!(
            second.contains("editor typing is active"),
            "second active typing error should name the blocker: {second}"
        );
        let second_log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            second_log.matches("crdt_current_text_unavailable").count() > first_missing_count,
            "fresh retry should emit another missing-replica probe after first_count={first_missing_count}:\n{second_log}"
        );
        assert_eq!(
            second_log.matches("document_model_ensure_start").count(),
            0,
            "active typing should not enter document-model ensure:\n{second_log}"
        );
        assert_eq!(
            second_log
                .matches("realtime_doc_resolve_disk_fallback")
                .count(),
            0,
            "active typing must not use disk fallback:\n{second_log}"
        );
        assert_eq!(
            second_log.matches("fallback=none").count(),
            2,
            "each active-typing attempt should log a no-fallback decision:\n{second_log}"
        );
    }

    // The editor reports Close after deregistering its replica. The reliable-sync plane
    // remains authoritative even though the relay's canonical model still exists.
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
            log.contains("realtime_doc_resolve authority=disk reason=editor_absent"),
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
    fn visible_write_guard_blocks_when_editor_typing_active() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/typing")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "body\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);
        for _ in 0..50 {
            if agent_doc_debounce::is_typing_via_file(&doc_str, 60_000) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let err = guard_visible_write_idle_with_budget(&doc, "test_visible_write", 60_000, 0)
            .unwrap_err();

        assert!(err.to_string().contains("editor typing did not settle"));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_typing_defer_active_typing:test_visible_write"));
        assert!(log.contains("visible_write_deferred_active_typing"));
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

        let err = guard_visible_write_idle_and_current(&doc, "test_current_changed", expected)
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
    fn visible_write_guard_ignores_legacy_digest_when_disk_matches_expected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        let live_buffer = expected.replace(
            "<!-- /agent:exchange -->",
            "prompt typed but not saved\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            live_buffer.len(),
            &agent_doc_hash::content_hash(&live_buffer),
        )
        .unwrap();

        guard_visible_write_idle_and_current(&doc, "test_live_buffer_changed", expected)
            .expect("legacy live-buffer digest must not block when disk matches expected");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), expected);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(!log.contains("visible_write_legacy_live_buffer_ignored"));
        assert!(!log.contains("visible_write_deferred_live_buffer_changed"));
    }

    #[test]
    fn visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift() {
        // #nm1x: the editor reported a buffer that diverges from `expected` but
        // *matches the current on-disk content* (an independent document edit the
        // editor already saved). That is not a pending unsaved user edit, so the
        // guard must not fail closed — it reports the reconcilable DiskDrifted case
        // instead, letting the response re-merge against the fresh disk content.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        // Disk + editor both carry an independent queue edit not present in
        // `expected`; the editor digest equals disk (saved, no pending edit).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#sibling]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, &drifted).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            drifted.len(),
            &agent_doc_hash::content_hash(&drifted),
        )
        .unwrap();

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
    fn visible_write_reconcile_normalizes_transient_live_buffer_markers() {
        // The editor sidecar can lag only in transient agent-doc markers (for
        // example a regenerated boundary id). That is not operator text and must
        // not fail closed as a live-buffer edit.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:boundary:old -->
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        let live = expected.replace("agent:boundary:old", "agent:boundary:new");
        std::fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &live,
            "jetbrains-boundary-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_transient_marker",
            expected,
            None,
        )
        .expect("transient marker churn must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk still matches expected"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("visible_write_live_buffer_matches_disk"),
            "legacy live-buffer sidecars should not be sampled: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "transient marker churn must not trip the fail-closed block: {log}"
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
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
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
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
            &doc_str,
            &target,
            "jetbrains-proof-target",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
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
    fn visible_write_reconcile_ignores_legacy_replica_churn_sidecar() {
        // Legacy live-buffer sidecars are no longer commit authority. A stale or
        // replica-churn sidecar may differ from disk, but without a live CRDT
        // relay it must not fail closed.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        // Disk matches the merge baseline (`expected`). The editor buffer is ahead
        // with a remote replica's converged content that is neither `expected` nor
        // anything the operator typed.
        std::fs::write(&doc, expected).unwrap();
        let replica_ahead = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: from-another-replica\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &replica_ahead,
            "jetbrains-42-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            true,
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_replica_churn", expected, None)
                .expect("legacy sidecar divergence must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk matches the expected current document"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("visible_write_legacy_live_buffer_ignored"),
            "legacy sidecar divergence should not be sampled: {log}"
        );
        assert!(
            !log.contains("visible_write_commit_candidate_applied_reconcile"),
            "unproven replica churn must not be treated as candidate proof: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_ignores_legacy_committed_blob_sidecar() {
        // A legacy live-buffer sidecar that equals a recent committed blob is not
        // document authority. Disk/relay decides the hot path.
        let old = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old\n",
            "<!-- /agent:exchange -->\n",
        );
        let new = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: new\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc, canonical) = temp_git_doc(&[old, new]);
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &canonical,
            old,
            "jetbrains-stale-commit-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_committed_blob", new, None)
                .expect("legacy committed-blob sidecar must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk matches the expected current document"
        );
        let log =
            std::fs::read_to_string(doc.parent().unwrap().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("visible_write_legacy_live_buffer_ignored"),
            "legacy committed-blob sidecar should not be sampled: {log}"
        );
        assert!(
            !log.contains("visible_write_commit_candidate_applied_reconcile"),
            "legacy sidecar must not be treated as candidate proof: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_ignores_legacy_sidecar_and_reports_disk_drift() {
        // Disk drift remains reconcilable. A legacy sidecar cannot turn that
        // legal state into a fail-closed editor-buffer mismatch.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign-append\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &drifted).unwrap();
        let replica_ahead = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: from-another-replica\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &replica_ahead,
            "jetbrains-43-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            true,
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_replica_churn_drift",
            expected,
            None,
        )
        .expect("legacy sidecar divergence with disk drift must not fail closed");
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(!log.contains("visible_write_legacy_live_buffer_ignored"));
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
    }

    #[test]
    fn visible_write_reconcile_ignores_legacy_operator_edit_sidecar() {
        // The sidecar can be stale even when it looks like unsaved operator text.
        // It is no longer the authority source; live CRDT relay state is.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        std::fs::write(&doc, expected).unwrap();
        let operator_typed = expected.replace(
            "<!-- /agent:exchange -->",
            "❯ operator is typing an unsaved question\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &operator_typed,
            "jetbrains-44-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_operator_edit", expected, None)
                .expect("legacy operator-edit sidecar must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk matches the expected current document"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("visible_write_legacy_live_buffer_ignored"),
            "legacy operator-edit sidecar should not be sampled: {log}"
        );
        assert!(
            !log.contains("visible_write_commit_candidate_applied_reconcile"),
            "legacy operator-edit sidecar must NOT be treated as commit-candidate proof: {log}"
        );
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
    /// keys the live-buffer sidecar, matching what the editor plugin reports.
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
