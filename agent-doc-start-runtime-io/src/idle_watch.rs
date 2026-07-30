//! The supervisor idle-queue watch thread and its two dedicated helpers, extracted
//! from `start.rs`. It polls the owned pane on a timer, and on each busy→idle
//! transition decides whether to drain a live `agent:queue` head, fire a context
//! reset, or (`#ctlrecycle` R3) hot-reload a stale supervisor. As a child module
//! of `start` it reaches the supervisor's private `SupervisorShared`, log/sleep
//! helpers, and `supervisor_perform_reexec` directly through `use super::*`.

use super::*;
use agent_doc_queue::queue::{
    CLEAR_COOLDOWN_RESUME_IDLE_TICKS, IdleQueueContextClearInFlightDecision,
    IdleQueueContextClearInFlightFacts, IdleQueueContextClearInFlightSettleFacts,
    IdleQueueContextResetDecision, IdleQueueDrainDecision, IdleQueueDrainDecisionFacts,
    between_turn_enqueue_plan, clean_session_head_forces_context_reset,
    clear_cooldown_resume_ready, drain_blocked_awaiting_clear_settle, drain_dispatch_dedup_skip,
    idle_queue_context_clear_in_flight_decision, idle_queue_context_clear_in_flight_settle_ticks,
    idle_queue_context_reset_decision_with_current_transition,
    idle_queue_drain_decision_with_current_transition, stale_drain_recycle_yield_requested,
};
#[cfg(test)]
use agent_doc_queue::queue::{idle_queue_context_reset_decision, idle_queue_drain_decision};
use agent_doc_supervisor::{
    agent_change::{AgentChangeRestartAction, agent_change_restart_decision},
    idle_reconcile::{
        ready_busy_conflict_reconcile_decision, reconcile_stale_busy_idle_queue_state,
        stale_busy_idle_reconcile_decision,
    },
    idle_revision::{ControllerProbeHealth, IdleRevisionState, RevisionObservation},
    idle_watch::{
        CapturedFinalizeResumeFacts, CapturedFinalizeResumeTriggers, SupervisorAutoInstallPhase,
        captured_finalize_resume_retry_delay, captured_finalize_resume_should_start,
        idle_queue_context_reset_ops_log_message, paused_idle_watch_should_skip,
        supervisor_auto_install_pane_message,
    },
    lifecycle::{
        MAX_CYCLE_OPEN_DEFER_TICKS, MAX_REEXEC_ESCALATIONS, SupervisorInstallAction,
        SupervisorRecycleAction, SupervisorRestartAction, cycle_open_defer_escalates,
        reexec_escalation_within_bound, supervisor_install_action, supervisor_recycle_action,
        supervisor_restart_action,
    },
};
use agent_doc_turn::op_log::OpsLogEvent;

const CLEAN_SESSION_CONTEXT_RESET_REASON: &str = "active queue head is a [clean-session] item - clearing to give it a fresh agent context (#cleandrainsup)";
const FOCUSED_CYCLE_CONTEXT_RESET_REASON: &str = "active queue head is a [focused-cycle] item - clearing to continue in a fresh agent context (#qfocsup)";
const CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED: &str = "operator_deferred_clear";
const CONTEXT_CLEAR_SOURCE_QUEUE_SLASH: &str = "queue_slash_command";
const CONTEXT_CLEAR_SOURCE_BACKGROUND_RESET: &str = "supervisor_background_context_reset";
const ZERO_REPLICA_IDLE_WATCH_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
const ZERO_REPLICA_IDLE_REPAIR_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);
static ZERO_REPLICA_IDLE_WATCH_LAST_PROBE: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<std::path::PathBuf, std::time::Instant>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// `#idlewatchrevisiongate` — the last queue-head observation and the revision it
/// was derived from, per document.
///
/// [`agent_doc_crdt_relay_io::CurrentRevision`]'s own doc comment states the
/// contract: "The idle supervisor compares this value before asking the relay to
/// materialize the canonical markdown. It therefore keeps full-text queue parsing
/// lazy." Both halves shipped — the `crdt_revision` RPC, and `PartialEq` on the
/// revision so it can be compared — but [`idle_watch_active_queue_head`] never
/// made the comparison. It asked for the full canonical text on **every** tick.
///
/// Measured cost of that gap on this project (26 MB `ops.log`, 20k-line window):
/// `crdt_current_text` + `controller_crdt_current_text` were **63% of all
/// controller operations**, peaking at 11/second, and `idle_watch_active_queue_head`
/// plus `current_transition_for_idle_queue` accounted for 56% of them. Each one
/// materializes the whole document (90-128 KB here), SHA-256s it, holds the
/// document's relay-hub lock while doing so, and writes an `ops.log` line — every
/// 500 ms, per attached document, whether or not anything changed.
///
/// The revision is read *before* the text on a miss, so a document that changes
/// between the two reads stores the older revision and simply misses again next
/// tick. Storing the revision observed after the text could pair a new revision
/// with stale text, which is the one ordering that would be wrong.
static IDLE_WATCH_QUEUE_HEAD_BY_REVISION: std::sync::LazyLock<
    parking_lot::Mutex<
        std::collections::HashMap<
            std::path::PathBuf,
            (
                agent_doc_crdt_relay_io::CurrentRevision,
                QueueHeadObservation,
            ),
        >,
    >,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// The memoized observation for `canonical`, if `revision` still matches.
fn memoized_queue_head(
    canonical: &Path,
    revision: &agent_doc_crdt_relay_io::CurrentRevision,
) -> Option<QueueHeadObservation> {
    IDLE_WATCH_QUEUE_HEAD_BY_REVISION
        .lock()
        .get(canonical)
        .filter(|(cached_revision, _)| cached_revision == revision)
        .map(|(_, observation)| observation.clone())
}

fn memoize_queue_head(
    canonical: &Path,
    revision: Option<agent_doc_crdt_relay_io::CurrentRevision>,
    observation: &QueueHeadObservation,
) {
    let Some(revision) = revision else {
        // No cheap revision to key on: drop any stale entry rather than let a
        // later probe match against a revision this observation never had.
        IDLE_WATCH_QUEUE_HEAD_BY_REVISION.lock().remove(canonical);
        return;
    };
    IDLE_WATCH_QUEUE_HEAD_BY_REVISION
        .lock()
        .insert(canonical.to_path_buf(), (revision, observation.clone()));
}

fn show_pane_message(
    pane: &str,
    delay: &str,
    message: &str,
) -> Result<(), agent_doc_tmux_io::TmuxIoError> {
    let runner = agent_doc_tmux_io::ProcessTmuxRunner::default_binary();
    agent_doc_tmux_io::show_message(&runner, pane, delay, message)
}

/// `#fbwire` / `#fullboundary` Phase 2 — bounded timeout for the inter-queue-item
/// convergence gate. While the prior turn has not proven a quiescent close
/// (committed + editor buffer converged to HEAD + IPC inflight drained + actor
/// idle) the supervisor defers the next dispatch; once this much wall-clock has
/// elapsed at the gate (editor IPC wedged — exactly the `inflight=5` /
/// `send_failed` state) the boundary fails closed and records a loud playback
/// artifact. The idle-watch polls every 500ms (`AUTO_TRIGGER_POLL_INTERVAL`), so
/// this is ~60 ticks.
const CONVERGENCE_GATE_TIMEOUT_MS: u64 = 30_000;

/// `#idlewatchctrlbackoff`: once the project controller is observed degraded
/// (its CRDT-model read RPCs time out), the idle-queue watch reads the queue
/// head from disk for this long before probing the controller again. Long
/// enough to let a saturated controller recover, short enough to keep
/// queue-drain readiness prompt (~one probe per window per document).
const IDLE_WATCH_CONTROLLER_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
/// A quiescent session keeps the cheap installed-binary probe at 500ms, but
/// bounds pane inspection, full CRDT text reads, and reconciliation to this
/// interval. A stable blocked queue head is quiescent too: retrying the same
/// controller/model projection twice a second cannot make it dispatchable.
const IDLE_WATCH_QUIESCENT_MAINTENANCE_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5);
/// Even with an unchanged compact revision, periodically rerun the authoritative
/// full-text queue projection as a fail-safe against missed external signals.
const IDLE_WATCH_FULL_RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const IDLE_WATCH_ZOMBIE_REAP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

struct CapturedFinalizeResumeWorker {
    key: agent_doc_repair_command_io::CapturedFinalizeResumeKey,
    result: std::sync::mpsc::Receiver<agent_doc_repair_command_io::CapturedFinalizeResumeOutcome>,
}

struct CapturedFinalizeResumeRetry {
    key: agent_doc_repair_command_io::CapturedFinalizeResumeKey,
    attempts: u32,
    retry_at: std::time::Instant,
    needs_operator: bool,
    trigger_published: bool,
}

struct CapturedFinalizeResumeSignalWatch {
    result: std::sync::mpsc::Receiver<()>,
}

fn spawn_captured_finalize_resume_signal_watch(
    file: PathBuf,
    stop: Arc<AtomicBool>,
) -> std::io::Result<CapturedFinalizeResumeSignalWatch> {
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let (send, result) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("captured-finalize-signal".into())
        .spawn(move || {
            let mut cursor = None;
            while !stop.load(Ordering::Relaxed) {
                match agent_doc_controller_io::project_controller::subscribe_captured_finalize_wakes_for_file(
                    &file,
                    cursor,
                    std::time::Duration::from_secs(30),
                ) {
                    Ok(subscription) => {
                        cursor = Some(
                            agent_doc_controller_io::project_controller::ControllerStatePlaneCursor {
                                controller_generation: subscription.controller_generation,
                                plane_version: subscription.latest_version,
                            },
                        );
                        if subscription
                            .wakes
                            .iter()
                            .any(|wake| wake.document_hash == document_hash)
                            && send.send(()).is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => {
                        // Reconnect is an effect failure, not document-state
                        // convergence. Back off the transport without invoking
                        // finalize/session-check.
                        if !sleep_with_stop(&stop, std::time::Duration::from_secs(2)) {
                            return;
                        }
                    }
                }
            }
        })?;
    Ok(CapturedFinalizeResumeSignalWatch { result })
}

fn spawn_captured_finalize_resume_worker(
    file: PathBuf,
    key: agent_doc_repair_command_io::CapturedFinalizeResumeKey,
) -> std::io::Result<CapturedFinalizeResumeWorker> {
    let (send, result) = std::sync::mpsc::channel();
    let worker_key = key.clone();
    std::thread::Builder::new()
        .name("captured-finalize-resume".into())
        .spawn(move || {
            let outcome = agent_doc_repair_command_io::resume_captured_finalize(&file, &worker_key);
            let _ = send.send(outcome);
        })?;
    Ok(CapturedFinalizeResumeWorker { key, result })
}

fn supervisor_background_context_clear_enabled() -> bool {
    false
}

fn context_clear_projection_source_allows_supervisor_action(
    projection: &agent_doc_state_backbone::QueueContextClearProjection,
) -> bool {
    matches!(
        projection.source.as_deref(),
        Some(CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED | CONTEXT_CLEAR_SOURCE_QUEUE_SLASH)
    )
}

fn idle_watch_fast_path_can_sleep(
    queue_state_observed: bool,
    actor_ready: bool,
    urgent_maintenance: bool,
    maintenance_due: bool,
) -> bool {
    queue_state_observed && actor_ready && !urgent_maintenance && !maintenance_due
}

fn stale_recycle_safe_checkpoint(supervisor_stale: bool, inflight_handlers: u64) -> bool {
    supervisor_stale && inflight_handlers == 0
}

/// Lazy invalidation key for the supervisor's expensive queue-head projection.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IdleWatchDocumentRevision {
    Disk {
        len: u64,
        modified_nanos: u128,
        controller_observation_suppressed: bool,
    },
    Controller(agent_doc_crdt_relay_io::CurrentRevision),
}

fn idle_watch_disk_revision(
    file: &Path,
    controller_observation_suppressed: bool,
) -> Option<IdleWatchDocumentRevision> {
    let metadata = std::fs::metadata(file).ok()?;
    let modified_nanos = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(IdleWatchDocumentRevision::Disk {
        len: metadata.len(),
        modified_nanos,
        controller_observation_suppressed,
    })
}

/// Probe the document's revision, keeping the three outcomes distinct
/// (`#idlerevisionreactive`).
///
/// This used to return `Option<IdleWatchDocumentRevision>`, and every caller read
/// `None` as "changed" — so a controller that could not answer the *cheap* probe
/// was immediately asked several *expensive* ones, every 500ms instead of every
/// 60s. See [`agent_doc_supervisor::idle_revision`] for the full shape.
fn idle_watch_revision_observation(
    file: &Path,
    controller_observation_suppressed: bool,
) -> RevisionObservation {
    fn from_disk(file: &Path, suppressed: bool) -> RevisionObservation {
        match idle_watch_disk_revision(file, suppressed) {
            Some(revision) => RevisionObservation::observed(format!("{revision:?}")),
            // No readable metadata is a real unknown, not a change.
            None => RevisionObservation::Unresolved,
        }
    }

    let editor_attached =
        agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(file);
    if !editor_attached {
        return from_disk(file, controller_observation_suppressed);
    }
    if controller_observation_suppressed {
        // We deliberately did not ask. That is the cooldown working, so it must
        // not be reported as a change.
        return RevisionObservation::Suppressed;
    }

    match agent_doc_controller_io::project_controller::revision_via_controller_model_read_for_doc(
        file,
        "idle_watch_document_revision",
    ) {
        Ok(Some(agent_doc_crdt_relay_io::CurrentRevision::Detached)) => from_disk(file, false),
        Ok(Some(revision)) => RevisionObservation::observed(format!(
            "{:?}",
            IdleWatchDocumentRevision::Controller(revision)
        )),
        Ok(None) | Err(_) => RevisionObservation::Unresolved,
    }
}

/// `#fbwire` Phase 2 — is the session document's current visible text converged
/// to `HEAD`? This mirrors the `git::emit_postcommit_worktree_check`
/// `match=...` proof but reads through the realtime document boundary first, so
/// an unsaved editor buffer ahead of disk participates in the gate. A non-git
/// document or an unreadable `HEAD`/current document must NEVER wedge the drain,
/// so those degenerate cases report `true` (converged) and the gate falls
/// through to dispatch; the fail-closed blocked boundary is reserved for
/// genuine editor wedges, not missing git state.
fn editor_buffer_converged_to_head(file: &std::path::Path) -> bool {
    let head_doc = match agent_doc_git_io::revision::show_head(file) {
        Ok(Some(doc)) => doc,
        _ => return true,
    };
    let working = match agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "idle_watch_editor_converged_to_head",
    ) {
        Ok(doc) => doc,
        Err(_) => return true,
    };
    agent_doc_document::transient_markers::normalize_for_replay_hash(&head_doc)
        == agent_doc_document::transient_markers::normalize_for_replay_hash(&working)
}

/// `#fbwire` Phase 2 — gather the four [`agent_doc_document_realtime::convergence_gate::ConvergenceFacts`]
/// the inter-item dispatch boundary needs from live runtime state. `deferring_since`
/// is the instant the gate FIRST deferred for the current pending dispatch (reset to
/// `None` on every `Dispatch`), so `elapsed_ms` measures only the
/// current boundary's wait. A missing cycle-state file (first drain) or a load error
/// defaults `committed` to `true` so a brand-new drain is never blocked.
fn gather_convergence_facts(
    file: &std::path::Path,
    shared: &SupervisorShared,
    deferring_since: Option<std::time::Instant>,
    timeout_ms: u64,
) -> agent_doc_document_realtime::convergence_gate::ConvergenceFacts {
    let committed = match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(Some(state)) => matches!(state.phase, agent_doc_turn::CyclePhase::Committed),
        _ => true,
    };
    let elapsed_ms = deferring_since
        .map(|since| since.elapsed().as_millis() as u64)
        .unwrap_or(0);
    agent_doc_document_realtime::convergence_gate::ConvergenceFacts {
        committed,
        editor_converged: editor_buffer_converged_to_head(file),
        inflight: agent_doc_ipc_io::inflight_connection_handlers(),
        actor_idle: !actor_state_is_busy_or_starting(shared),
        elapsed_ms,
        timeout_ms,
    }
}

/// Outcome of the idle-watch document-transition check.
///
/// `Unresolved` is deliberately distinct from `Pending`: "the document is
/// mid-sync" and "we could not find out" are different facts, and only the first
/// one justifies skipping the drain indefinitely. See
/// [`agent_doc_supervisor::idle_reconcile::unresolved_transition_blocks_dispatch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdleQueueTransition {
    Converged,
    Pending,
    Unresolved,
}

impl IdleQueueTransition {
    fn from_converged(delivery_converged: bool) -> Self {
        if delivery_converged {
            Self::Converged
        } else {
            Self::Pending
        }
    }
}

/// `#recycletransitionwedge` — resolve whether a document transition is still in
/// flight, through the CONTROLLER, which is the process that actually owns the
/// CRDT hub.
///
/// This must not use the crdt-relay `current_text` file helpers. Those
/// resolve against `hub_registry()`, a process-local map that only the project
/// controller ever populates (`replica_register` is served by the controller RPC,
/// and the supervisor's IPC protocol explicitly rejects the replica methods). A
/// supervisor asking its own registry can only ever miss, and once durable
/// liveness reports the editor as attached that miss reads as
/// `EditorAttachedMissingReplica` — permanently, for the life of the process.
///
/// That is exactly the wedge this fixes: the local read pinned "pending", the
/// drain skipped every tick with `reason=current_transition_pending`, and because
/// the skip log is one-shot the session went silent with a queue head still owed.
/// Route it the same way `idle_watch_active_queue_head` and
/// `idle_watch_document_revision` already do, and report an unanswerable read as
/// `Unresolved` so the caller can bound it instead of blocking forever.
fn current_transition_for_idle_queue(file: &std::path::Path) -> IdleQueueTransition {
    // `#idlewatchtransitionrevision`: this needs exactly one boolean —
    // `delivery_converged` — and used to obtain it by asking the relay to
    // materialize the whole document, SHA-256 it, and write an `ops.log` line
    // carrying its length and hash, then discard the text (`..`). The compact
    // revision already carries `delivery_converged`, which is what
    // `CurrentRevision` exists for. Second-largest idle-watch read source in the
    // measured window (935 of ~6,300 full-text reads).
    //
    // The two `Current` shapes are deliberately identical here: `live_editors`
    // does not change the transition, only convergence does.
    match agent_doc_controller_io::project_controller::revision_via_controller_model_read_for_doc(
        file,
        "current_transition_for_idle_queue",
    ) {
        Ok(Some(agent_doc_crdt_relay_io::CurrentRevision::Detached)) => {
            IdleQueueTransition::Converged
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentRevision::Current {
            delivery_converged, ..
        })) => {
            if delivery_converged {
                IdleQueueTransition::Converged
            } else {
                IdleQueueTransition::Pending
            }
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentRevision::EditorAttachedMissingReplica))
        | Ok(None)
        | Err(_) => IdleQueueTransition::Unresolved,
    }
}

/// Fail-closed single-shot view for callers that have no tick budget to spend
/// (the captured-finalize resume diagnostic). An unresolved read counts as
/// pending here; that path retries on its own cadence and never gates the drain.
fn current_transition_pending_for_idle_queue(file: &std::path::Path) -> bool {
    !matches!(
        current_transition_for_idle_queue(file),
        IdleQueueTransition::Converged
    )
}

/// `#fbwire` / `#fullboundary` Phase 2 - the convergence gate could not be
/// satisfied within `CONVERGENCE_GATE_TIMEOUT_MS` (editor IPC wedged). Persist a
/// replayable [`agent_doc_workflow_io::convergence_playback::ConvergencePlayback`] artifact and
/// emit the ERROR-level `convergence_gate_blocked` ops-log line so a later agent
/// can root-cause the wedge from the logs alone. Best-effort: a failure to persist
/// the artifact is logged (never silently swallowed), but the boundary still fails
/// closed.
fn record_convergence_gate_blocked(
    file: &std::path::Path,
    facts: &agent_doc_document_realtime::convergence_gate::ConvergenceFacts,
    unmet: &[&'static str],
) {
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten();
    let cycle_id = state
        .as_ref()
        .map(|s| s.cycle_id.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let run_id = state.as_ref().and_then(|s| s.turn_id.clone());
    let playback = agent_doc_workflow_io::convergence_playback::ConvergencePlayback::new(
        file.display().to_string(),
        cycle_id,
        facts.inflight,
        facts.timeout_ms,
        unmet.iter().map(|s| (*s).to_string()).collect(),
    )
    .with_run_id(run_id)
    .with_state_transitions(vec![
        format!("committed={}", facts.committed),
        format!("editor_converged={}", facts.editor_converged),
        format!("inflight={}", facts.inflight),
        format!("actor_idle={}", facts.actor_idle),
        format!("elapsed_ms={}", facts.elapsed_ms),
        format!("timeout_ms={}", facts.timeout_ms),
    ]);
    match agent_doc_workflow_io::convergence_playback::record_blocked_boundary_with_logger(
        file,
        &playback,
        agent_doc_ops_log_io::log_op,
    ) {
        // `record_blocked_boundary_with_logger` already emits the canonical ERROR-level
        // `convergence_gate_blocked severity=error ... playback=<path>`
        // ops-log line referencing the persisted artifact, so a successful persist
        // needs no additional ops record here (avoid double-logging the wedge).
        Ok(_) => {}
        Err(err) => {
            eprintln!(
                "[agent-doc] warning: failed to persist convergence blocked-boundary playback for {}: {err}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{} severity=error file={} reason=editor_ipc_convergence_boundary_failed action=fail_closed unmet={} inflight={} elapsed_ms={} timeout_ms={} playback=unwritten playback_error={} (#fbwire)",
                    OpsLogEvent::ConvergenceGateBlocked,
                    file.display(),
                    unmet.join(","),
                    facts.inflight,
                    facts.elapsed_ms,
                    facts.timeout_ms,
                    err
                ),
            );
        }
    }
}

/// Surface the auto-install status on the owned tmux pane so the long blocking
/// rebuild does not read as a stall (`#supinstallfeedback`). Best-effort: no owned
/// pane (PTY-only) or a tmux failure falls back to stderr. The `Started` phase
/// persists (`-d 0`, until the next message/keypress) so it stays visible for the
/// whole compile; terminal phases use a bounded display time.
fn surface_supervisor_auto_install_status(
    shared: &SupervisorShared,
    phase: SupervisorAutoInstallPhase,
) {
    let message = supervisor_auto_install_pane_message(phase);
    let Some(pane) = shared.inject_pane.as_deref() else {
        eprintln!("{message}");
        return;
    };
    let delay = if matches!(phase, SupervisorAutoInstallPhase::Started) {
        "0"
    } else {
        "5000"
    };
    if let Err(err) = show_pane_message(pane, delay, message) {
        eprintln!(
            "[agent-doc] warning: failed to surface auto-install status on pane {pane}: {err}"
        );
    }
}

fn record_context_clear_prompt_for_hooks(
    shared: &SupervisorShared,
    path: &Path,
    harness: &agent_doc_harness::HarnessConfig,
    clear_cmd: &str,
) {
    if !matches!(harness.binary.as_str(), "codex" | "opencode") {
        return;
    }
    let Some(runtime) = shared.actor_runtime.as_ref() else {
        return;
    };
    if let Err(err) = agent_doc_codex_hook_io::record_external_prompt_for_file(
        path,
        &runtime.session_id,
        clear_cmd,
    ) {
        eprintln!(
            "[agent-doc] idle-queue watch: failed to record context clear prompt for {}: {err:#}",
            path.display()
        );
    }
}

/// Read the live `queue_active: true` ready head for the owned document, if any.
///
/// Thin wrapper over [`agent_doc_queue::queue_continuation::live_drainable_continuation_head`]
/// so the supervisor idle-watch dispatch agrees with `session-check`'s continuation
/// decision: it returns a head only when there is **agent-drainable** work at the
/// queue head, skipping inert artifact/log noise lines and
/// deferred `[clean-session]`/`[operator-verify]` heads. Otherwise the watch would
/// re-inject a no-op `/agent-doc` drain trigger every idle boundary for a queue
/// that has no continuation required (#qchurn / #goqueuestall / #goqstall2).
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueueHeadObservation {
    /// The drainable head plus the delivery-transition state observed by the SAME
    /// authority read. Keeping them together is deliberate: they must agree, and
    /// resolving them separately meant a second per-tick controller round-trip
    /// (`#idlewatchctrlbackoff`) or — worse — a process-local relay read that the
    /// supervisor can never satisfy (`#recycletransitionwedge`).
    Observed {
        head: Option<String>,
        transition: IdleQueueTransition,
    },
    AuthorityUnavailable,
}

fn disk_queue_authority_allowed(editor_attached: bool) -> bool {
    !editor_attached
}

fn idle_watch_replica_recovery_needed(
    current: &anyhow::Result<Option<agent_doc_crdt_relay_io::CurrentText>>,
) -> bool {
    matches!(
        current,
        Ok(Some(
            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
                | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
        ))
    )
}

fn idle_watch_active_queue_head(file: &Path) -> QueueHeadObservation {
    // `#idlewatchdetacheddisk`: when no live editor plugin owns the document,
    // disk is authoritative — a controller CRDT model read resolves back to disk
    // anyway (`live_editors == 0`), so the round-trip is pure overhead. Skip it
    // and read the on-disk queue head directly. Without this, an idle,
    // editorless supervisor polls the project controller every
    // `AUTO_TRIGGER_POLL_INTERVAL`; a slow/degraded controller then times out
    // each probe and pins the idle-watch in a permanent timeout→backoff cycle
    // even though the queue is fully drained (observed on a fully-committed
    // recruit session whose controller had grown too slow to answer the 750ms
    // model read). Only when a live editor is attached do we consult the
    // controller so an unsaved editor-buffer queue edit is still observed.
    let editor_attached =
        agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(file);
    if disk_queue_authority_allowed(editor_attached) {
        return idle_watch_disk_queue_head(file);
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    {
        let mut probes = ZERO_REPLICA_IDLE_WATCH_LAST_PROBE.lock();
        if probes
            .get(&canonical)
            .is_some_and(|last| last.elapsed() < ZERO_REPLICA_IDLE_WATCH_BACKOFF)
        {
            return QueueHeadObservation::AuthorityUnavailable;
        }
        probes.remove(&canonical);
    }
    // `#idlewatchrevisiongate`: honour the contract `CurrentRevision` already
    // documents — compare the compact revision before asking the relay to
    // materialize canonical markdown. A quiescent document (the common case at a
    // 500 ms poll) now costs one small state-vector comparison instead of a
    // full-document materialization, SHA-256, hub-lock hold, and `ops.log` write.
    //
    // Read first, on purpose: on a miss the revision observed *before* the text
    // is what gets stored, so a document that changes between the two reads
    // simply misses again next tick. Storing the revision observed after the
    // text could pair a fresh revision with stale text.
    let revision = match agent_doc_controller_io::project_controller::revision_via_controller_model_read_for_doc(
        file,
        "idle_watch_queue_head_revision_gate",
    ) {
        Ok(Some(revision @ agent_doc_crdt_relay_io::CurrentRevision::Current { .. })) => {
            if let Some(cached) = memoized_queue_head(&canonical, &revision) {
                return cached;
            }
            Some(revision)
        }
        // Detached / missing-replica / unavailable: fall through to the full read,
        // which owns those cases and their backoff bookkeeping. Failing open keeps
        // a degraded revision probe from changing the supervisor's decisions.
        _ => None,
    };
    let current =
        agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
            file,
            "idle_watch_active_queue_head",
        );
    if idle_watch_replica_recovery_needed(&current) {
        // A missing model is a repair trigger, not merely an unavailable read.
        // Request one targeted editor observation, then let the existing 30-second
        // backoff suppress further work. This repairs transparent controller
        // restarts without turning the idle watcher into a poller.
        let _ = agent_doc_crdt_relay_io::request_lazily_current_observation_with_timeout(
            file,
            "idle_watch_missing_replica_recovery",
            ZERO_REPLICA_IDLE_REPAIR_TIMEOUT,
        );
    }
    let (content, transition, queue_unresolved_prompts) = match current {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged,
            semantics,
            ..
        })) if live_editors > 0 => {
            ZERO_REPLICA_IDLE_WATCH_LAST_PROBE.lock().remove(&canonical);
            (
                text,
                IdleQueueTransition::from_converged(delivery_converged),
                semantics.map(|semantics| semantics.queue_unresolved_prompts),
            )
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            delivery_converged,
            semantics,
            ..
        })) => {
            ZERO_REPLICA_IDLE_WATCH_LAST_PROBE
                .lock()
                .insert(canonical.clone(), std::time::Instant::now());
            (
                text,
                IdleQueueTransition::from_converged(delivery_converged),
                semantics.map(|semantics| semantics.queue_unresolved_prompts),
            )
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) => {
            return idle_watch_disk_queue_head(file);
        }
        Ok(Some(
            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
            | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending,
        ))
        | Ok(None)
        | Err(_) => {
            ZERO_REPLICA_IDLE_WATCH_LAST_PROBE
                .lock()
                .insert(canonical.clone(), std::time::Instant::now());
            return QueueHeadObservation::AuthorityUnavailable;
        }
    };
    let observation = QueueHeadObservation::Observed {
        head: if queue_unresolved_prompts == Some(0) {
            None
        } else {
            agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                &content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            )
        },
        transition,
    };
    memoize_queue_head(&canonical, revision, &observation);
    observation
}

/// Resolve the drainable active-queue continuation head straight from the
/// on-disk document, bypassing the project controller. Shared by the
/// controller-paused / degraded-cooldown path and the no-live-editor fast path
/// (`#idlewatchdetacheddisk`); in every one of those states disk is the
/// authoritative replica for the supervisor's continuation decision.
fn idle_watch_disk_queue_head(file: &Path) -> QueueHeadObservation {
    let head = agent_doc_fs::read_optional_text(file)
        .ok()
        .flatten()
        .and_then(|content| {
            agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                &content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            )
        });
    // Disk IS the authority on this path, so there is no editor delivery in
    // flight to wait for — the transition is converged by construction.
    QueueHeadObservation::Observed {
        head,
        transition: IdleQueueTransition::Converged,
    }
}

fn idle_watch_paused_queue_head(file: &Path) -> QueueHeadObservation {
    if disk_queue_authority_allowed(
        agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(file),
    ) {
        idle_watch_disk_queue_head(file)
    } else {
        QueueHeadObservation::AuthorityUnavailable
    }
}

fn log_idle_queue_context_reset_submit(
    file: &Path,
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    clear_cmd: &str,
    active_head: &str,
    reason: &str,
) {
    let target = shared.inject_pane.as_deref().unwrap_or("child_pty");
    agent_doc_ops_log_io::log_op(
        file,
        &idle_queue_context_reset_ops_log_message(
            file,
            &harness.binary,
            clear_cmd,
            target,
            active_head,
            reason,
        ),
    );
}

fn forced_context_reset_reason_for_head(file: &Path, head: &str) -> Option<&'static str> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "idle_watch_forced_context_reset_reason",
    )
    .ok()?;
    if agent_doc_queue::queue_continuation::head_requires_focused_cycle_in(&content, head) {
        Some(FOCUSED_CYCLE_CONTEXT_RESET_REASON)
    } else if agent_doc_queue::queue_continuation::head_requires_clean_session_in(&content, head) {
        Some(CLEAN_SESSION_CONTEXT_RESET_REASON)
    } else {
        None
    }
}

fn record_context_clear_in_flight_projection(
    file: &Path,
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    clear_cmd: &str,
    source: &str,
    active_head: Option<&str>,
) {
    let target = shared
        .inject_pane
        .as_deref()
        .or_else(|| {
            shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.pane_id.as_str())
        })
        .unwrap_or("child_pty");
    if let Err(err) =
        agent_doc_controller_io::project_controller::queue_context_clear_started_for_file(
            file,
            target,
            &harness.binary,
            clear_cmd,
            source,
            active_head,
        )
    {
        eprintln!(
            "[agent-doc] idle-queue watch: failed to record context-clear projection for {}: {err:#}",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "idle_queue_watch_context_clear_projection_failed file={} harness={} error={:?}",
                file.display(),
                harness.binary,
                err.to_string()
            ),
        );
    }
}

fn log_between_turn_enqueue_delivery(file: &Path, clear_cmd: &str, drain_payload: &str) {
    let plan = between_turn_enqueue_plan([clear_cmd, drain_payload], clear_cmd, drain_payload);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "between_turn_enqueue deduped={} kept={} result=delivered",
            plan.deduped,
            plan.kept_labels()
        ),
    );
}

fn idle_queue_pending_payload_needs_enter_resubmit(
    harness_binary: &str,
    payload_already_pending: Option<bool>,
    already_resubmitted: bool,
) -> bool {
    agent_doc_tmux_commands::tmux_submit_profile_for_harness(harness_binary)
        .pending_draft_enter_resubmit()
        && drain_dispatch_dedup_skip(payload_already_pending)
        && !already_resubmitted
}

/// `#30p6`: policy for the only safe actions after one live pane sample.
///
/// A missing capture or a non-ready composer is not permission to write. A
/// matching draft is either submitted once or treated as already owned, while a
/// same-sample empty composer authorizes a fresh dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleQueuePendingPayloadAction {
    ResubmitEnter,
    SkipProvenPending,
    DispatchFresh,
    DeferUnobservable,
    DeferComposerOwned,
}

impl IdleQueuePendingPayloadAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::ResubmitEnter => "resubmit_enter",
            Self::SkipProvenPending => "skip_proven_pending",
            Self::DispatchFresh => "dispatch_fresh",
            Self::DeferUnobservable => "defer_unobservable",
            Self::DeferComposerOwned => "defer_composer_owned",
        }
    }
}

fn idle_queue_pending_payload_action(
    harness_binary: &str,
    payload_already_pending: Option<bool>,
    dispatch_ready: Option<bool>,
    already_resubmitted: bool,
) -> IdleQueuePendingPayloadAction {
    match payload_already_pending {
        Some(true)
            if idle_queue_pending_payload_needs_enter_resubmit(
                harness_binary,
                Some(true),
                already_resubmitted,
            ) =>
        {
            IdleQueuePendingPayloadAction::ResubmitEnter
        }
        Some(true) => IdleQueuePendingPayloadAction::SkipProvenPending,
        Some(false) if dispatch_ready == Some(true) => IdleQueuePendingPayloadAction::DispatchFresh,
        Some(false) => IdleQueuePendingPayloadAction::DeferComposerOwned,
        None => IdleQueuePendingPayloadAction::DeferUnobservable,
    }
}

fn record_idle_queue_payload_observation(
    file: &Path,
    harness: &agent_doc_harness::HarnessConfig,
    head: &str,
    payload: &str,
    observation: Option<&SupervisorPanePayloadObservation>,
    action: IdleQueuePendingPayloadAction,
) {
    let (pane, cursor_y, pending, dispatch_ready, capture_len, capture_hash, snapshot_path) =
        if let Some(observation) = observation {
            let outcome = agent_doc_controller_io::route_snapshot::preserve_route_pane_snapshot(
                file,
                &observation.pane_id,
                &harness.binary,
                "idle_queue_payload_observation",
                &observation.content,
                agent_doc_ops_log_io::log_op,
            );
            (
                observation.pane_id.as_str(),
                observation
                    .cursor_y
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                observation.payload_already_pending.to_string(),
                observation.dispatch_ready.to_string(),
                outcome.snapshot.len.to_string(),
                outcome.snapshot.hash,
                outcome
                    .snapshot
                    .path
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "none".to_string()),
            )
        } else {
            (
                "unknown",
                "unknown".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
                "none".to_string(),
            )
        };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "idle_queue_payload_observation file={} harness={} pane={} head_bytes={} head_sha256={} payload_bytes={} payload_sha256={} cursor_y={} payload_already_pending={} dispatch_ready={} action={} capture_len={} capture_hash={} snapshot_path={}",
            file.display(),
            harness.binary,
            pane,
            head.len(),
            agent_doc_hash::content_hash(head),
            payload.len(),
            agent_doc_hash::content_hash(payload),
            cursor_y,
            pending,
            dispatch_ready,
            action.as_str(),
            capture_len,
            capture_hash,
            snapshot_path,
        ),
    );
}

fn context_reset_dedupe_head<'a>(
    active_head: Option<&'a str>,
    last_context_reset_head: Option<&'a str>,
    context_reset_in_flight: bool,
) -> Option<&'a str> {
    if context_reset_in_flight {
        active_head
    } else {
        last_context_reset_head
    }
}

fn idle_queue_resubmit_pending_payload(
    file: &Path,
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    payload_kind: &str,
    active_head: &str,
    payload: &str,
) -> AutoTriggerOutcome {
    let Some(pane) = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))
    else {
        return AutoTriggerOutcome::SendFailed;
    };
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "supervisor.idle_queue_resubmit",
        &format!("pane:{pane}"),
        "",
        Some(&harness.binary),
        "idle_queue_pending_payload_submit_key",
        submit_key,
    );
    let tmux = tmux_router::Tmux::default_server();
    match agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        &tmux,
        &pane,
        "",
        &harness.binary,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    ) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "idle_queue_watch_resubmit file={} harness={} action=submit_key key={} result=sent target={} payload_kind={} head_bytes={} head_sha256={} payload_bytes={}",
                    file.display(),
                    harness.binary,
                    submit_key,
                    pane,
                    payload_kind,
                    active_head.len(),
                    agent_doc_hash::content_hash(active_head),
                    payload.len()
                ),
            );
            AutoTriggerOutcome::Sent
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "idle_queue_watch_resubmit file={} harness={} action=submit_key key={} result=send_failed target={} payload_kind={} head_bytes={} head_sha256={} error={:?}",
                    file.display(),
                    harness.binary,
                    submit_key,
                    pane,
                    payload_kind,
                    active_head.len(),
                    agent_doc_hash::content_hash(active_head),
                    err.to_string()
                ),
            );
            eprintln!(
                "[agent-doc] idle-queue watch: {} pending payload {} re-submit failed for pane {}: {err:#}",
                harness.binary, submit_key, pane
            );
            AutoTriggerOutcome::SendFailed
        }
    }
}

pub(super) fn spawn_idle_queue_watch_thread(
    shared: Arc<SupervisorShared>,
    stop: Arc<AtomicBool>,
    file: String,
    harness: agent_doc_harness::HarnessConfig,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("idle-queue-watch".into())
        .spawn(move || {
            let path = PathBuf::from(&file);
            // `#jbdisprecycle`: a freshly-started (post-recycle) supervisor
            // publishes the CP graph settle transition so route waiters reopen.
            if let Err(err) =
                agent_doc_controller_io::project_controller::supervisor_recycle_settled_for_file(
                    &path,
                    "watch_loop_started",
                )
            {
                eprintln!(
                    "[agent-doc] warning: failed to publish supervisor recycle settled: {err:#}"
                );
            }
            let mut last_dispatched: Option<String> = None;
            let mut last_context_reset_head: Option<String> = None;
            let mut last_context_clear_at: Option<u64> = None;
            let mut context_reset_in_flight = false;
            let mut last_pending_enter_resubmitted: Option<String> = None;
            let mut last_go_payload_observation_key: Option<String> = None;
            let mut clear_cooldown_logged = false;
            let mut route_submit_in_flight_logged = false;
            let mut context_clear_route_wait_logged = false;
            let mut current_transition_pending_logged = false;
            let mut context_reset_policy_error_logged = false;
            let mut idle_busy_ticks: u32 = 0;
            let mut ready_busy_ticks: u32 = 0;
            let mut ready_busy_logged_key: Option<(String, String)> = None;
            // `#clearcontresume`: consecutive idle-prompt polls observed while a
            // manual clear cooldown is active and a go-mode head is waiting.
            // Used to debounce the cooldown auto-expiry so a resumed drain never
            // injects into an in-flight `/clear`.
            let mut clear_cooldown_idle_ticks: u32 = 0;
            // `#qflood2`: after this watch sends its OWN `/clear` (an opt-in
            // context reset or a `/clear` queue head), block the next drain
            // trigger until the cleared pane settles to a fresh idle prompt for
            // `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls. Without this
            // the trigger was injected into the still-in-flight `/clear` and the
            // harness saw one concatenated line (`/clear /agent-doc <FILE>`).
            // Tracked in memory because the supervisor's own clears never record
            // the manual cooldown projection.
            let mut awaiting_clear_settle = false;
            let mut clear_settle_idle_ticks: u32 = 0;
            // `#fbwire` Phase 2: the instant the convergence gate FIRST deferred the
            // current pending inter-item dispatch. `None` ⇒ not currently deferring;
            // set on the first `Defer`, cleared on `Dispatch`.
            // Drives `elapsed_ms` so a wedged editor IPC reaches the bounded
            // `CONVERGENCE_GATE_TIMEOUT_MS` → loud fail-closed block (`#fullboundary`).
            let mut convergence_gate_deferring_since: Option<std::time::Instant> = None;
            let mut convergence_gate_blocked_reported = false;
            // R3 (#ctlrecycle): stale probes compare this supervisor process against
            // the installed binary via `SupervisorShared::refresh_binary_stale`.
            // That covers both a later `cargo install` and a process already
            // mapping an old executable at start.
            let recycle_auto_enabled =
                agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(&path);
            let recycle_grace = agent_doc_controller_io::project_controller::recycle_idle_grace();
            let mut recycle_stale_since: Option<std::time::Instant> = None;
            let mut recycle_detected_logged = false;
            // `#suprecyclestall` — set once a self-`execve` recycle has failed so we
            // do not re-attempt a hopeless hot-reload (which orphans the child / hangs
            // the pane) on every later idle boundary. The supervisor keeps running on
            // its current binary until the operator restarts it.
            let mut reexec_recycle_disabled = false;
            // `#supautoinstall`: dogfood auto-install rung that PRECEDES the recycle rung.
            // When this supervisor hosts an agent-doc session editing agent-doc's OWN
            // source and a finalize committed an edit, build+install at the idle boundary
            // so the installed binary catches up — then the recycle block below hot-reloads
            // onto it. Resolved ONCE: a disabled path does zero extra work (no source
            // walk, no crate-root probe), and only an agent-doc dogfood document may
            // resolve a crate root.
            // `#agentreloadrestart`: capture the harness this supervisor launched
            // with + the agent-change-restart knob. A later frontmatter `agent:`
            // change (e.g. claude->opencode) makes the freshly-resolved harness
            // differ; at a quiet dispatch-ready boundary the watch logs the
            // detected change + the gate decision (`harness_change_detected` /
            // `agent_restart_boundary_gate`) so the operator can prove detection
            // live. Executing the restart (Phase 1b) is the focused-cycle wiring.
            let agent_change_restart_enabled =
                agent_doc_supervisor_io::config::agent_change_restart_enabled(&path);
            let launch_harness_binary = harness.binary.clone();
            let mut agent_change_logged_for: Option<String> = None;
            // `#agentreloadrestart` Phase 1b — dedup the actual restart TRIGGER
            // separately from the per-harness detection log: a change can be
            // detected mid-turn (`WaitForBoundary`, logged once) and only later
            // reach a quiet boundary (`Restart`), so the trigger must not be
            // suppressed by the logging dedup.
            let mut agent_change_restart_requested_for: Option<String> = None;
            let auto_install_enabled =
                agent_doc_supervisor_io::config::supervisor_auto_install_enabled(&path);
            let install_crate_root = if auto_install_enabled {
                agent_doc_controller_io::project_controller::dogfood_agent_doc_crate_root(&path)
            } else {
                None
            };
            let mut install_stale_since: Option<std::time::Instant> = None;
            let mut install_detected_logged = false;
            let mut install_dirty_logged = false;
            // `#supautoinstall` — one-shot latch: a failed build must not be re-attempted
            // every idle boundary (it would block the watch on a hopeless multi-minute
            // build each tick). After one failure the supervisor logs once and leaves the
            // refresh to the operator, exactly like `reexec_recycle_disabled`.
            let mut auto_install_disabled = false;
            // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): set true once an
            // in-place `execve` recycle has failed, so the recycle policy escalates to
            // a bounded kill+relaunch (`EscalateKillRelaunch`) instead of re-logging
            // `continue_current_binary` on every later idle boundary. The escalation
            // counter caps the kill+relaunches (`MAX_REEXEC_ESCALATIONS`) so a relaunch
            // that never clears the staleness cannot spin into an unbounded kill loop.
            let mut reexec_failed = false;
            let mut reexec_escalation_attempts: u32 = 0;
            let mut reexec_escalation_exhausted_logged = false;
            // `#wd40` / `#staleloop-recycle-restart`: one-shot log latch for the
            // stale-binary recycle-yield projection. A continuously self-draining
            // `/loop` holds the harness `turn_active` back-to-back so this
            // supervisor never reaches its own recycle boundary; when it is stale
            // AND that loop owns the drain we publish a controller recycle-yield
            // projection that the in-session loop reads at its next inter-item
            // boundary and yields, letting the `execve` recycle fire on its own.
            // The projection is refreshed every tick while the condition holds;
            // the log line fires once so the watch loop stays quiet.
            let mut recycle_yield_requested_logged = false;
            // `#midturn-recycle-resume` Phase B: consecutive idle-watch boundary ticks
            // the recycle has been deferred for an open agent-doc cycle. The landed
            // `#suprecyclespin` stalled-cycle-resolve (below) clears the gate for an
            // abandoned-older superseded cycle, but a cycle that never closes for some
            // OTHER reason (intermittent IPC inflight, a wedged finalize that keeps
            // ticking `updated_at`) would still starve the recycle. Once this streak
            // reaches `MAX_CYCLE_OPEN_DEFER_TICKS` the watch ESCALATES — it forces the
            // recycle DECISION (`effective_cycle_open=false`) as a backstop layered on
            // top of the stalled-resolve. The forced `execve` severs the wedged cycle,
            // but its open `#durablerecycle` checkpoint survives so the fresh boot
            // re-dispatches the genuinely-interrupted turn (see `boot_resume_action`).
            let mut cycle_open_defer_streak: u32 = 0;
            let mut cycle_open_defer_escalated_logged = false;
            // `#suprecyclespin-falseabandon`: consecutive polls for which the
            // transactional-cycle staleness predicate has held at a `turn_boundary`. The
            // force-abandon only fires once this reaches
            // `STALLED_CYCLE_RESOLVE_CONFIRM_TICKS`, so a transiently-misread
            // boundary during a live harness generation can never abandon the
            // turn from a merely-stale projection (a stale projection must not
            // interfere with the live turn — Lazily stays authoritative).
            let mut stalled_resolve_streak: u32 = 0;
            // `#idlewatchctrlbackoff`: when the controller is degraded (its RPCs
            // are timing out), the idle-watch would otherwise hammer it with a
            // CRDT-model read every poll — paying the full read timeout each
            // time AND saturating the controller further (observed live: three
            // supervisors × 2 reads/s pinned the controller at ~82% CPU and
            // produced multi-hour controller-lookup timeout storms). Instead,
            // once a controller failure is observed we pause queue observation
            // for a cooldown, then probe the controller once per cooldown window.
            // We never substitute disk while an editor is attached because that
            // would resurrect operator-deleted unsaved queue items.
            let mut controller_degraded_until: Option<std::time::Instant> = None;
            let mut controller_backoff_logged = false;
            let mut queue_state_observed = false;
            let mut last_quiescent_maintenance: Option<std::time::Instant> = None;
            // `#idlerevisionreactive`: the revision baseline, staleness, and
            // controller probe health used to be loop-local `mut` bindings that
            // whichever branch remembered to update. They are now one state
            // machine cell and two `Computed`s in a process-lifetime scope, so a
            // derived fact cannot be left behind by a branch that forgot it.
            let revision_state = IdleRevisionState::new();
            // The ONE thing here that is genuinely an effect: writing a
            // diagnostic. Gated on derived health, so it fires on the transition
            // and never per tick.
            //
            // The backoff deliberately is NOT an effect. An effect whose whole job
            // is to set a variable is a `Computed` in disguise, and stamping a
            // deadline from a clock reading puts the answer somewhere the graph
            // cannot derive or invalidate. `should_probe_controller` counts
            // skipped observations instead, so the backoff is a pure function of
            // the observation stream.
            let health_log_path = path.clone();
            let _probe_health_effect = revision_state.on_probe_health_change(move |health| {
                if let ControllerProbeHealth::Degraded { unresolved_streak } = health {
                    agent_doc_ops_log_io::log_op(
                        &health_log_path,
                        &format!(
                            "idle_watch_controller_probe_degraded file={} unresolved_streak={} retry_after_observations={} action=hold_projection_and_back_off",
                            health_log_path.display(),
                            unresolved_streak,
                            agent_doc_supervisor::idle_revision::SUPPRESSED_OBSERVATIONS_BEFORE_RETRY,
                        ),
                    );
                }
            });
            let mut last_full_reconcile: Option<std::time::Instant> = None;
            let mut last_zombie_reap: Option<std::time::Instant> = None;
        // `#binaryownedfinalize`: once finalize has durably captured a
        // response, this supervisor owns event-driven resumption of that exact
        // operation. A dedicated worker keeps repair/commit latency off the
        // queue-watch thread; `resume_worker` is the local keyed-single-flight
        // latch. Only a failed effect receives a timed retry edge.
    let mut resume_worker: Option<CapturedFinalizeResumeWorker> = None;
    let mut resume_retry: Option<CapturedFinalizeResumeRetry> = None;
    let resume_triggers = CapturedFinalizeResumeTriggers::new();
    let resume_signal_watch =
        spawn_captured_finalize_resume_signal_watch(path.clone(), Arc::clone(&stop)).ok();
    // Cold-start inspection is the covering snapshot for a capture that
    // predates this supervisor. Afterwards, only controller state edges or an
    // effect-retry receipt re-arm inspection.
    let mut resume_key_refresh_pending = true;
    let mut last_resume_key_error_hash: Option<String> = None;
    loop {
                if !sleep_with_stop(&stop, AUTO_TRIGGER_POLL_INTERVAL) {
                    return;
                }
                // `#adturnscopehotloop`: one turn-attribution memo per tick.
                //
                // Every `log_op` resolves a `turn=` id, and outside a scope that
                // resolution is `load_document_projection` — which, from this
                // process, is a **controller IPC round trip with a 5s timeout**.
                // A poll loop that logs several lines per tick was therefore
                // issuing several controller round trips per tick purely to
                // decorate its own log lines, feeding the saturation that then
                // made those round trips time out. Diagnostics must not perturb
                // the system they measure.
                //
                // A tick cannot span two turns, so the scope is exact rather
                // than a TTL guess, and it drops at the end of each iteration.
                let _turn_attribution = agent_doc_ops_log_io::begin_turn_attribution_scope();
                // `#idlequiet`: stale-binary convergence is the one check that
                // must remain prompt at every stage of a turn. Keep it ahead of
                // every quiescent fast-path gate; it is a local inode/stat probe
                // and does not touch the controller or editor.
        let supervisor_stale_fast = shared.refresh_binary_stale();
        let now = std::time::Instant::now();
        if resume_signal_watch.as_ref().is_some_and(|watch| {
            let mut observed = false;
            while watch.result.try_recv().is_ok() {
                observed = true;
            }
            observed
        }) {
            resume_triggers.observe_state_edge();
            resume_key_refresh_pending = true;
            last_quiescent_maintenance = None;
        }
        let finished_resume = resume_worker.as_ref().and_then(|worker| {
                    match worker.result.try_recv() {
                        Ok(outcome) => Some((worker.key.clone(), outcome)),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((
                            worker.key.clone(),
                    agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::RetryableEffect {
                        reason: "captured finalize resume worker disconnected".to_string(),
                    },
                        )),
                    }
                });
                if let Some((key, outcome)) = finished_resume {
                    resume_worker = None;
                    last_quiescent_maintenance = None;
                    match outcome {
                        agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Committed {
                            repair_outcome,
                } => {
                    resume_retry = None;
                    resume_triggers.observe_operation(None);
                    let event = format!(
                                "captured_finalize_resume_committed file={} cycle_id={} capture_id={} response_sha256={} repair_outcome={} authority=editor_crdt",
                                path.display(),
                                key.cycle_id,
                                key.capture_id,
                                key.response_sha256,
                                repair_outcome,
                            );
                            log_event(&mut session_log, &event);
                            agent_doc_ops_log_io::log_op(&path, &event);
                        }
                agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded => {
                    resume_retry = None;
                    resume_triggers.observe_operation(None);
                            let event = format!(
                                "captured_finalize_resume_superseded file={} cycle_id={} capture_id={} response_sha256={}",
                                path.display(),
                                key.cycle_id,
                                key.capture_id,
                                key.response_sha256,
                            );
                            log_event(&mut session_log, &event);
                            agent_doc_ops_log_io::log_op(&path, &event);
                        }
                agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::WaitingForSignal {
                    reason,
                } => {
                    resume_retry = None;
                    let event = format!(
                        "captured_finalize_resume_waiting_for_state file={} cycle_id={} capture_id={} response_sha256={} reason_bytes={} reason_sha256={} action=await_controller_state_edge",
                        path.display(),
                        key.cycle_id,
                        key.capture_id,
                        key.response_sha256,
                        reason.len(),
                        agent_doc_hash::content_hash(&reason),
                    );
                    log_event(&mut session_log, &event);
                    agent_doc_ops_log_io::log_op(&path, &event);
                }
                agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::RetryableEffect {
                    reason,
                } => {
                            let attempts = resume_retry
                                .as_ref()
                                .filter(|retry| retry.key == key)
                                .map_or(1, |retry| retry.attempts.saturating_add(1));
                            let delay = captured_finalize_resume_retry_delay(attempts);
                            resume_retry = Some(CapturedFinalizeResumeRetry {
                                key: key.clone(),
                                attempts,
                        retry_at: now + delay,
                        needs_operator: false,
                        trigger_published: false,
                            });
                            let event = format!(
                                "captured_finalize_resume_retry_scheduled file={} cycle_id={} capture_id={} response_sha256={} attempt={} delay_ms={} reason_bytes={} reason_sha256={} authority=editor_crdt no_force_disk=true",
                                path.display(),
                                key.cycle_id,
                                key.capture_id,
                                key.response_sha256,
                                attempts,
                                delay.as_millis(),
                                reason.len(),
                                agent_doc_hash::content_hash(&reason),
                            );
                            log_event(&mut session_log, &event);
                            agent_doc_ops_log_io::log_op(&path, &event);
                        }
                        agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::NeedsOperator {
                            reason,
                        } => {
                    resume_retry = Some(CapturedFinalizeResumeRetry {
                        key: key.clone(),
                        attempts: 1,
                        retry_at: now,
                        needs_operator: true,
                        trigger_published: true,
                    });
                    resume_triggers.require_operator();
                            let event = format!(
                                "captured_finalize_resume_needs_operator file={} cycle_id={} capture_id={} response_sha256={} reason_bytes={} reason_sha256={} action=retain_without_mutation",
                                path.display(),
                                key.cycle_id,
                                key.capture_id,
                                key.response_sha256,
                                reason.len(),
                                agent_doc_hash::content_hash(&reason),
                            );
                            log_event(&mut session_log, &event);
                            agent_doc_ops_log_io::log_op(&path, &event);
                            eprintln!(
                                "[agent-doc] captured finalize for {} needs operator resolution; response retained without mutation",
                                path.display()
                            );
                        }
                    }
                }
                let zombie_reap_due = last_zombie_reap
                    .is_none_or(|last| last.elapsed() >= IDLE_WATCH_ZOMBIE_REAP_INTERVAL);
                if zombie_reap_due {
                    let reaped = agent_doc_supervisor_process::detached_child::reap_historical_agent_doc_zombies();
                    last_zombie_reap = Some(now);
                    if reaped > 0 {
                        log_event(
                            &mut session_log,
                            &format!("idle_queue_watch_reaped_historical_controller_zombies count={reaped}"),
                        );
                    }
                }
                let actor_ready_fast = actor_state_is_ready(&shared);
                let urgent_maintenance = supervisor_stale_fast
                    || context_reset_in_flight
                    || awaiting_clear_settle
                    || shared.restart_reexec.load(Ordering::Relaxed);
        let maintenance_due = last_quiescent_maintenance.is_none_or(|last| {
            last.elapsed() >= IDLE_WATCH_QUIESCENT_MAINTENANCE_INTERVAL
        });
        if let Some(retry) = resume_retry.as_mut()
            && !retry.needs_operator
            && !retry.trigger_published
            && now >= retry.retry_at
        {
            retry.trigger_published = true;
            resume_triggers.observe_effect_retry_due();
            resume_key_refresh_pending = true;
            last_quiescent_maintenance = None;
        }
        if maintenance_due && (resume_key_refresh_pending || resume_triggers.ready()) {
            match agent_doc_repair_command_io::captured_finalize_resume_key(&path) {
                Ok(Some(key)) => {
                    resume_key_refresh_pending = false;
                    last_resume_key_error_hash = None;
                    resume_triggers.observe_operation(Some(format!(
                        "{}:{}:{}",
                        key.cycle_id, key.capture_id, key.response_sha256
                    )));
                    if resume_retry
                        .as_ref()
                        .is_some_and(|retry| retry.key != key)
                            {
                                resume_retry = None;
                            }
                            let needs_operator = resume_retry
                                .as_ref()
                                .filter(|retry| retry.key == key)
                                .is_some_and(|retry| retry.needs_operator);
                            let retry_cooldown_elapsed = resume_retry
                                .as_ref()
                                .filter(|retry| retry.key == key)
                                .is_none_or(|retry| now >= retry.retry_at);
                let facts = CapturedFinalizeResumeFacts {
                    captured_operation_present: true,
                    // Diagnostic only: a captured finalize owns its closeout
                    // lease and must recover even while the Stop hook keeps the
                    // harness actor active.
                    actor_ready: actor_ready_fast,
                                current_transition_pending: current_transition_pending_for_idle_queue(&path),
                                ipc_inflight: agent_doc_ipc_io::inflight_connection_handlers(),
                                worker_in_flight: resume_worker.is_some(),
                                retry_cooldown_elapsed: retry_cooldown_elapsed
                                    && !needs_operator,
                                controller_pressure_cooldown: agent_doc_controller_io::project_controller::controller_model_pressure_cooldown_active_for_doc(&path),
                        urgent_supervisor_maintenance: urgent_maintenance,
                    };
                    if resume_triggers.ready() && captured_finalize_resume_should_start(facts) {
                        match spawn_captured_finalize_resume_worker(
                            path.clone(),
                            key.clone(),
                                ) {
                                    Ok(worker) => {
                                        let attempt = resume_retry
                                            .as_ref()
                                            .filter(|retry| retry.key == key)
                                            .map_or(1, |retry| retry.attempts.saturating_add(1));
                                        let event = format!(
                                            "captured_finalize_resume_started file={} cycle_id={} capture_id={} response_sha256={} attempt={} authority=editor_crdt no_force_disk=true",
                                            path.display(),
                                            key.cycle_id,
                                            key.capture_id,
                                            key.response_sha256,
                                            attempt,
                                        );
                                log_event(&mut session_log, &event);
                                agent_doc_ops_log_io::log_op(&path, &event);
                                resume_triggers.consume_attempt();
                                resume_worker = Some(worker);
                            }
                                    Err(err) => {
                                        let attempts = resume_retry
                                            .as_ref()
                                            .filter(|retry| retry.key == key)
                                            .map_or(1, |retry| retry.attempts.saturating_add(1));
                                        resume_retry = Some(CapturedFinalizeResumeRetry {
                                            key,
                                    attempts,
                                    retry_at: now
                                        + captured_finalize_resume_retry_delay(attempts),
                                    needs_operator: false,
                                    trigger_published: false,
                                });
                                        eprintln!(
                                            "[agent-doc] warning: failed to spawn captured finalize resume worker: {err}"
                                        );
                                    }
                                }
                            }
                }
                Ok(None) => {
                    resume_key_refresh_pending = false;
                    last_resume_key_error_hash = None;
                    if resume_worker.is_none() {
                        resume_retry = None;
                        resume_triggers.observe_operation(None);
                    }
                }
                Err(err) => {
                    resume_key_refresh_pending = false;
                    let reason = format!("{err:#}");
                            let reason_hash = agent_doc_hash::content_hash(&reason);
                            if last_resume_key_error_hash.as_deref() != Some(&reason_hash) {
                                last_resume_key_error_hash = Some(reason_hash.clone());
                                let event = format!(
                                    "captured_finalize_resume_key_error file={} reason_bytes={} reason_sha256={} action=retain_without_mutation",
                                    path.display(),
                                    reason.len(),
                                    reason_hash,
                                );
                                log_event(&mut session_log, &event);
                                agent_doc_ops_log_io::log_op(&path, &event);
                            }
                        }
                    }
                }
                if idle_watch_fast_path_can_sleep(
                    queue_state_observed,
                    actor_ready_fast,
                    urgent_maintenance,
                    maintenance_due,
                ) {
                    continue;
                }
                if !actor_ready_fast {
                    last_quiescent_maintenance = None;
                }
                if actor_ready_fast && !urgent_maintenance && maintenance_due {
                    let queue_controller_paused = agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(
                        &path,
                    )
                    .is_some();
                    let controller_in_cooldown = controller_degraded_until
                        .is_some_and(|until| now < until)
                        || agent_doc_controller_io::project_controller::controller_model_pressure_cooldown_active_for_doc(&path);
                    // `#idlerevisionreactive`: feed the observation in and read
                    // the derived answer back. Staleness and controller health are
                    // `Computed`s over this one write, so neither can drift out of
                    // step with the observation that produced it.
                    // `#idlerevisionreactive`: the derived backoff joins the
                    // caller's own suppression reasons. The cheap probe failing is
                    // already evidence the controller is struggling, so it backs
                    // off here rather than waiting for an expensive probe to fail
                    // too — and because the answer is derived from the observation
                    // stream, obeying it feeds the same stream that will clear it.
                    let suppress_controller_observation = queue_controller_paused
                        || controller_in_cooldown
                        || !revision_state.should_probe_controller();
                    revision_state.observe(idle_watch_revision_observation(
                        &path,
                        suppress_controller_observation,
                    ));
                    let revision_changed = revision_state.projection_stale();
                    let full_reconcile_due = last_full_reconcile.is_none_or(|last| {
                        last.elapsed() >= IDLE_WATCH_FULL_RECONCILE_INTERVAL
                    });
                    if queue_state_observed && !revision_changed && !full_reconcile_due {
                        last_quiescent_maintenance = Some(now);
                        continue;
                    }
                    last_full_reconcile = Some(now);
                }
                // `#capproofbg`: a *pending* managed-capability proof no longer
                // stalls the idle-queue dispatch. Drain dispatch proceeds
                // immediately while the proof runs in the background; only a proven
                // FAILURE gates dispatch (via `capability_dispatch_blocker` in the
                // shared inject path), and that failure is surfaced asynchronously
                // through the session log + tmux `display-message`.
                let clear_cooldown_active = clear_cooldown_blocks_auto_dispatch(
                    &path,
                    &harness,
                    "idle_queue_watch",
                    &mut session_log,
                    &mut clear_cooldown_logged,
                );

                // `#stale-busy-after-auto-inject-no-clear`: poll-based self-heal
                // for a stale busy actor wedged over an idle pane. The
                // edge-triggered pty completion transition can miss an
                // injected turn's composer redraw after it returns, leaving the
                // actor `busy` with no further output to retrigger ready.
                // Re-derive ready from direct pane evidence so the session
                // never gets "truly stuck" needing a pane kill or
                // `session clear`.
                let actor_busy = actor_state_is_busy_or_starting(&shared);
                let pane_busy_cue = if actor_busy && !clear_cooldown_active {
                    supervisor_pane_has_busy_cue(&shared, &harness)
                } else {
                    None
                };
                let turn_active = actor_busy
                    && turn_active_for_owned_pane_with_idle_evidence(
                        &path,
                        &shared,
                        false,
                        &mut session_log,
                    );
                let dispatch_grace_active = actor_busy
                    && shared.prompt_dispatch_grace_active(std::time::Duration::from_secs(15));
                match pane_busy_cue {
                    Some(false) if !turn_active && !dispatch_grace_active => {
                        idle_busy_ticks = idle_busy_ticks.saturating_add(1)
                    }
                    _ => idle_busy_ticks = 0,
                }
                if stale_busy_idle_reconcile_decision(
                    actor_busy,
                    pane_busy_cue == Some(true),
                    turn_active,
                    dispatch_grace_active,
                    clear_cooldown_active,
                    idle_busy_ticks,
                    STALE_BUSY_RECONCILE_TICKS,
                ) {
                    shared.transition_actor_state(
                        agent_doc_controller::actor::ActorState::Ready,
                        "supervisor",
                        "idle_pane_reconcile",
                    );
                    // Reset the one-shot prompt latch so a later genuine
                    // busy→ready edge still fires normally. Preserve the
                    // dispatch dedup: if the injected command returned without
                    // consuming the same active head, re-firing it every
                    // stale-busy reconcile tick loops the owner pane.
                    shared.prompt_visible_once.store(false, Ordering::Relaxed);
                    last_dispatched = reconcile_stale_busy_idle_queue_state(
                        last_dispatched,
                        &mut idle_busy_ticks,
                    );
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_stale_busy_reconciled harness={} pane={} after_ticks={}",
                            harness.binary,
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            STALE_BUSY_RECONCILE_TICKS
                        ),
                    );
                    eprintln!(
                        "[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)"
                    );
                }

                let queue_pause_reason =
                    agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(
                        &path,
                    );
                let queue_controller_paused = queue_pause_reason.is_some();
                // A controller pause is the unattended flood guard. During a
                // pause/cooldown, disk may be consulted only when the editor is
                // detached. An attached editor with unavailable Lazily authority
                // yields `AuthorityUnavailable` and the drain simply waits.
                let now = std::time::Instant::now();
                let shared_controller_cooldown = agent_doc_controller_io::project_controller::controller_model_pressure_cooldown_active_for_doc(&path);
                let controller_in_cooldown = controller_degraded_until
                    .is_some_and(|until| now < until)
                    || shared_controller_cooldown;
                if shared_controller_cooldown {
                    controller_degraded_until = Some(now + IDLE_WATCH_CONTROLLER_BACKOFF);
                }
                let active_head_observation = if queue_controller_paused || controller_in_cooldown {
                    idle_watch_paused_queue_head(&path)
                } else {
                    let observation = idle_watch_active_queue_head(&path);
                    if agent_doc_document_realtime_io::controller_failed_within(
                        std::time::Duration::from_secs(2),
                    ) {
                        controller_degraded_until =
                            Some(now + IDLE_WATCH_CONTROLLER_BACKOFF);
                        if !controller_backoff_logged {
                            controller_backoff_logged = true;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_controller_backoff pane={} cooldown_secs={} reason=controller_degraded",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    IDLE_WATCH_CONTROLLER_BACKOFF.as_secs(),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] idle-queue watch: backing off degraded project controller for {}s (attached editor queue authority paused)",
                                IDLE_WATCH_CONTROLLER_BACKOFF.as_secs()
                            );
                        }
                    } else {
                        // Controller responded cleanly — clear the one-shot
                        // latch so a *later* degradation logs again.
                        controller_backoff_logged = false;
                    }
                    observation
                };
                // The head and its delivery-transition state come out of ONE
                // authority read (`#recycletransitionwedge`).
                let (active_head, active_transition) = match active_head_observation {
                    QueueHeadObservation::Observed { head, transition } => (head, transition),
                    QueueHeadObservation::AuthorityUnavailable => {
                        queue_state_observed = false;
                        last_quiescent_maintenance = None;
                        continue;
                    }
                };
                if active_head.is_none() {
                    context_reset_in_flight = false;
                    last_context_reset_head = None;
                }
                queue_state_observed = true;
                if actor_ready_fast && !urgent_maintenance {
                    last_quiescent_maintenance = Some(now);
                }
                let actor_ready = actor_ready_fast;
                let ready_busy_reason = if active_head.is_some()
                    && actor_ready
                    && !clear_cooldown_active
                {
                    ready_busy_blocker_reason(&shared, &harness)
                } else {
                    None
                };
                if ready_busy_reason.is_some() {
                    ready_busy_ticks = ready_busy_ticks.saturating_add(1);
                } else {
                    ready_busy_ticks = 0;
                    ready_busy_logged_key = None;
                }
                let ready_busy_reconciled = ready_busy_conflict_reconcile_decision(
                    actor_ready,
                    ready_busy_reason.as_deref(),
                    clear_cooldown_active,
                    ready_busy_ticks,
                    STALE_BUSY_RECONCILE_TICKS,
                );
                if ready_busy_reconciled {
                    let reason = ready_busy_reason.as_deref().unwrap_or("unknown");
                    let head = active_head.as_deref().unwrap_or("unknown");
                    let key = (head.to_string(), reason.to_string());
                    if ready_busy_logged_key.as_ref() != Some(&key) {
                        let event = format!(
                            "owned_pane_ready_busy_conflict source=idle_queue_watch harness={} pane={} reason={:?} after_ticks={} head={:?}",
                            harness.binary,
                            owned_pane_label(&shared),
                            reason,
                            STALE_BUSY_RECONCILE_TICKS,
                            head
                        );
                        log_event(&mut session_log, &event);
                        agent_doc_ops_log_io::log_op(&path, &event);
                        ready_busy_logged_key = Some(key);
                    }
                }
                let prompt_visible =
                    ready_busy_reconciled || idle_queue_prompt_visible(&shared, &harness);
                let turn_active = turn_active_for_owned_pane_with_idle_evidence(
                    &path,
                    &shared,
                    prompt_visible,
                    &mut session_log,
                );

                // `#agentreloadrestart` Phase 1a: detect a frontmatter `agent:`
                // change and log the boundary-gate decision. Re-resolve the
                // harness from CURRENT frontmatter and compare to the one this
                // supervisor launched with; on a change (deduped per new harness),
                // emit `harness_change_detected` + `agent_restart_boundary_gate`.
                // The restart execution itself (spawn the new harness fresh) is the
                // focused-cycle Phase 1b wiring; until then a detected change is
                // surfaced so `agent:` edits are observable + operator-verifiable.
                if agent_change_restart_enabled
                    && let Ok(content) = std::fs::read_to_string(&path)
                    && let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(&content)
                {
                    let global = agent_doc_config::load().unwrap_or_default();
                    let resolved = agent_doc_harness::HarnessConfig::from_context(&fm, &global);
                    let harness_changed = resolved.binary != launch_harness_binary;
                    if harness_changed {
                        let decision = agent_change_restart_decision(
                            harness_changed,
                            agent_change_restart_enabled,
                            prompt_visible,
                            turn_active,
                        );
                        let already_logged =
                            agent_change_logged_for.as_deref() == Some(resolved.binary.as_str());
                        if !already_logged {
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "harness_change_detected file={} old={} new={} gate={:?}",
                                    path.display(),
                                    launch_harness_binary,
                                    resolved.binary,
                                    decision,
                                ),
                            );
                            log_event(
                                &mut session_log,
                                &format!(
                                    "agent_restart_boundary_gate old={} new={} decision={:?} prompt_visible={} turn_active={} note=phase1b_restart_triggered",
                                    launch_harness_binary,
                                    resolved.binary,
                                    decision,
                                    prompt_visible,
                                    turn_active,
                                ),
                            );
                            // Dedupe logging: only re-log when the resolved harness
                            // changes again (avoid per-tick spam for a standing change).
                            agent_change_logged_for = Some(resolved.binary.clone());
                        }

                        // `#agentreloadrestart` Phase 1b execution: at a quiet
                        // dispatch-ready boundary the policy returns `Restart` — fire
                        // the SAME restart signal the IPC restart uses. The supervisor
                        // restart loop (run.rs) then re-reads current frontmatter,
                        // re-resolves the harness spec, sees the changed binary, and
                        // spawns the new harness FRESH (`agent_restart_performed`).
                        // Never interrupt an in-flight turn: `WaitForBoundary` waits
                        // for the next idle tick. Deduped so a standing change cannot
                        // re-request a restart every tick.
                        let already_requested = agent_change_restart_requested_for.as_deref()
                            == Some(resolved.binary.as_str());
                        if decision == AgentChangeRestartAction::Restart
                            && !already_requested
                        {
                            shared.restart_reexec.store(false, Ordering::Relaxed);
                            // A harness change has no prior session in the NEW harness:
                            // request a FRESH restart (no `--continue`/`resume` args)
                            // rather than the default continue mode.
                            *shared.restart_mode.lock() = "fresh".to_string();
                            shared.restart_requested.store(true, Ordering::Relaxed);
                            agent_change_restart_requested_for = Some(resolved.binary.clone());
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "agent_restart_triggered file={} old={} new={} action=request_fresh_restart",
                                    path.display(),
                                    launch_harness_binary,
                                    resolved.binary,
                                ),
                            );
                            log_event(
                                &mut session_log,
                                &format!(
                                    "agent_restart_triggered old={} new={} action=request_fresh_restart",
                                    launch_harness_binary, resolved.binary,
                                ),
                            );
                        }
                    }
                }
                let route_submit_in_flight =
                    match agent_doc_controller_io::project_controller::route_submit_in_flight_for_file(
                        &path,
                    ) {
                        Ok(active) => active,
                        Err(err) => {
                            if !route_submit_in_flight_logged {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_skipped harness={} reason=route_submit_projection_error file={} error={:?}",
                                        harness.binary,
                                        path.display(),
                                        err.to_string()
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: failed to inspect route-submit projection for {}: {err:#}",
                                    path.display()
                                );
                                route_submit_in_flight_logged = true;
                            }
                            true
                        }
                    };
                if route_submit_in_flight {
                    if !route_submit_in_flight_logged {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_skipped harness={} reason=route_submit_in_flight file={}",
                                harness.binary,
                                path.display()
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "idle_queue_watch_skipped file={} harness={} reason=route_submit_in_flight",
                                path.display(),
                                harness.binary
                            ),
                        );
                        route_submit_in_flight_logged = true;
                    }
            } else {
                route_submit_in_flight_logged = false;
            }
            // `#recycletransitionwedge`: the transition comes from the SAME
            // controller model read that produced `active_head` this tick — never a
            // second read (a per-tick extra controller round-trip is what
            // `#idlewatchctrlbackoff` exists to prevent) and never the supervisor's
            // own process-local relay registry, which can only ever miss.
            let current_transition_pending =
                active_head.is_some() && active_transition == IdleQueueTransition::Pending;
            if current_transition_pending {
                if !current_transition_pending_logged {
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_skipped harness={} reason=current_transition_pending file={}",
                            harness.binary,
                            path.display()
                        ),
                    );
                    agent_doc_ops_log_io::log_op(
                        &path,
                        &format!(
                            "idle_queue_watch_skipped file={} harness={} reason=current_transition_pending",
                            path.display(),
                            harness.binary
                        ),
                    );
                    current_transition_pending_logged = true;
                }
            } else {
                current_transition_pending_logged = false;
            }
            let mut context_clear_projection =
                match agent_doc_controller_io::project_controller::queue_context_clear_in_flight_for_file(&path) {
                    Ok(projection) => projection,
                    Err(err) => {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_skipped harness={} reason=context_clear_projection_error file={} error={:?}",
                                harness.binary,
                                path.display(),
                                err.to_string()
                            ),
                        );
                        eprintln!(
                            "[agent-doc] idle-queue watch: failed to inspect context-clear projection for {}: {err:#}",
                            path.display()
                        );
                        None
                    }
            };
    if let Some(projection) = context_clear_projection.as_ref()
        && !supervisor_background_context_clear_enabled()
        && !context_clear_projection_source_allows_supervisor_action(projection)
    {
                let source = projection.source.as_deref().unwrap_or("legacy");
                log_event(
                    &mut session_log,
                    &format!(
                        "idle_queue_watch_context_clear_projection_dropped harness={} reason=background_context_clear_disabled source={} target={} cmd=\"{}\"",
                        harness.binary, source, projection.target, projection.command
                    ),
                );
                agent_doc_ops_log_io::log_op(
                    &path,
                    &format!(
                        "idle_queue_watch_context_clear_projection_dropped file={} harness={} reason=background_context_clear_disabled source={} target={} cmd={:?}",
                        path.display(),
                        harness.binary,
                        source,
                        projection.target,
                        projection.command
                    ),
                );
                if let Err(err) =
                    agent_doc_controller_io::project_controller::clear_queue_context_clear_in_flight_for_file(&path)
                {
                    eprintln!(
                        "[agent-doc] idle-queue watch: failed to drop unsupported context-clear projection for {}: {err:#}",
                        path.display()
                    );
                }
                context_clear_projection = None;
            }
            let context_clear_pending = context_clear_projection.as_ref().and_then(|projection| {
                supervisor_pane_payload_already_pending(&shared, &projection.command, &harness)
            });
            if route_submit_in_flight && context_clear_projection.is_some() {
                if !context_clear_route_wait_logged {
                    if let Some(projection) = context_clear_projection.as_ref() {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_context_clear_projection_wait harness={} reason=route_submit_in_flight target={} cmd=\"{}\" prompt_visible={} turn_active={}",
                                harness.binary,
                                projection.target,
                                projection.command,
                                prompt_visible,
                                turn_active
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "idle_queue_watch_context_clear_projection_wait file={} harness={} reason=route_submit_in_flight target={} cmd={:?} prompt_visible={} turn_active={}",
                                path.display(),
                                harness.binary,
                                projection.target,
                                projection.command,
                                prompt_visible,
                                turn_active
                            ),
                        );
                    }
                    context_clear_route_wait_logged = true;
                }
            } else {
                context_clear_route_wait_logged = false;
            }
            if let Some(projection) = context_clear_projection.as_ref() {
                context_reset_in_flight = true;
                awaiting_clear_settle = true;
                last_context_clear_at = Some(projection.marked_secs);
            }
            // `#qflood2`: advance the post-`/clear` settle debounce. Require
            // consecutive fresh-idle polls (reset on any busy/non-idle tick)
            // so the next drain trigger is never injected into an in-flight
            // `/clear`. Once the cleared pane has settled, drop the gate so
            // the normal drain dispatches the head.
            let clear_settle = idle_queue_context_clear_in_flight_settle_ticks(
                IdleQueueContextClearInFlightSettleFacts {
                    awaiting_clear_settle,
                    prompt_visible,
                    turn_active,
                    route_submit_in_flight,
                    clear_already_pending: context_clear_pending,
                    settled_idle_ticks: clear_settle_idle_ticks,
                    settle_threshold: CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                },
            );
            clear_settle_idle_ticks = clear_settle.settled_idle_ticks;
            if let Some(projection) = context_clear_projection.as_ref() {
                let projection_key = projection
                    .head_sha256
                    .clone()
                    .unwrap_or_else(|| projection.marked_secs.to_string());
                let resubmit_key = format!("context_clear_projection:{projection_key}");
                match idle_queue_context_clear_in_flight_decision(
                    IdleQueueContextClearInFlightFacts {
                        projection_active: true,
                        prompt_visible,
                        turn_active,
                        route_submit_in_flight,
                        clear_already_pending: context_clear_pending,
                        already_resubmitted: last_pending_enter_resubmitted.as_deref()
                            == Some(resubmit_key.as_str()),
                        settled_idle_ticks: clear_settle_idle_ticks,
                        settle_threshold: CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                    },
                ) {
                    IdleQueueContextClearInFlightDecision::ResubmitPendingClear => {
                        let head_label = active_head
                            .as_deref()
                            .or(projection.head_sha256.as_deref())
                            .unwrap_or("<unknown>");
                        match idle_queue_resubmit_pending_payload(
                            &path,
                            &shared,
                            &harness,
                            "context_clear",
                            head_label,
                            &projection.command,
                        ) {
                            AutoTriggerOutcome::Sent => {
                                last_pending_enter_resubmitted = Some(resubmit_key);
                                context_reset_in_flight = true;
                                awaiting_clear_settle = true;
                                clear_settle_idle_ticks = 0;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_clear_projection_resubmit harness={} reason=clear_already_pending target={} cmd=\"{}\"",
                                        harness.binary, projection.target, projection.command
                                    ),
                                );
                                continue;
                            }
                            AutoTriggerOutcome::Cancelled => return,
                            outcome => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_clear_projection_resubmit_failed harness={} outcome={}",
                                        harness.binary,
                                        outcome.as_str()
                                    ),
                                );
                                continue;
                            }
                        }
                    }
                    IdleQueueContextClearInFlightDecision::WaitForIdle
                    | IdleQueueContextClearInFlightDecision::WaitForPendingClear
                    | IdleQueueContextClearInFlightDecision::AwaitSettle => continue,
                    IdleQueueContextClearInFlightDecision::Settled => {
                        if let Err(err) =
                            agent_doc_controller_io::project_controller::clear_queue_context_clear_in_flight_for_file(&path)
                        {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to clear context-clear projection for {}: {err:#}",
                                path.display()
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_context_clear_projection_clear_failed file={} error={:?}",
                                    path.display(),
                                    err.to_string()
                                ),
                            );
                        } else {
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_context_clear_projection_cleared file={} harness={} after_ticks={}",
                                    path.display(),
                                    harness.binary,
                                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                                ),
                            );
                        }
                    }
                    IdleQueueContextClearInFlightDecision::Ignore => {}
                }
            }
            if clear_settle.settled_now {
                awaiting_clear_settle = false;
                clear_settle_idle_ticks = 0;
                log_event(
                    &mut session_log,
                    &format!(
                            "idle_queue_watch_clear_settled harness={} after_ticks={}",
                            harness.binary, CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                        ),
                    );
                }

                // `#clearcontresume`: a lingering manual clear cooldown projection must not
                // suppress an active go-mode queue drain forever. The cooldown's
                // only job is to avoid dispatching into an in-flight `/clear`;
                // once the cleared pane has settled to a fresh idle prompt for
                // `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls and a head
                // is waiting (and no operator-deferred clear is pending — that
                // path owns its own resume), auto-expire the cooldown so the
                // recycle + clear is a continuation step, not a stall.
                if clear_cooldown_active
                    && active_head.is_some()
                    && prompt_visible
                    && !turn_active
                    && !route_submit_in_flight
                {
                    clear_cooldown_idle_ticks = clear_cooldown_idle_ticks.saturating_add(1);
                } else {
                    clear_cooldown_idle_ticks = 0;
                }
                let deferred_operator_clear_pending =
                    agent_doc_controller_io::project_controller::queue_context_clear_deferred_operator_for_file(&path)
                        .ok()
                        .flatten()
                        .is_some();
                if clear_cooldown_resume_ready(
                    clear_cooldown_active,
                    active_head.is_some(),
                    prompt_visible,
                    turn_active,
                    deferred_operator_clear_pending,
                    clear_cooldown_idle_ticks,
                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                ) {
                    match agent_doc_controller_io::project_controller::clear_queue_context_clear_manual_cooldown_for_file(&path) {
                        Ok(_) => {
                            clear_cooldown_idle_ticks = 0;
                            clear_cooldown_logged = false;
                            last_dispatched = None;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resumed harness={} head={:?} after_ticks={}",
                                    harness.binary,
                                    active_head.as_deref().unwrap_or("<unknown>"),
                                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resumed file={} harness={} head={:?}",
                                    path.display(),
                                    harness.binary,
                                    active_head.as_deref().unwrap_or("<unknown>")
                                ),
                            );
                            eprintln!(
                                "[agent-doc] idle-queue watch: clear cooldown settled — resuming active go-mode queue drain for {}",
                                path.display()
                            );
                            // Re-evaluate next tick with the cooldown cleared so
                            // the normal drain decision dispatches the head.
                            continue;
                        }
                        Err(err) => {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to settle clear cooldown projection for {}: {err:#}",
                                path.display()
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resume_failed file={} error={:?}",
                                    path.display(),
                                    err.to_string()
                                ),
                            );
                        }
                    }
                }

                // R3 (#ctlrecycle) / #suprecyclequeue / #supselfheal: recycle this
                // supervisor onto a freshly-installed binary at a turn boundary. On unix
                // the recycle is a blue/green `execve`-preserve-child hot-reload that
                // keeps the live agent child + pane, so it now defaults ON
                // (`resolve_supervisor_auto_recycle`): a stale supervisor self-retires at
                // the next turn/queue boundary instead of re-filing File Cache Conflict /
                // IPC-drift dialogs forever. Operators opt out with a falsey
                // `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` / frontmatter / project knob, which
                // drops back to surfacing staleness once (`Detect`) for a deliberate
                // restart. A turn boundary is a dispatch-ready prompt with no turn
                // running. When a queue
                // head is still waiting to drain, the *next* queue item is the
                // deliberate restart point, so we recycle immediately (the re-exec'd
                // image re-dispatches the pending head on the fresh binary); with no
                // head waiting we debounce so a brief idle gap between unrelated turns
                // never trips it.
                let turn_boundary = prompt_visible && !turn_active;
                let head_pending = active_head.is_some();
                // `#supkill-a` — graceful, idle-gated self-kill. An external driver
                // (the CP / `admin kill-supervisor`) records a per-document request;
                // the supervisor honors it at a turn boundary by tearing down its
                // harness child and exiting cleanly (no relaunch — this is a kill, not
                // a recycle). A wedged supervisor that never reaches this point is
                // force-killed externally instead (`#supkill-b`). Checked before the
                // recycle decision so a kill request wins over a hot-reload.
                if agent_doc_supervisor::selfkill::supervisor_self_kill_action(
                    agent_doc_supervisor_io::selfkill::self_kill_requested(&path),
                    turn_boundary,
                ) {
                    let child_pid = shared.child_pid.load(Ordering::Relaxed);
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_self_killed file={} boundary=turn child_pid={} caller=self_watchdog",
                            path.display(),
                            child_pid,
                        ),
                    );
                    eprintln!(
                        "[agent-doc] supervisor self-kill requested; tearing down the harness child and exiting"
                    );
                    agent_doc_supervisor_io::selfkill::clear_self_kill_request(&path);
                    #[cfg(unix)]
                    if child_pid > 0 {
                        unsafe {
                            libc::kill(child_pid as libc::pid_t, libc::SIGTERM);
                        }
                    }
                    std::process::exit(0);
                }
                // `#supautoinstall` — dogfood auto-install rung. Default ON, but only for
                // an agent-doc dogfood session document (`install_crate_root` resolved). When a
                // finalize has committed a source edit (source mtime > installed binary
                // mtime), build+install at this turn boundary so the binary catches up; the
                // recycle block immediately below then sees the now-newer binary
                // (`process_binary_is_stale`) and hot-reloads onto it. The build runs HERE,
                // in the idle supervisor — never in the finalize client — which is what
                // root-fixes the mid-session-install drift (`#no-mid-session-install`).
                if !auto_install_disabled
                    && let Some(crate_root) = install_crate_root.as_ref()
                {
                    {
                        let source_newer = match (
                            agent_doc_fs::install_freshness::newest_crate_source_mtime_secs(
                                crate_root,
                            ),
                            agent_doc_controller_io::project_controller::current_binary_identity().ok(),
                        ) {
                            (Some(src), Some(bin)) => {
                                agent_doc_supervisor::config::source_newer_than_installed_binary(
                                    src,
                                    bin.modified_secs,
                                )
                            }
                            // Fail-open: any unreadable source/binary → not newer.
                            _ => false,
                        };
                        // An auto-install is a promotion boundary, not a build
                        // scratchpad. Installing from an in-progress dirty
                        // worktree can publish a half-edited binary and recycle
                        // every live session while its author is still typing.
                        // Wait for the source commit, then promote that stable
                        // checkout on the next idle boundary.
                        let source_ready = if source_newer {
                            match agent_doc_controller_io::project_controller::supervisor_auto_install_worktree_clean(crate_root) {
                                Ok(true) => {
                                    install_dirty_logged = false;
                                    true
                                }
                                Ok(false) => {
                                    if !install_dirty_logged {
                                        install_dirty_logged = true;
                                        log_event(
                                            &mut session_log,
                                            &format!(
                                                "supervisor_auto_install_deferred reason=source_worktree_dirty crate_root={}",
                                                crate_root.display()
                                            ),
                                        );
                                        eprintln!(
                                            "[agent-doc] supervisor auto-install deferred: {} has uncommitted changes; waiting for the source commit before promoting a binary",
                                            crate_root.display()
                                        );
                                    }
                                    false
                                }
                                Err(err) => {
                                    if !install_dirty_logged {
                                        install_dirty_logged = true;
                                        log_event(
                                            &mut session_log,
                                            &format!(
                                                "supervisor_auto_install_deferred reason=source_cleanliness_unproven crate_root={} error={err}",
                                                crate_root.display()
                                            ),
                                        );
                                        eprintln!(
                                            "[agent-doc] supervisor auto-install deferred: could not prove {} is a clean committed checkout ({err:#})",
                                            crate_root.display()
                                        );
                                    }
                                    false
                                }
                            }
                        } else {
                            install_dirty_logged = false;
                            false
                        };
                        let install_action = supervisor_install_action(
                            source_ready,
                            auto_install_enabled,
                            turn_boundary,
                        );
                        // A build is heavy → always debounce (no head-pending immediate
                        // path): a momentary idle gap mid-edit must never trip it.
                        let (do_install, next_install_since) =
                            agent_doc_controller::recycle::recycle_debounce_decision(
                                matches!(install_action, SupervisorInstallAction::Install),
                                install_stale_since,
                                std::time::Instant::now(),
                                recycle_grace,
                            );
                        install_stale_since = next_install_since;
                        if matches!(install_action, SupervisorInstallAction::Detect)
                            && !install_detected_logged
                        {
                            install_detected_logged = true;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_source_newer_detected pane={} auto_install=opted_out hint=set_AGENT_DOC_SUPERVISOR_AUTO_INSTALL_or_run_dogfood_refresh crate_root={}",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    crate_root.display(),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] agent-doc source is newer than the installed binary and supervisor auto-install is opted OUT; set AGENT_DOC_SUPERVISOR_AUTO_INSTALL=1 or run the dogfood refresh to rebuild+install"
                            );
                        }
                        if do_install {
                            install_stale_since = None;
                            // `#jbdisprecycle`: publish project mid-recycle BEFORE the
                            // rebuild+install (which takes seconds) and the `execve`
                            // that follows, so a concurrent `route` dispatch defers
                            // instead of typing a trigger that the recycle drops
                            // before submit. Refreshed at the reexec boundary; the
                            // fresh supervisor settles it on watch-loop start.
                            if let Err(err) =
                                agent_doc_controller_io::project_controller::supervisor_recycle_started_for_file(
                                &path,
                                agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
                            ) {
                                eprintln!(
                                    "[agent-doc] warning: failed to publish recycle-inflight before auto-install: {err:#}"
                                );
                            }
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_auto_install_started pane={} boundary=turn crate_root={}",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    crate_root.display(),
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "supervisor_auto_install_started file={} crate_root={} caller=idle_watch",
                                    path.display(),
                                    crate_root.display(),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] supervisor auto-install: rebuilding + installing agent-doc from {} so the next queue item runs on the freshly-committed source",
                                crate_root.display()
                            );
                            // #supinstallfeedback: tell the operator the pane is
                            // rebuilding (not stalled at the restart keepalive).
                            surface_supervisor_auto_install_status(
                                &shared,
                                SupervisorAutoInstallPhase::Started,
                            );
                            match agent_doc_controller_io::project_controller::run_supervisor_auto_install(crate_root) {
                                Ok(()) => {
                                    log_event(
                                        &mut session_log,
                                        "supervisor_auto_install_succeeded next=recycle_onto_fresh_binary",
                                    );
                                    agent_doc_ops_log_io::log_op(
                                        &path,
                                        &format!(
                                            "supervisor_auto_install_succeeded file={} next=recycle_onto_fresh_binary",
                                            path.display(),
                                        ),
                                    );
                                    eprintln!(
                                        "[agent-doc] supervisor auto-install succeeded; recycling onto the freshly-installed binary"
                                    );
                                    surface_supervisor_auto_install_status(
                                &shared,
                                        SupervisorAutoInstallPhase::Succeeded,
                                    );
                                }
                                Err(err) => {
                                    auto_install_disabled = true;
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "supervisor_auto_install_failed fallback=operator_refresh auto_install_disabled=true error={err}"
                                        ),
                                    );
                                    agent_doc_ops_log_io::log_op(
                                        &path,
                                        &format!(
                                            "supervisor_auto_install_failed file={} fallback=operator_refresh error={:?}",
                                            path.display(),
                                            err.to_string(),
                                        ),
                                    );
                                    eprintln!(
                                        "[agent-doc] supervisor auto-install failed ({err}); leaving the rebuild to the operator (run the dogfood refresh) — not re-attempting this session"
                                    );
                                    surface_supervisor_auto_install_status(
                                &shared,
                                        SupervisorAutoInstallPhase::Failed,
                                    );
                                }
                            }
                        }
                    }
                }
                let supervisor_stale = supervisor_stale_fast;
                // `#supkill-bg` — publish the live staleness probe so the IPC `Restart`
                // handler can decide drain-reexec vs immediate relaunch without
                // recomputing it.
                shared
                    .binary_stale
                    .store(supervisor_stale, Ordering::Relaxed);
                // `#suptmuxstale` — publish the same staleness probe as an on-disk
                // marker so the `turn-status` hook (a separate short-lived process in
                // the agent pane that cannot read this in-memory atomic) can decorate
                // the pane turn-in-progress title with a `⚠ STALE SUPERVISOR` warning.
                // Best-effort display surface — never let a marker write fail the watch.
                if let Some(base) = path
                    .canonicalize()
                    .ok()
                    .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))
                    && let Err(e) =
                        agent_doc_turn_status_io::set_supervisor_stale_marker(&base, supervisor_stale)
                {
                    eprintln!(
                        "[idle-watch] warning: failed to update stale-supervisor marker: {e:#}"
                    );
                }
                // `#supkill-bg` blue/green drain-and-supersede: an explicit
                // `restart-supervisor` routed to the in-place reexec path
                // (`restart_reexec`, stamped stale by the IPC handler) drains its
                // in-flight turn, then hot-reloads onto the fresh binary IN PLACE at
                // the turn boundary — the default healthy restart that fixes the
                // `generation closed` / stale-supervisor `#fcc0` case without dropping
                // the live turn. Checked before the opt-in auto-recycle decision so a
                // deliberate operator restart always supersedes (no env opt-in needed).
            // `#midturn-recycle-resume`: a stale operator-requested replacement may
            // reexec at any turn stage once supervisor IPC drains. The execve
            // preserves the harness child and durable cycle checkpoint; only an
            // active receipt handler is unsafe. Fresh-binary child relaunches retain
            // the real turn-boundary and open-cycle interlocks.
                // This prevents a long model/tool turn from pinning an obsolete supervisor.
                // `#suprecyclespin`: an open cycle whose harness turn has already
                // ended (we are at `turn_boundary`) but that never reached
                // `committed`/`abandoned` keeps `is_open()` true forever, so the
                // recycle-defer arm (`DeferCycleOpen`) fires ~2/sec indefinitely and
                // the stale supervisor never hot-reloads. This is the unbuilt
                // `#midturnresumeb` Phase B: a stalled cycle (no IPC inflight,
                // untouched past `STALLED_CYCLE_RESOLVE_SECS`) may be an abandoned
                // older turn that a newer cycle has superseded. Resolve it only at a
                // true turn boundary; mid-turn, preserve the open checkpoint so a
                // finalize/recycle race can still be resumed by the fresh supervisor.
                let inflight = agent_doc_ipc_io::inflight_connection_handlers();
                let cycle_open = match agent_doc_cycle_state_io::load_with_closeout_projection(&path)
                    .ok()
                    .flatten()
                {
                    Some(state) if state.is_open() => {
                        let before_response_capture_cycle_stalled = turn_boundary
                            && state.stalled_before_response_capture_cycle(
                                inflight,
                                current_epoch_secs(),
                                agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_SECS,
                            );
                        // `#suprecyclespin-falseabandon`: require the stall to
                        // persist across `STALLED_CYCLE_RESOLVE_CONFIRM_TICKS`
                        // consecutive polls before abandoning. A live generation
                        // does not hold `turn_boundary && stalled` back-to-back for
                        // the confirm window, so a transient boundary misread can no
                        // longer abandon a live turn from a stale sidecar; a truly
                        // orphaned cycle stays stalled every poll and still resolves.
                        if before_response_capture_cycle_stalled {
                            stalled_resolve_streak = stalled_resolve_streak.saturating_add(1);
                        } else {
                            stalled_resolve_streak = 0;
                        }
                        let stall_confirmed = before_response_capture_cycle_stalled
                            && stalled_resolve_streak
                                >= agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_CONFIRM_TICKS;
                        if stall_confirmed {
                            stalled_resolve_streak = 0;
                            let stalled_secs =
                                current_epoch_secs().saturating_sub(state.updated_at);
                            if let Err(err) = agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(&agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
                                &path,
                                "suprecyclespin_stalled_cycle_resolved",
                                None,
                                None,
                            ) {
                                agent_doc_ops_log_io::log_op(
                                    &path,
                                    &format!(
                                        "supervisor_cycle_stale_resolve_failed file={} cycle={} err={err:#} (#suprecyclespin)",
                                        path.display(),
                                        state.cycle_id,
                                    ),
                                );
                            }
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "supervisor_cycle_stale_resolved file={} cycle={} turn={} phase={} stalled_secs={} inflight={} confirm_ticks={} reason=abandoned_older_turn_superseded (#suprecyclespin)",
                                    path.display(),
                                    state.cycle_id,
                                    state.turn_id.as_deref().unwrap_or("<none>"),
                                    state.phase.as_str(),
                                    stalled_secs,
                                    inflight,
                                    agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_CONFIRM_TICKS,
                                ),
                            );
                            // Gate cleared — let the recycle proceed at this boundary.
                            inflight > 0
                        } else {
                            true
                        }
                    }
                    _ => inflight > 0,
                };
                // `#midturn-recycle-resume` Phase B escalation backstop (layered on the
                // `#suprecyclespin` stalled-resolve above): count consecutive cycle-open
                // recycle deferrals at a `turn_boundary` (the only place a recycle would
                // otherwise fire); once the streak reaches `MAX_CYCLE_OPEN_DEFER_TICKS`,
                // force the recycle DECISION for this tick via `effective_cycle_open`.
                // Off a boundary the recycle never fires, so an open cycle there is not
                // starving anything and must not accrue the streak.
                if cycle_open && turn_boundary {
                    cycle_open_defer_streak = cycle_open_defer_streak.saturating_add(1);
                } else {
                    cycle_open_defer_streak = 0;
                    cycle_open_defer_escalated_logged = false;
                }
                let escalate_cycle_open = cycle_open_defer_escalates(
                    cycle_open_defer_streak,
                    MAX_CYCLE_OPEN_DEFER_TICKS,
                );
                if escalate_cycle_open && !cycle_open_defer_escalated_logged {
                    cycle_open_defer_escalated_logged = true;
                    agent_doc_ops_log_io::log_op(
                        &path,
                        &format!(
                            "supervisor_recycle_cycle_open_escalated file={} pane={} streak={} threshold={} inflight={} action=force_recycle reason=cycle_never_closed (#midturn-recycle-resume)",
                            path.display(),
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            cycle_open_defer_streak,
                            MAX_CYCLE_OPEN_DEFER_TICKS,
                            inflight,
                        ),
                    );
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_recycle_cycle_open_escalated streak={} threshold={} action=force_recycle reason=cycle_never_closed",
                            cycle_open_defer_streak,
                            MAX_CYCLE_OPEN_DEFER_TICKS,
                        ),
                    );
                }
            // The cycle-open escalation remains the backstop for ordinary recycle
            // and fresh-binary child relaunch. Stale in-place replacement instead
            // uses the no-IPC safe checkpoint below: execve preserves both the
            // child and durable cycle, so a stale turn marker cannot starve it.
            let effective_cycle_open = cycle_open && !escalate_cycle_open;
            let stale_restart_safe_checkpoint =
                stale_recycle_safe_checkpoint(supervisor_stale, inflight);
            let restart_action = supervisor_restart_action(
                shared.restart_requested.load(Ordering::Relaxed),
                shared.restart_reexec.load(Ordering::Relaxed),
                turn_boundary,
                stale_restart_safe_checkpoint,
            );
            if !reexec_recycle_disabled
                && (!effective_cycle_open || stale_restart_safe_checkpoint)
                && matches!(restart_action, SupervisorRestartAction::ReexecInPlace)
            {
                    #[cfg(unix)]
                    {
                        let candidate_notes = supervisor_reexec_candidates()
                            .iter()
                            .map(|(path, note)| format!("{note}={}", path.display()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_restart_drain_reexec via=execve_preserve_child boundary=turn pane={} child_pid={} master_fd={} current_exe={:?} candidates=[{candidate_notes}]",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                                std::env::current_exe().ok(),
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "supervisor_restart_drain_reexec file={} pane={} action=drain_and_supersede caller=operator",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor restart: draining complete, hot-reloading onto freshly-installed agent-doc binary; preserving the live agent child via execve"
                        );
                        // `#jbdisprecycle`: refresh the CP recycle-in-flight graph
                        // immediately before the `execve` so a concurrent dispatch
                        // defers across the hot-reload boundary.
                        if let Err(err) =
                            agent_doc_controller_io::project_controller::supervisor_recycle_started_for_file(
                            &path,
                            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_RESTART,
                        ) {
                            eprintln!(
                                "[agent-doc] warning: failed to publish recycle-inflight before restart reexec: {err:#}"
                            );
                        }
                        match supervisor_perform_reexec(&shared) {
                            Ok(never) => match never {},
                            Err(err) => {
                                // A failed execve must NOT strand the restart. Clear the
                                // reexec intent so the in-process host loop's restart-kill
                                // condition fires and relaunches the child on the current
                                // binary (the restart still happens, pane survives), and
                                // disable further reexec attempts this lifetime.
                                shared.restart_reexec.store(false, Ordering::Relaxed);
                                reexec_recycle_disabled = true;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "supervisor_restart_reexec_failed fallback=relaunch_current_binary error={err}"
                                    ),
                                );
                                agent_doc_ops_log_io::log_op(
                                    &path,
                                    &format!(
                                        "supervisor_restart_reexec_failed file={} pane={} fallback=relaunch_current_binary error={:?}",
                                        path.display(),
                                        shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                        err.to_string(),
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] supervisor restart execve hot-reload failed ({err}); relaunching on the current binary"
                                );
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        // No execve on non-unix: fall back to the normal relaunch.
                        shared.restart_reexec.store(false, Ordering::Relaxed);
                    }
                }
                // `#supselfheal` Phase 1: the policy owner now accepts an
                // `explicit_admin` override (an `agent-doc admin recycle` request
                // recycles a stale supervisor even when auto-recycle is opted OUT).
                // The `admin recycle` → route-owned-supervisor IPC adapter that flips
                // this to `true` is the queued follow-up (`#supselfheal-adminwire`);
                // until it lands the idle path keeps its current behavior.
                // `#turnsaferecycle` Goal 1 — an install fan-out
                // (`recycle_supervisors_all_projects_force`) writes a per-document
                // recycle-request marker so EVERY route-owned supervisor recycles onto
                // the freshly-installed binary at its next idle boundary, not just the
                // ones that independently self-detect staleness. Honor it like an
                // explicit admin recycle: `supervisor_recycle_action` maps that to
                // `RecycleImmediate` whether or not the running binary reads stale. The
                // marker is cleared immediately before the `execve` below so the fresh
                // process does not re-loop on it.
                let recycle_request =
                    agent_doc_supervisor_io::recycle_request::fresh_recycle_request(
                        &file,
                        current_epoch_secs(),
                    );
                let recycle_requested = recycle_request.is_some();
                let stale_editor_replica_requested = recycle_request.as_ref().is_some_and(|request| {
                    request.reason
                        == agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE
                });
                let explicit_admin_recycle = recycle_requested;
                // `#lazily-recycle-request`: mirror the pending recycle/restart request
                // onto the lazily statechart (phase `Requested`) so route callers and
                // the editor observe the intent through the state subscription instead
                // of polling this marker file. Best-effort and idempotent — the
                // controller handler no-ops unless the chart is `Settled`, so
                // re-emitting while the request stays pending is cheap and burns no
                // epochs. Every recycle cause (admin, stale-preflight, install fan-out,
                // wedge) writes this marker, so this single point covers them all.
                if recycle_requested
                    && let Ok(canonical) = path.canonicalize()
                {
                    let reason = recycle_request
                        .as_ref()
                        .map(|request| request.reason.clone())
                        .unwrap_or_else(|| "supervisor_recycle_requested".to_string());
                    if let Err(err) =
                        agent_doc_controller_io::project_controller::supervisor_recycle_requested_for_file(
                            &canonical, &reason,
                        )
                    {
                        eprintln!(
                            "[agent-doc] idle-queue watch: failed to record recycle request on lazily statechart for {}: {err:#}",
                            canonical.display()
                        );
                    }
                }
                // `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): read the
                // persisted editor-IPC wedge fact for the owned document. The
                // write/converge closeout path latches `degraded` after repeated
                // `send_failed`/`no_ack` against a nominally-active JB listener and
                // logs `write_wedged_supervisor_recycle_requested`; the supervisor
                // reads that latch here and combines it with its own staleness probe,
                // so a wedge against a stale binary recycles immediately instead of
                // waiting for an opt-in or an idle boundary that may never come.
                // `#midturn-wedge-recycle`: use the once-per-episode signal. It is
                // true only while the wedge is latched AND no recycle has yet been
                // attempted for this episode, so the recycle fires mid-turn (even on a
                // fresh binary) but at most once — the guard is latched on the marker
                // right before the execve below.
                let wedge_needs_recycle = path
                    .canonicalize()
                    .ok()
                    .map(|canonical| {
                        let project_root =
                            agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
                        agent_doc_write_converge_io::editor_ipc_write_wedge_needs_recycle(
                            &project_root,
                            &canonical,
                        )
                    })
                    .unwrap_or(false);
                // Recycle a wedged OR stale supervisor at the FIRST AVAILABLE SAFE
                // intra-turn checkpoint, not merely at an idle prompt. A tick is a safe checkpoint when no
                // supervisor IPC connection is being handled
                // (`inflight_connection_handlers() == 0`): an `execve` recycle there
                // cannot sever an active patch apply mid-flight. The wedge is already
                // capture-backed (the write closeout parked the response + retry patch
                // before latching the wedge), so an in-flight handler is the only unsafe
                // window. Off a safe checkpoint the recycle defers to the next tick and
                // re-checks — it still fires mid-turn (no wait for the full turn
                // boundary), just at the earliest point that cannot corrupt in-flight IO.
            let inflight_handlers = inflight;
            let at_safe_checkpoint = inflight_handlers == 0;
            let stale_safe_checkpoint = stale_restart_safe_checkpoint;
                let write_wedged = wedge_needs_recycle && at_safe_checkpoint;
                let editor_delivery_stale =
                    stale_editor_replica_requested && at_safe_checkpoint;
                if wedge_needs_recycle && !at_safe_checkpoint {
                    agent_doc_ops_log_io::log_op(
                        &path,
                        &format!(
                            "supervisor_wedge_recycle_deferred_unsafe_checkpoint file={} pane={} inflight={} reason=await_safe_intra_turn_checkpoint (#midturn-wedge-recycle)",
                            path.display(),
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            inflight_handlers,
                        ),
                    );
                }
                if stale_editor_replica_requested {
                    agent_doc_ops_log_io::log_op(
                        &path,
                        &format!(
                            "supervisor_stale_editor_replica_recycle file={} pane={} inflight={} action={} reason=refresh_delivery_worker_preserve_capture (#editor-delivery-liveness)",
                            path.display(),
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            inflight_handlers,
                            if editor_delivery_stale {
                                "recycle_at_safe_checkpoint"
                            } else {
                                "defer_until_safe_checkpoint"
                            },
                        ),
                    );
                }
            let recycle_action = supervisor_recycle_action(
                supervisor_stale,
                recycle_auto_enabled,
                // A nominal prompt boundary cannot override an active supervisor
                // IPC handler. The live handler count is the authoritative I/O
                // safety fact for both ordinary and stale recycle.
                (turn_boundary && at_safe_checkpoint) || stale_safe_checkpoint,
                head_pending,
                    explicit_admin_recycle,
                    write_wedged,
                    editor_delivery_stale,
                    reexec_failed,
                    // A stale supervisor's safe intra-turn checkpoint owns the durable
                    // resume handoff, so an open cycle does not block that hot reload.
                    // Non-stale operator replacement retains the cycle-open interlock.
                    effective_cycle_open && !stale_safe_checkpoint,
                );
                if matches!(recycle_action, SupervisorRecycleAction::DeferCycleOpen) {
                    agent_doc_ops_log_io::log_op(
                        &path,
                        &format!(
                            "supervisor_recycle_deferred_cycle_open file={} pane={} stale={} inflight={} reason=agent_doc_cycle_open (#midturn-recycle-resume)",
                            path.display(),
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            supervisor_stale,
                            agent_doc_ipc_io::inflight_connection_handlers(),
                        ),
                    );
                }
                // `#midturn-wedge-recycle`: if this tick's recycle is being driven by a
                // proven editor-IPC wedge, latch the once-per-episode guard on the
                // dewedge marker BEFORE any recycle path runs — the `execve` below never
                // returns, so the fresh supervisor must not re-read the still-latched
                // wedge and recycle-loop. Latching for both the in-place recycle and the
                // kill+relaunch escalation covers every wedge-driven recycle path.
                if write_wedged
                    && matches!(
                        recycle_action,
                        SupervisorRecycleAction::RecycleImmediate
                            | SupervisorRecycleAction::EscalateKillRelaunch
                    )
                    && let Ok(canonical) = path.canonicalize()
                {
                    let project_root =
                        agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
                    if let Err(err) = agent_doc_write_converge_io::mark_ipc_wedge_recycle_attempted(
                        &project_root,
                        &canonical,
                    ) {
                        eprintln!(
                            "[agent-doc] idle-queue watch: failed to latch wedge recycle guard for {}: {err:#}",
                            path.display()
                        );
                    }
                }
                // `#wd40` / `#staleloop-recycle-restart`: automate the manual
                // `make install` + `admin recycle` + end-turn the operator had to
                // run to force a stale supervisor onto a fresh binary during a
                // continuously self-draining session. The recycle above only fires
                // at a `turn_boundary` (`prompt_visible && !turn_active`), but a
                // self-driving `/loop` holds a fresh drain-owner lease AND keeps
                // the harness `turn_active` back-to-back, so the boundary is never
                // reached. When this supervisor is stale, a loop owns the drain,
                // and a recycle WOULD fire at a boundary, publish a controller
                // recycle-yield projection: the in-session loop reads it at its next
                // inter-item boundary (via `queue_continuation::detect` /
                // `session-check` / preflight), yields one boundary instead of
                // re-triggering, and the resulting idle turn lets the `execve`
                // recycle fire on its own. After the recycle the fresh supervisor
                // (no longer stale) clears the projection and the drain resumes.
                {
                    let drain_owner_active = agent_doc_queue_io::drain_owner::fresh_loop_drain_owner_lease(
                        &file,
                        current_epoch_secs(),
                    )
                    .is_some();
                    // Would a real recycle (or the Phase-3 escalation) fire if the
                    // boundary were reachable? A bare `Detect` (auto-recycle opted
                    // out, no admin/wedge) does NOT hot-reload, so yielding the loop
                    // for it would only stall the drain.
                    // `#midturn-recycle-resume`: this is the stale-intent question
                    // ("would a recycle EVER fire at a boundary?"), not the live-tick
                    // gate, so pass `cycle_open=false`. A transiently-open cycle must
                    // not suppress the yield request — the cycle closes momentarily and
                    // then the deferred recycle fires; suppressing the yield here would
                    // stall the drain for a stale binary that genuinely needs the loop
                    // to yield a boundary.
                    let would_recycle_at_boundary = !matches!(
                        supervisor_recycle_action(
                            supervisor_stale,
                            recycle_auto_enabled,
                            true,
                            head_pending,
                            explicit_admin_recycle,
                            write_wedged,
                            editor_delivery_stale,
                            reexec_failed,
                            false,
                        ),
                        SupervisorRecycleAction::None | SupervisorRecycleAction::Detect
                    );
                    // Never yield-loop a supervisor that can no longer converge: once
                    // the bounded kill+relaunch escalation is exhausted the recycle
                    // will never fire, so asking the loop to keep yielding would stall
                    // the drain forever. Fall through to the one-time operator-restart
                    // hint instead (`#supselfheal` Phase 3 exhaustion).
                    if !reexec_escalation_exhausted_logged
                        && stale_drain_recycle_yield_requested(
                            would_recycle_at_boundary,
                            drain_owner_active,
                            turn_boundary,
                        )
                    {
                        // Refresh the request every tick so it stays live until the
                        // loop yields; log once. The reason distinguishes a stale-binary
                        // swap from a fresh-binary state flush (`#wd40`: an explicit
                        // `admin recycle` restarts the process to clear a lagging CRDT
                        // projection even when the installed binary already matches).
                        let yield_reason = if supervisor_stale {
                            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY
                        } else {
                            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STATE_FLUSH
                        };
                        if let Err(err) =
                            agent_doc_controller_io::project_controller::supervisor_recycle_yield_requested_for_file(
                                &path,
                                yield_reason,
                            )
                        {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to publish recycle-yield request for {}: {err:#}",
                                path.display()
                            );
                        } else if !recycle_yield_requested_logged {
                            recycle_yield_requested_logged = true;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_recycle_yield_requested pane={} reason={yield_reason} turn_active={} drain_owner=loop note=loop_yields_to_let_execve_recycle_fire",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    turn_active,
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "supervisor_recycle_yield_requested file={} pane={} reason={yield_reason} action=signal_loop_yield",
                                    path.display(),
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                ),
                            );
                            let cause = if supervisor_stale {
                                "supervisor binary is stale"
                            } else {
                                "an explicit admin recycle wants stale in-memory state flushed"
                            };
                            eprintln!(
                                "[agent-doc] {cause} while a self-draining loop owns the drain; requesting the in-session loop to yield one boundary so the recycle can hot-reload/restart at a clean boundary"
                            );
                        }
                    } else if !supervisor_stale {
                        // Post-recycle (or no longer stale): drop any leftover
                        // request so the loop resumes draining on the fresh binary.
                        // Reset the log latch so a later staleness can re-request.
                        if let Err(err) =
                            agent_doc_controller_io::project_controller::clear_supervisor_recycle_yield_for_file(&path)
                        {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to clear recycle-yield request for {}: {err:#}",
                                path.display()
                            );
                        }
                        recycle_yield_requested_logged = false;
                    }
                }
                // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): a stale
                // supervisor whose in-place `execve` already failed cannot converge by
                // re-execing again. Escalate to a bounded kill+relaunch of the harness
                // child — reusing the `#supkill-bg` drain-and-relaunch path (request a
                // non-reexec restart; it drains the in-flight turn first, so no live
                // turn is dropped) — instead of looping `continue_current_binary`
                // forever (the indefinite wedge this plan fixes). Bounded so a relaunch
                // that never clears the staleness cannot spin into an unbounded kill
                // loop; past the bound, fall back to continuing on the current binary
                // with a one-time operator-restart hint.
                if matches!(recycle_action, SupervisorRecycleAction::EscalateKillRelaunch) {
                    if reexec_escalation_within_bound(
                        reexec_escalation_attempts,
                        MAX_REEXEC_ESCALATIONS,
                    ) {
                        reexec_escalation_attempts += 1;
                        shared.restart_reexec.store(false, Ordering::Relaxed);
                        shared.restart_requested.store(true, Ordering::Relaxed);
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_reexec_escalate_kill_relaunch pane={} attempt={}/{} reason=reexec_failed",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                                MAX_REEXEC_ESCALATIONS,
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "supervisor_reexec_escalate_kill_relaunch file={} pane={} attempt={}/{} action=request_kill_relaunch reason=reexec_failed",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                                MAX_REEXEC_ESCALATIONS,
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor in-place hot-reload failed; escalating to a kill+relaunch of the harness child (attempt {}/{}) to clear the wedge",
                            reexec_escalation_attempts, MAX_REEXEC_ESCALATIONS,
                        );
                    } else if !reexec_escalation_exhausted_logged {
                        reexec_escalation_exhausted_logged = true;
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_reexec_escalation_exhausted pane={} attempts={} fallback=continue_current_binary",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "supervisor_reexec_escalation_exhausted file={} pane={} attempts={} fallback=continue_current_binary",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor kill+relaunch escalation exhausted after {} attempts; continuing on the current binary — restart this session to pick up the new build",
                            reexec_escalation_attempts,
                        );
                        if let Some(pane) = shared.inject_pane.as_deref() {
                            let _ = show_pane_message(
                                pane,
                                "3000",
                                "agent-doc: binary hot-reload + relaunch failed; restart this session to pick up the new build",
                            );
                        }
                    }
                    // The escalation owns this tick's recycle decision; the requested
                    // restart is consumed at the next tick's restart-action boundary.
                    continue;
                }
                // The idle-grace debounce only gates the no-head-pending path; an
                // inter-queue-item recycle bypasses it.
                let (recycle_debounced, next_recycle_since) =
                    agent_doc_controller::recycle::recycle_debounce_decision(
                        matches!(recycle_action, SupervisorRecycleAction::RecycleDebounced),
                        recycle_stale_since,
                        std::time::Instant::now(),
                        recycle_grace,
                    );
                recycle_stale_since = next_recycle_since;
                if matches!(recycle_action, SupervisorRecycleAction::Detect)
                    && !recycle_detected_logged
                {
                    recycle_detected_logged = true;
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_binary_stale_detected pane={} auto_recycle=opted_out hint=restart_or_unset_AGENT_DOC_SUPERVISOR_AUTO_RECYCLE",
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                        ),
                    );
                    eprintln!(
                        "[agent-doc] supervisor is running a prior agent-doc binary and turn-boundary self-recycle is opted OUT; restart this session (or clear the falsey AGENT_DOC_SUPERVISOR_AUTO_RECYCLE / frontmatter / project knob) to pick up the new build"
                    );
                }
                // `#recycleidleonly` / `#eqmv`: a ROUTINE stale-binary recycle must
                // wait for a real turn boundary — a pending queue head does NOT
                // license recycling mid-turn (it only bypasses the idle-grace
                // debounce above). Policy + rationale live in
                // `agent_doc_controller::recycle`.
                let routine_recycle_deferred_intra_turn =
                    agent_doc_controller::recycle::routine_stale_recycle_deferred_intra_turn(
                        matches!(recycle_action, SupervisorRecycleAction::RecycleDebounced),
                        turn_boundary,
                    );
                if routine_recycle_deferred_intra_turn {
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_binary_stale_recycle_deferred pane={} reason=await_turn_boundary (#recycleidleonly)",
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                        ),
                    );
                }
                let do_recycle = !reexec_recycle_disabled
                    && !routine_recycle_deferred_intra_turn
                    && match recycle_action {
                        SupervisorRecycleAction::RecycleImmediate => true,
                        SupervisorRecycleAction::RecycleDebounced => recycle_debounced,
                        _ => false,
                    };
                if do_recycle {
                    // `#ctlrecycle` R3 — hot-reload onto the fresh binary IN PLACE via
                    // `execve`, preserving the live harness child + tmux pane. Falls
                    // back to a clean exit (child restarts) if the in-place swap cannot
                    // start.
                    let recycle_boundary = if stale_safe_checkpoint && !turn_boundary {
                        "safe_intra_turn"
                    } else if head_pending {
                        "next_queue_item"
                    } else {
                        "idle"
                    };
                    #[cfg(unix)]
                    {
                        // Pre-attempt provenance so a recurring recycle failure is
                        // diagnosable even if the exec wedges: record the launch
                        // identity, current_exe, and the ordered candidate ladder
                        // the exec will walk.
                        let candidate_notes = supervisor_reexec_candidates()
                            .iter()
                            .map(|(path, note)| format!("{note}={}", path.display()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_binary_stale_self_recycled via=execve_preserve_child boundary={} pane={} child_pid={} master_fd={} current_exe={:?} candidates=[{candidate_notes}]",
                                recycle_boundary,
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                                std::env::current_exe().ok(),
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "supervisor_binary_stale_self_recycled file={} pane={} boundary={} via=execve_preserve_child child_pid={} master_fd={} candidates=[{candidate_notes}]",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                recycle_boundary,
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary ({recycle_boundary}); preserving the live agent child via execve"
                        );
                        // `#jbdisprecycle`: refresh the CP recycle-in-flight graph
                        // immediately before the `execve` so a concurrent dispatch
                        // defers across the hot-reload boundary (this is the path
                        // that emits the `(next_queue_item)`/`(idle)` hot-reload
                        // lines seen in the live repro).
                        if let Err(err) =
                            agent_doc_controller_io::project_controller::supervisor_recycle_started_for_file(
                            &path,
                            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
                        ) {
                            eprintln!(
                                "[agent-doc] warning: failed to publish recycle-inflight before self-recycle reexec: {err:#}"
                            );
                        }
                        // `#turnsaferecycle` Goal 1 — consume any install-fanout
                        // recycle-request marker BEFORE the `execve` so the fresh
                        // process (now serving the current binary) does not immediately
                        // re-recycle in a loop.
                        agent_doc_supervisor_io::recycle_request::clear_recycle_request(&file);
                        match supervisor_perform_reexec(&shared) {
                            Ok(never) => match never {},
                            Err(err) => {
                                // `#suprecyclestall` — a failed execve must NOT kill
                                // the session. Previously we `process::exit(0)` here,
                                // which orphaned the live harness child and hung the
                                // tmux pane (no relaunch exists). Instead log the full
                                // per-candidate diagnostics, disable further recycle
                                // attempts for this supervisor (so we don't re-spam a
                                // hopeless execve every idle boundary), surface a
                                // one-time operator hint, and keep running on the
                                // current binary. The operator restarts deliberately
                                // to pick up the new build.
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "supervisor_reexec_failed fallback=continue_current_binary recycle_disabled=true error={err}"
                                    ),
                                );
                                agent_doc_ops_log_io::log_op(
                                    &path,
                                    &format!(
                                        "supervisor_reexec_failed file={} pane={} boundary={} fallback=continue_current_binary error={:?}",
                                        path.display(),
                                        shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                        recycle_boundary,
                                        err.to_string(),
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] supervisor execve hot-reload failed ({err}); escalating to a bounded kill+relaunch on the next idle boundary"
                                );
                                if let Some(pane) = shared.inject_pane.as_deref() {
                                    let _ = show_pane_message(
                                        pane,
                                        "3000",
                                        "agent-doc: binary hot-reload failed; escalating to kill+relaunch",
                                    );
                                }
                                reexec_recycle_disabled = true;
                                // `#supselfheal` Phase 3: the in-place execve cannot
                                // start (deleted inode / syscall error). Mark the
                                // failure so the recycle policy returns
                                // `EscalateKillRelaunch` next tick instead of sitting
                                // forever on `continue_current_binary`.
                                reexec_failed = true;
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_binary_stale_self_recycled via=process_exit boundary={} pane={}",
                                recycle_boundary,
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor recycling onto freshly-installed agent-doc binary ({recycle_boundary}); the next launch uses the new build"
                        );
                        std::process::exit(0);
                    }
                }

                // `#autoloop-command-preemption` Phase 2b: a non-interrupting
                // `session clear` against this busy auto-loop deferred itself
                // (recorded a context-clear deferred projection). Deliver that
                // clear here at the idle gap, then promote the projection to
                // in-flight. When no projection exists this is a complete no-op, so
                // the existing drain path below is unchanged.
                let deferred_clear =
                    agent_doc_controller_io::project_controller::queue_context_clear_deferred_operator_for_file(&path)
                        .ok()
                        .flatten();
                match agent_doc_queue::queue_preemption::plan_deferred_clear_step(
                    deferred_clear.is_some(),
                    prompt_visible && !turn_active,
                ) {
                    agent_doc_queue::queue_preemption::DeferredClearStep::None => {}
                    agent_doc_queue::queue_preemption::DeferredClearStep::WaitForIdle => {
                        // Pending clear, pane still mid-turn: do not interrupt
                        // in-flight work; wait for the next idle tick.
                        continue;
                    }
                    agent_doc_queue::queue_preemption::DeferredClearStep::Deliver => {
                        let clear_cmd = deferred_clear
                            .as_ref()
                            .map(|d| d.command.clone())
                            .unwrap_or_default();
                        match auto_trigger_clear_command(&shared, &stop, &clear_cmd) {
                            AutoTriggerOutcome::Cancelled => return,
                            AutoTriggerOutcome::Sent => {
                                // Resume: the started projection below supersedes
                                // the deferred projection, so a later tick can
                                // drain normally after the clear settles.
                                last_dispatched = None;
                                awaiting_clear_settle = true;
                                context_reset_in_flight = true;
                                clear_settle_idle_ticks = 0;
                                if let Some(head) = active_head.clone() {
                                    last_context_reset_head = Some(head);
                                }
                                last_context_clear_at = Some(current_epoch_secs());
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    &clear_cmd,
                                );
                                if let Some(projection) = deferred_clear.as_ref() {
                                    if let Err(err) = agent_doc_controller_io::project_controller::queue_context_clear_started_for_file(
                                        &path,
                                        &projection.target,
                                        &projection.harness,
                                        &projection.command,
                                        projection
                                            .source
                                            .as_deref()
                                            .unwrap_or(CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED),
                                        active_head.as_deref(),
                                    ) {
                                        eprintln!(
                                            "[agent-doc] idle-queue watch: failed to promote deferred clear projection: {err:#}"
                                        );
                                    }
                                } else {
                                    record_context_clear_in_flight_projection(
                                        &path,
                                        &shared,
                                        &harness,
                                        &clear_cmd,
                                        CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED,
                                        active_head.as_deref(),
                                    );
                                }
                                clear_cooldown_logged = false;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_deferred_clear_delivered harness={} cmd=\"{}\"",
                                        harness.binary, clear_cmd
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: delivered deferred operator clear and resumed the loop"
                                );
                                // Let the clear run; resume drains on a later tick.
                                continue;
                            }
                            _ => {
                                // Delivery failed: keep the marker and retry on
                                // the next idle tick (do not resume yet).
                                log_event(
                                    &mut session_log,
                                    "idle_queue_watch_deferred_clear_failed",
                                );
                                continue;
                            }
                        }
                    }
                }

                // Background context-reset injection used to live here for
                // `[clean-session]`, `[focused-cycle]`, and opted-in Codex
                // accretion/threshold resets. It is disabled: ordinary queue
                // heads drain in pane, while only explicit operator clears and
                // explicit queued slash commands may submit a visible clear.
                let context_reset_reason = if supervisor_background_context_clear_enabled() {
                    let forced_context_reset_reason = active_head
                        .as_deref()
                        .and_then(|head| forced_context_reset_reason_for_head(&path, head));
                    if clean_session_head_forces_context_reset(
                        forced_context_reset_reason.is_some(),
                        clear_cooldown_active,
                    ) {
                        context_reset_policy_error_logged = false;
                        forced_context_reset_reason.map(str::to_string)
                    } else if harness.binary == "codex" {
                        match agent_doc_codex_hook_io::codex_queue_context_reset_reason(
                            &path,
                            last_context_clear_at,
                        ) {
                            Ok(reason) => {
                                context_reset_policy_error_logged = false;
                                reason
                            }
                            Err(err) => {
                                if !context_reset_policy_error_logged {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_context_reset_policy_failed harness={} file={} error={:?}",
                                            harness.binary,
                                            path.display(),
                                            err.to_string()
                                        ),
                                    );
                                    eprintln!(
                                        "[agent-doc] idle-queue watch: failed to resolve Codex queue context reset for {}: {err:#}",
                                        path.display()
                                    );
                                    context_reset_policy_error_logged = true;
                                }
                                None
                            }
                        }
                    } else {
                        context_reset_policy_error_logged = false;
                        None
                    }
                } else {
                    context_reset_policy_error_logged = false;
                    None
                };
                let reset_already_sent_for_active_slot = context_reset_dedupe_head(
                    active_head.as_deref(),
                    last_context_reset_head.as_deref(),
                    context_reset_in_flight,
                );
                match idle_queue_context_reset_decision_with_current_transition(
                    prompt_visible,
                    turn_active,
                    route_submit_in_flight,
                    current_transition_pending,
                    active_head.as_deref(),
                    reset_already_sent_for_active_slot,
                    context_reset_reason.is_some(),
                ) {
                    IdleQueueContextResetDecision::Reset => {
                        let head = active_head.as_deref().unwrap_or("<unknown>");
                        let clear_cmd = harness.context_clear_command();
                        // `#qflood2`: never stack a second `/clear` when one is
                        // already pending in the composer. For Enter-key
                        // profiles, a proven pending draft can be recovered by
                        // pressing Enter once instead of declaring the queue
                        // handled.
                        let clear_already_pending =
                            supervisor_pane_payload_already_pending(&shared, clear_cmd, &harness);
                        let resubmit_key = format!("context_reset:{head}");
                        if idle_queue_pending_payload_needs_enter_resubmit(
                            &harness.binary,
                            clear_already_pending,
                            last_pending_enter_resubmitted.as_deref()
                                == Some(resubmit_key.as_str()),
                        ) {
                            match idle_queue_resubmit_pending_payload(
                                &path,
                                &shared,
                                &harness,
                                "context_clear",
                                head,
                                clear_cmd,
                            ) {
                                AutoTriggerOutcome::Sent => {
                                    last_pending_enter_resubmitted = Some(resubmit_key);
                                    last_context_reset_head = active_head.clone();
                                    last_context_clear_at = Some(current_epoch_secs());
                                    context_reset_in_flight = true;
                                    awaiting_clear_settle = true;
                                    clear_settle_idle_ticks = 0;
                                    record_context_clear_in_flight_projection(
                                        &path,
                                        &shared,
                                        &harness,
                                        clear_cmd,
                                        CONTEXT_CLEAR_SOURCE_BACKGROUND_RESET,
                                        Some(head),
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_context_reset_resubmit harness={} reason=clear_already_pending head={:?}",
                                            harness.binary, head
                                        ),
                                    );
                                    continue;
                                }
                                AutoTriggerOutcome::Cancelled => return,
                                outcome => {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_context_reset_resubmit_failed harness={} outcome={}",
                                            harness.binary,
                                            outcome.as_str()
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                        if drain_dispatch_dedup_skip(clear_already_pending) {
                            last_context_reset_head = active_head.clone();
                            context_reset_in_flight = true;
                            awaiting_clear_settle = true;
                            clear_settle_idle_ticks = 0;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_context_reset_skipped harness={} reason=clear_already_pending head={:?}",
                                    harness.binary, head
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "queue_dispatch_skipped file={} harness={} reason=clear_already_pending",
                                    path.display(),
                                    harness.binary
                                ),
                            );
                            continue;
                        }
                        match auto_trigger_clear_command(&shared, &stop, clear_cmd) {
                            AutoTriggerOutcome::Cancelled => return,
                            AutoTriggerOutcome::Sent => {
                                last_context_reset_head = active_head.clone();
                                last_context_clear_at = Some(current_epoch_secs());
                                context_reset_in_flight = true;
                                last_dispatched = None;
                                awaiting_clear_settle = true;
                                clear_settle_idle_ticks = 0;
                                // `#qcontdrain` retired the `#freshgrant` fresh-context
                                // grant: the in-session `/loop` now drains
                                // `[clean-session]` heads in place, so the supervisor no
                                // longer needs to write a grant to un-defer the
                                // freshly-cleared agent. The force-`/clear` for a
                                // clean-session head (`#cleandrainsup`) still happens
                                // above (`active_head_is_clean_session`); it just no
                                // longer records a grant sidecar.
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    clear_cmd,
                                );
                                record_context_clear_in_flight_projection(
                                    &path,
                                    &shared,
                                    &harness,
                                    clear_cmd,
                                    CONTEXT_CLEAR_SOURCE_BACKGROUND_RESET,
                                    Some(head),
                                );
                                log_idle_queue_context_reset_submit(
                                    &path,
                                    &shared,
                                    &harness,
                                    clear_cmd,
                                    head,
                                    context_reset_reason.as_deref().unwrap_or(""),
                                );
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_reset harness={} cmd=\"{}\" head={:?} reason={:?}",
                                        harness.binary,
                                        clear_cmd,
                                        head,
                                        context_reset_reason.as_deref().unwrap_or("")
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: interleaved {} before active queue head {:?}: {}",
                                    clear_cmd,
                                    head,
                                    context_reset_reason.as_deref().unwrap_or("fresh context required")
                                );
                                continue;
                            }
                            outcome => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_reset_failed harness={} cmd=\"{}\" outcome={}",
                                        harness.binary,
                                        clear_cmd,
                                        outcome.as_str()
                                    ),
                                );
                                continue;
                            }
                        }
                    }
                    IdleQueueContextResetDecision::SkipNoActiveHead
                    | IdleQueueContextResetDecision::SkipNotIdle
                    | IdleQueueContextResetDecision::SkipTurnActive
                    | IdleQueueContextResetDecision::SkipCurrentTransition
                    | IdleQueueContextResetDecision::SkipRouteSubmitInFlight
                    | IdleQueueContextResetDecision::SkipAlreadyResetHead
                    | IdleQueueContextResetDecision::SkipNoResetNeeded => {}
                }

                if supervisor_background_context_clear_enabled() {
                    // A visible `/clear`/`/new` draft is an input-ownership hazard even when its
                    // in-flight marker was lost across recycle or aged out. This recovery is
                    // disabled with background context clears because an unproven draft must not
                    // be converted into a supervisor-owned clear.
                    if let Some(head) = active_head.as_deref() {
                        let clear_cmd = harness.context_clear_command();
                        let clear_already_pending =
                            supervisor_pane_payload_already_pending(&shared, clear_cmd, &harness);
                        let resubmit_key = format!("orphan_context_clear:{head}");
                        if route_submit_in_flight
                            && drain_dispatch_dedup_skip(clear_already_pending)
                        {
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_orphan_context_clear_wait harness={} reason=route_submit_in_flight head={:?}",
                                    harness.binary, head
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_orphan_context_clear_wait file={} harness={} reason=route_submit_in_flight head_bytes={} head_sha256={} cmd={:?}",
                                    path.display(),
                                    harness.binary,
                                    head.len(),
                                    agent_doc_hash::content_hash(head),
                                    clear_cmd
                                ),
                            );
                            continue;
                        }
                        if !route_submit_in_flight
                            && idle_queue_pending_payload_needs_enter_resubmit(
                                &harness.binary,
                                clear_already_pending,
                                last_pending_enter_resubmitted.as_deref()
                                    == Some(resubmit_key.as_str()),
                            )
                        {
                            match idle_queue_resubmit_pending_payload(
                                &path,
                                &shared,
                                &harness,
                                "context_clear",
                                head,
                                clear_cmd,
                            ) {
                                AutoTriggerOutcome::Sent => {
                                    last_pending_enter_resubmitted = Some(resubmit_key);
                                    last_context_reset_head = active_head.clone();
                                    last_context_clear_at = Some(current_epoch_secs());
                                    context_reset_in_flight = true;
                                    awaiting_clear_settle = true;
                                    clear_settle_idle_ticks = 0;
                                    record_context_clear_in_flight_projection(
                                        &path,
                                        &shared,
                                        &harness,
                                        clear_cmd,
                                        CONTEXT_CLEAR_SOURCE_BACKGROUND_RESET,
                                        Some(head),
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_orphan_context_clear_resubmit harness={} reason=clear_draft_pending head={:?}",
                                            harness.binary, head
                                        ),
                                    );
                                    continue;
                                }
                                AutoTriggerOutcome::Cancelled => return,
                                outcome => {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_orphan_context_clear_resubmit_failed harness={} outcome={}",
                                            harness.binary,
                                            outcome.as_str()
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                    }
                }

                // `#qpausego`: an accepted `admin queue pause` records a durable
                // controller pause the dispatch RPC already honors, but the
                // idle-watch injects triggers straight into the pane and so
                // historically ignored it — a `go`-mode auto-queue kept
                // re-dispatching after an accepted pause (the unattended flood
                // this fixes). Defer the *supervisor's* auto-injection while the
                // pause is active. This suppresses only the unattended injector:
                // the attended in-session `/loop` continues to drain via
                // `queue_continuation_required` (a pause does not stall it).
                // `resume`/`drain` are not `paused` and do not block here.
                // Single-owner tie-break: only a fresh self-driving `/loop` lease
                // defers the supervisor. A supervisor-side failsafe dispatch is not
                // a competing owner; treating it as one stranded paused queues behind
                // the lease TTL after a clean closeout (#qstallguard Layer D).
                let drain_owner_lease =
                    agent_doc_queue_io::drain_owner::fresh_loop_drain_owner_lease(&file, current_epoch_secs());

                let mut paused_failsafe_active = false;
                if active_head.is_some() && queue_controller_paused {
                    // `#qstallguard` Layer C/D: pause throttles to the in-session
                    // loop owner; it does not disable the supervisor failsafe. Skip
                    // when a real `/loop` lease exists or nothing is drainable. With
                    // no loop owner and a drainable head, fall through to the normal
                    // dispatch decision. Do not claim a drain-owner lease here: a
                    // later gate may still skip dispatch, and a self-lease would
                    // suppress the next valid attempt until TTL expiry.
                    if paused_idle_watch_should_skip(
                        true,
                        active_head.is_some(),
                        drain_owner_lease.is_some(),
                    ) {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_drain_skipped harness={} reason=queue_control_paused file={}",
                                harness.binary,
                                path.display()
                            ),
                        );
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "queue_dispatch_skipped file={} harness={} reason=queue_control_paused",
                                path.display(),
                                harness.binary
                            ),
                        );
                        continue;
                    }
                    // fall through to the guarded drain decision below.
                    paused_failsafe_active = true;
                }

                // #sqedit-race Phase 2: a direct `queue prune-noise` / `queue
                // consume` is mid read-modify-write. Defer this tick so the
                // idle-watch never reads (and re-dispatches against) a torn
                // intermediate queue head. The lease is short-TTL, so this is a
                // brief yield: the edit settles and the next tick drains normally.
                if let Some(holder_pid) =
                    agent_doc_queue_io::queue_edit_owner::foreign_queue_edit_in_flight(&file)
                {
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_drain_skipped reason=queue_edit_in_flight holder_pid={holder_pid} (#sqedit-race)"
                        ),
                    );
                    continue;
                }
                match idle_queue_drain_decision_with_current_transition(IdleQueueDrainDecisionFacts {
                    clear_cooldown_active,
                    prompt_visible,
                    turn_active,
                    self_driving_loop_active: drain_owner_lease.is_some(),
                    route_submit_in_flight,
                    current_transition_pending,
                    active_head: active_head.as_deref(),
                    last_dispatched: last_dispatched.as_deref(),
                }) {
                    IdleQueueDrainDecision::Dispatch => {
                        // `#qstallguard` Layer B/C interaction: the supervisor idle-watch
                        // is itself continuing the drain (whether the queue is paused —
                        // the Layer C single-owner failsafe — or normal go-mode). That is
                        // NOT an in-session stall, so clear any continuation-pending
                        // projection the prior in-session closeout recorded. Otherwise the
                        // next drained agent's preflight would see the projection with no
                        // in-session drain lease and false-fire `queue_stall_detected` for
                        // a drain the supervisor is actively progressing.
                        let _ = agent_doc_controller_io::project_controller::clear_queue_drain_stall_continuation_pending_for_file(
                            Path::new(&file),
                            "supervisor_drain_progressed",
                        );
                        // `#qflood2`: hold the trigger until a just-sent `/clear`
                        // has settled, so it is never injected into the in-flight
                        // clear (the concatenated `/clear /agent-doc <FILE>`).
                        if drain_blocked_awaiting_clear_settle(
                            awaiting_clear_settle,
                            prompt_visible,
                            turn_active,
                            clear_settle_idle_ticks,
                            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                        ) {
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_drain_skipped harness={} reason=awaiting_clear_settle settle_ticks={}",
                                    harness.binary, clear_settle_idle_ticks
                                ),
                            );
                            continue;
                        }
                        // `#fbwire` / `#fullboundary` Phase 2: convergence-gated full
                        // boundary between queue items. Even though the pure drain
                        // decision says Dispatch (prompt visible, no active turn, lease
                        // owned), do NOT inject item N+1's trigger until item N proves a
                        // quiescent close — cycle committed AND the editor buffer
                        // converged to HEAD AND this doc's IPC inflight drained to 0 AND
                        // the actor idle. This caps inflight at ~1 and removes the
                        // concurrent-writer windows that produced the `content_ours` /
                        // `postcommit_worktree_check match=false` / `inflight=5
                        // send_failed` drift family. Composes with (does not replace) the
                        // drain-owner lease + clear-settle guards above.
                        {
                            let facts = gather_convergence_facts(
                                &path,
                                &shared,
                                convergence_gate_deferring_since,
                                CONVERGENCE_GATE_TIMEOUT_MS,
                            );
                            match agent_doc_document_realtime::convergence_gate::convergence_gate_decision(&facts) {
                                agent_doc_document_realtime::convergence_gate::ConvergenceGateDecision::Dispatch => {
                                    // `#j9ja` / `#optverify`: distinctive SUCCESS marker so the
                                    // `#fbwireverify` slow-ACK live test auto-verifies. Emit it
                                    // ONLY when the gate had been deferring (the meaningful
                                    // "the boundary converged after waiting on the editor/IPC"
                                    // signal) — a steady-state quiescent dispatch needs no marker,
                                    // keeping ops.log quiet on the common path.
                                    if let Some(since) = convergence_gate_deferring_since {
                                        agent_doc_ops_log_io::log_op(
                                            &path,
                                            &format!(
                                                "convergence_gate_converged_dispatch file={} waited_ms={} inflight={} editor_converged={} (#fbwire #j9ja)",
                                                path.display(),
                                                since.elapsed().as_millis(),
                                                facts.inflight,
                                                facts.editor_converged
                                            ),
                                        );
                                    }
                                    convergence_gate_deferring_since = None;
                                    convergence_gate_blocked_reported = false;
                                    // fall through to the existing dispatch path below
                                }
                                agent_doc_document_realtime::convergence_gate::ConvergenceGateDecision::Defer {
                                    unmet,
                                } => {
                                    if convergence_gate_deferring_since.is_none() {
                                        convergence_gate_deferring_since =
                                            Some(std::time::Instant::now());
                                        convergence_gate_blocked_reported = false;
                                    }
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_drain_deferred reason=convergence_gate unmet={} inflight={} elapsed_ms={} (#fbwire)",
                                            unmet.join(","),
                                            facts.inflight,
                                            facts.elapsed_ms
                                        ),
                                    );
                                    agent_doc_ops_log_io::log_op(
                                        &path,
                                        &format!(
                                            "convergence_gate_defer file={} unmet={} committed={} editor_converged={} inflight={} actor_idle={} elapsed_ms={} timeout_ms={} (#fbwire)",
                                            path.display(),
                                            unmet.join(","),
                                            facts.committed,
                                            facts.editor_converged,
                                            facts.inflight,
                                            facts.actor_idle,
                                            facts.elapsed_ms,
                                            facts.timeout_ms
                                        ),
                                    );
                                    continue;
                                }
                                agent_doc_document_realtime::convergence_gate::ConvergenceGateDecision::Blocked {
                                    unmet,
                                } => {
                                    if convergence_gate_deferring_since.is_none() {
                                        convergence_gate_deferring_since =
                                            Some(std::time::Instant::now());
                                    }
                                    if !convergence_gate_blocked_reported {
                                        record_convergence_gate_blocked(&path, &facts, &unmet);
                                        convergence_gate_blocked_reported = true;
                                    }
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_drain_blocked reason=convergence_gate_timeout unmet={} inflight={} elapsed_ms={} (#fbwire)",
                                            unmet.join(","),
                                            facts.inflight,
                                            facts.elapsed_ms
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                        let head = active_head.expect("dispatch implies an active head");
                        let trigger_command = harness.trigger_command(&file);
                        let drain_payload = idle_queue_drain_payload(&head, trigger_command);
                        let payload_kind = idle_queue_drain_payload_kind(&head);
                        let slash_command = idle_queue_head_slash_command(&head);
                    // `#qflood2` / `#30p6`: classify the pending payload and
                    // composer readiness from one capture. An unavailable or
                    // operator-owned composer is a defer state, never implicit
                    // permission to append text or claim a dispatch.
                    let payload_observation =
                        supervisor_pane_payload_observation(&shared, &drain_payload, &harness);
                    let payload_already_pending = payload_observation
                        .as_ref()
                        .map(|observation| observation.payload_already_pending);
                    let dispatch_ready = payload_observation
                        .as_ref()
                        .map(|observation| observation.dispatch_ready);
                    let resubmit_key = format!("drain:{head}");
                    let pending_action = idle_queue_pending_payload_action(
                        &harness.binary,
                        payload_already_pending,
                        dispatch_ready,
                        last_pending_enter_resubmitted.as_deref()
                            == Some(resubmit_key.as_str()),
                    );
                    let observation_key = format!(
                        "{}:{}:{}:{}:{}",
                        agent_doc_hash::content_hash(&head),
                        payload_observation
                            .as_ref()
                            .map(|observation| agent_doc_hash::content_hash(&observation.content))
                            .unwrap_or_else(|| "uncapturable".to_string()),
                        payload_already_pending
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        dispatch_ready
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        pending_action.as_str(),
                    );
                    if last_go_payload_observation_key.as_deref()
                        != Some(observation_key.as_str())
                    {
                        record_idle_queue_payload_observation(
                            &path,
                            &harness,
                            &head,
                            &drain_payload,
                            payload_observation.as_ref(),
                            pending_action,
                        );
                        last_go_payload_observation_key = Some(observation_key);
                    }
                    if pending_action == IdleQueuePendingPayloadAction::ResubmitEnter {
                            match idle_queue_resubmit_pending_payload(
                                &path,
                                &shared,
                                &harness,
                                payload_kind,
                                &head,
                                &drain_payload,
                            ) {
                                AutoTriggerOutcome::Sent => {
                                    if paused_failsafe_active {
                                        log_event(
                                            &mut session_log,
                                            &format!(
                                                "idle_queue_watch_paused_failsafe_drain harness={} reason=queue_paused_no_loop_owner action=resubmit file={}",
                                                harness.binary,
                                                path.display()
                                            ),
                                        );
                                        agent_doc_ops_log_io::log_op(
                                            &path,
                                            &format!(
                                                "queue_paused_failsafe_single_owner_drain file={} harness={} reason=no_in_session_loop_owner action=resubmit (#qstallguard Layer D)",
                                                path.display(),
                                                harness.binary
                                            ),
                                        );
                                    }
                                    last_pending_enter_resubmitted = Some(resubmit_key);
                                    last_dispatched = Some(head.clone());
                                    if slash_command
                                        .as_deref()
                                        .is_some_and(agent_doc_queue::queue_command::is_context_clear_command)
                                    {
                                        record_context_clear_in_flight_projection(
                                            &path,
                                            &shared,
                                            &harness,
                                            &drain_payload,
                                            CONTEXT_CLEAR_SOURCE_QUEUE_SLASH,
                                            Some(&head),
                                        );
                                    }
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_drain_resubmit harness={} reason=trigger_already_pending payload_kind={}",
                                            harness.binary, payload_kind
                                        ),
                                    );
                                    continue;
                                }
                                AutoTriggerOutcome::Cancelled => return,
                                outcome => {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_drain_resubmit_failed harness={} outcome={}",
                                            harness.binary,
                                            outcome.as_str()
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                    if pending_action == IdleQueuePendingPayloadAction::SkipProvenPending {
                        last_dispatched = Some(head.clone());
                        log_event(
                            &mut session_log,
                                &format!(
                                    "idle_queue_watch_drain_skipped harness={} reason=trigger_already_pending payload_kind={}",
                                    harness.binary, payload_kind
                                ),
                            );
                            agent_doc_ops_log_io::log_op(
                                &path,
                                &format!(
                                    "queue_dispatch_skipped file={} harness={} reason=trigger_already_pending payload_kind={}",
                                    path.display(),
                                    harness.binary,
                                    payload_kind
                                ),
                        );
                        continue;
                    }
                    if matches!(
                        pending_action,
                        IdleQueuePendingPayloadAction::DeferUnobservable
                            | IdleQueuePendingPayloadAction::DeferComposerOwned
                    ) {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_drain_deferred harness={} reason={} payload_kind={}",
                                harness.binary,
                                pending_action.as_str(),
                                payload_kind,
                            ),
                        );
                        continue;
                    }
                    debug_assert_eq!(
                        pending_action,
                        IdleQueuePendingPayloadAction::DispatchFresh
                    );
                    // A prior accepted write may have left the local projection
                    // behind even though this exact pane capture proves an empty
                    // composer. Clear only that matching projection; otherwise
                    // `auto_trigger_inject_command` would suppress the real retry
                    // as a duplicate and falsely return `Sent`.
                    if shared.clear_matching_prompt_dispatch_projection_for_retry(
                        "auto_trigger",
                        &drain_payload,
                    ) {
                        agent_doc_ops_log_io::log_op(
                            &path,
                            &format!(
                                "idle_queue_dispatch_projection_recovered file={} harness={} reason=same_capture_dispatch_ready payload_kind={} head_sha256={} payload_sha256={}",
                                path.display(),
                                harness.binary,
                                payload_kind,
                                agent_doc_hash::content_hash(&head),
                                agent_doc_hash::content_hash(&drain_payload),
                            ),
                        );
                    }
                    match auto_trigger_submit_queue_command(&shared, &stop, &drain_payload, &harness) {
                            AutoTriggerOutcome::Sent => {
                                if paused_failsafe_active {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_paused_failsafe_drain harness={} reason=queue_paused_no_loop_owner action=dispatch file={}",
                                            harness.binary,
                                            path.display()
                                        ),
                                    );
                                    agent_doc_ops_log_io::log_op(
                                        &path,
                                        &format!(
                                            "queue_paused_failsafe_single_owner_drain file={} harness={} reason=no_in_session_loop_owner action=dispatch (#qstallguard Layer D)",
                                            path.display(),
                                            harness.binary
                                        ),
                                    );
                                }
                                context_reset_in_flight = false;
                                if last_context_reset_head.as_deref() == Some(head.as_str())
                                    && !agent_doc_queue::queue_command::is_context_clear_command(
                                        &drain_payload,
                                    )
                                {
                                    log_between_turn_enqueue_delivery(
                                        &path,
                                        harness.context_clear_command(),
                                        &drain_payload,
                                    );
                                }
                                log_idle_queue_drain_submit(
                                    &path,
                                    &shared,
                                    &harness,
                                    payload_kind,
                                    &head,
                                    &drain_payload,
                                );
                                if let Some(command) = slash_command.as_deref() {
                                    let completed = complete_idle_queue_slash_command_head(
                                        &path,
                                        &head,
                                        command,
                                        &mut session_log,
                                    );
                                    if agent_doc_queue::queue_command::is_context_clear_command(command) {
                                        last_context_reset_head = Some(head.clone());
                                        last_context_clear_at = Some(current_epoch_secs());
                                        context_reset_in_flight = true;
                                        // `#qflood2`: a dispatched `/clear` head
                                        // must settle before the next head's
                                        // trigger fires, same as the context-reset
                                        // path, so they cannot concatenate.
                                        awaiting_clear_settle = true;
                                        clear_settle_idle_ticks = 0;
                                        record_context_clear_prompt_for_hooks(
                                            &shared,
                                            &path,
                                            &harness,
                                            command,
                                        );
                                        record_context_clear_in_flight_projection(
                                            &path,
                                            &shared,
                                            &harness,
                                            command,
                                            CONTEXT_CLEAR_SOURCE_QUEUE_SLASH,
                                            Some(&head),
                                        );
                                    }
                                    last_dispatched = if completed { None } else { Some(head) };
                                } else {
                                    last_dispatched = Some(head);
                                }
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_drain harness={} payload_kind={} submit_mode={}",
                                        harness.binary,
                                        payload_kind,
                                        agent_doc_supervisor::idle_watch::idle_queue_submit_mode(
                                            shared.inject_pane.is_some(),
                                            &harness.binary,
                                        )
                                    ),
                                );
                                // Already recorded in session_log above; only
                                // surface on stderr under verbose input diag so it
                                // does not bleed in front of a full-screen harness
                                // TUI (e.g. OpenCode). (#opencode-stdout-bleed)
                                if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                                    eprintln!(
                                        "[agent-doc] idle-queue watch: drained active queue head via {payload_kind}",
                                    );
                                }
                            }
                            AutoTriggerOutcome::Cancelled => return,
                            outcome => {
                                // Do NOT record the head: a failed inject must be
                                // retried on the next idle tick, not suppressed.
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_drain_failed harness={} outcome={}",
                                        harness.binary,
                                        outcome.as_str()
                                    ),
                                );
                            }
                        }
                    }
                    IdleQueueDrainDecision::SkipNoActiveHead => {
                        // Head drained (or never present): clear dedup so a later
                        // re-enqueue of the same prompt text fires again.
                        last_dispatched = None;
                        last_pending_enter_resubmitted = None;
                    }
                    IdleQueueDrainDecision::SkipSelfDrivingLoopOwner => {
                        // The Claude Code `/loop` owns the drain — proof the
                        // supervisor deferred (the live-verify signal for #kp5z).
                        if let Some(lease) = &drain_owner_lease {
                            let lease_age =
                                current_epoch_secs().saturating_sub(lease.heartbeat_secs);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner owner={} lease_age={}",
                                    lease.owner, lease_age
                                ),
                            );
                        }
                    }
                    IdleQueueDrainDecision::SkipNotIdle
                    | IdleQueueDrainDecision::SkipTurnActive
                    | IdleQueueDrainDecision::SkipCurrentTransition
                    | IdleQueueDrainDecision::SkipRouteSubmitInFlight
                    | IdleQueueDrainDecision::SkipClearCooldown
                    | IdleQueueDrainDecision::SkipAlreadyDispatched => {}
                }
            }
        })
        .expect("spawn idle-queue watch thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#idlewatchrevisiongate`: the memo answers only for the revision it was
    /// built from.
    ///
    /// `CurrentRevision`'s doc comment already promised this ("the idle
    /// supervisor compares this value before asking the relay to materialize the
    /// canonical markdown"), and the RPC and `PartialEq` both shipped — the call
    /// site just never compared. This pins the comparison itself: same revision
    /// serves the cached head, any moved field forces a fresh read.
    #[test]
    fn the_queue_head_memo_only_answers_for_the_revision_it_was_built_from() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("memo.md");
        std::fs::write(&doc, "# memo\n").unwrap();

        let revision = |sv: &[u8], live: usize, converged: bool| {
            agent_doc_crdt_relay_io::CurrentRevision::Current {
                state_vector: sv.to_vec(),
                live_editors: live,
                delivery_converged: converged,
            }
        };
        let observed = QueueHeadObservation::Observed {
            head: Some("do [#alpha]".to_string()),
            transition: IdleQueueTransition::Converged,
        };

        let base = revision(b"sv-1", 1, true);
        memoize_queue_head(&doc, Some(base.clone()), &observed);
        assert_eq!(
            memoized_queue_head(&doc, &base),
            Some(observed.clone()),
            "an unchanged revision must be served from the memo, not a full-text read"
        );

        // Every field of the revision is part of the identity: the state vector
        // is the text, and `live_editors` / `delivery_converged` change the
        // transition the drain acts on even when the text has not moved.
        for moved in [
            revision(b"sv-2", 1, true),
            revision(b"sv-1", 2, true),
            revision(b"sv-1", 1, false),
        ] {
            assert_eq!(
                memoized_queue_head(&doc, &moved),
                None,
                "a moved revision must force a fresh read: {moved:?}"
            );
        }

        // Without a cheap revision to key on, the entry is dropped rather than
        // left where a later probe could match it against a revision this
        // observation never had.
        memoize_queue_head(&doc, None, &observed);
        assert_eq!(memoized_queue_head(&doc, &base), None);
    }

    #[test]
    fn idle_queue_transition_maps_delivery_convergence() {
        assert_eq!(
            IdleQueueTransition::from_converged(true),
            IdleQueueTransition::Converged
        );
        assert_eq!(
            IdleQueueTransition::from_converged(false),
            IdleQueueTransition::Pending
        );
    }

    #[test]
    fn disk_queue_head_reports_converged_transition() {
        // `#recycletransitionwedge`: on the disk-authority path there is no editor
        // delivery in flight, so the drain must never be told a transition is
        // pending — that is what used to skip the drain on every tick.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("d.md");
        std::fs::write(&doc, "---\nqueue_active: true\n---\n\n").unwrap();
        match idle_watch_disk_queue_head(&doc) {
            QueueHeadObservation::Observed { transition, .. } => {
                assert_eq!(transition, IdleQueueTransition::Converged);
            }
            QueueHeadObservation::AuthorityUnavailable => {
                panic!("disk authority must always observe")
            }
        }
    }

    #[test]
    fn idle_watch_transition_never_reads_the_process_local_relay() {
        // The regression guard (`#recycletransitionwedge`). The supervisor process
        // never hosts a CRDT hub — only the project controller registers replicas,
        // and the supervisor IPC protocol rejects the replica methods outright. So
        // any `current_text_for_file*` read from this module resolves against an
        // always-empty local `hub_registry()`; once durable liveness says an editor
        // is attached that miss reads as `EditorAttachedMissingReplica` forever,
        // pinning `current_transition_pending` true and wedging the queue drain
        // with no self-heal (observed live after an execve self-recycle).
        //
        // Transition state must be resolved through the controller instead. Assert
        // the source has no local-relay read left in it, so a future edit cannot
        // silently reintroduce the wedge.
        // Built from fragments so this guard never matches its own source text.
        let needle = ["agent_doc_crdt_relay_io", "::", "current_text", "_for_file"].concat();
        let offenders: Vec<&str> = include_str!("idle_watch.rs")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| line.contains(needle.as_str()))
            .collect();
        assert!(
            offenders.is_empty(),
            "idle_watch must resolve document text/transition state through the \
             project controller, never the supervisor-local relay registry; found: {offenders:#?}"
        );
    }

    #[test]
    fn idle_watch_fast_path_requires_observed_ready_nonurgent_state() {
        assert!(idle_watch_fast_path_can_sleep(true, true, false, false));
        assert!(!idle_watch_fast_path_can_sleep(false, true, false, false));
        assert!(!idle_watch_fast_path_can_sleep(true, false, false, false));
        assert!(!idle_watch_fast_path_can_sleep(true, true, true, false));
        assert!(!idle_watch_fast_path_can_sleep(true, true, false, true));
    }

    #[test]
    fn stale_supervisor_recycles_at_any_handler_safe_turn_stage() {
        assert!(stale_recycle_safe_checkpoint(true, 0));
        assert!(!stale_recycle_safe_checkpoint(true, 1));
        assert!(!stale_recycle_safe_checkpoint(false, 0));
    }

    /// `#idlerevisionreactive`. This test previously asserted that a *failed*
    /// probe invalidates the projection, on fail-safe grounds: better to redo the
    /// expensive work than to miss a change.
    ///
    /// The intent was right and the mechanism was wrong. Missing a change is
    /// already covered — [`IDLE_WATCH_FULL_RECONCILE_INTERVAL`] reruns the
    /// authoritative projection every 60s regardless of what the probe said. So
    /// the invalidate-on-failure rule bought no safety that was not already
    /// there, and it cost a feedback loop: an unanswerable probe means the
    /// controller is struggling, and the response was to issue the expensive
    /// controller RPCs every 500ms instead of every 60s, up to 120x the intended
    /// load, aimed at the process that was already failing to keep up.
    #[test]
    fn an_unanswerable_probe_does_not_invalidate_the_full_projection() {
        let first = IdleWatchDocumentRevision::Disk {
            len: 42,
            modified_nanos: 7,
            controller_observation_suppressed: false,
        };
        let changed = IdleWatchDocumentRevision::Disk {
            len: 43,
            modified_nanos: 8,
            controller_observation_suppressed: false,
        };
        let fingerprint = |revision: &IdleWatchDocumentRevision| format!("{revision:?}");

        let state = IdleRevisionState::new();
        state.observe(RevisionObservation::observed(fingerprint(&first)));
        assert!(state.projection_stale(), "a first observation is a change");

        state.observe(RevisionObservation::observed(fingerprint(&first)));
        assert!(
            !state.projection_stale(),
            "an equal revision is not a change"
        );

        state.observe(RevisionObservation::observed(fingerprint(&changed)));
        assert!(state.projection_stale(), "a different revision is a change");

        state.observe(RevisionObservation::Unresolved);
        assert!(
            !state.projection_stale(),
            "an unanswered probe must not invalidate: the 60s full reconcile is \
             the fail-safe, and escalating here feeds the wedge instead"
        );

        state.observe(RevisionObservation::Suppressed);
        assert!(
            !state.projection_stale(),
            "a cooldown-suppressed probe must not invalidate the path the cooldown \
             exists to avoid"
        );

        state.observe(RevisionObservation::observed(fingerprint(&first)));
        assert!(
            state.projection_stale(),
            "the baseline survives the unanswered stretch, so a real change after \
             it is still seen"
        );
    }

    #[test]
    fn attached_editor_never_allows_disk_queue_authority() {
        assert!(disk_queue_authority_allowed(false));
        assert!(!disk_queue_authority_allowed(true));
    }

    #[test]
    fn idle_watch_repairs_only_explicit_missing_or_sync_pending_replicas() {
        assert!(idle_watch_replica_recovery_needed(&Ok(Some(
            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        ))));
        assert!(idle_watch_replica_recovery_needed(&Ok(Some(
            agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
        ))));
        assert!(!idle_watch_replica_recovery_needed(&Ok(Some(
            agent_doc_crdt_relay_io::CurrentText::Detached
        ))));
        assert!(!idle_watch_replica_recovery_needed(&Ok(None)));
        assert!(!idle_watch_replica_recovery_needed(&Err(anyhow::anyhow!(
            "controller unavailable"
        ))));
    }

    fn context_clear_projection_with_source(
        source: Option<&str>,
    ) -> agent_doc_state_backbone::QueueContextClearProjection {
        agent_doc_state_backbone::QueueContextClearProjection {
            phase: agent_doc_state_backbone::QueueContextClearPhase::InFlight,
            file: "plan.md".to_string(),
            target: "%1".to_string(),
            harness: "codex".to_string(),
            command: "/clear".to_string(),
            source: source.map(str::to_string),
            head_sha256: None,
            head_bytes: None,
            clear_epoch: 1,
            marked_secs: 42,
        }
    }

    #[test]
    fn context_clear_projection_source_allows_only_operator_and_queue_actions() {
        assert!(context_clear_projection_source_allows_supervisor_action(
            &context_clear_projection_with_source(Some(CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED))
        ));
        assert!(context_clear_projection_source_allows_supervisor_action(
            &context_clear_projection_with_source(Some(CONTEXT_CLEAR_SOURCE_QUEUE_SLASH))
        ));
        assert!(!context_clear_projection_source_allows_supervisor_action(
            &context_clear_projection_with_source(Some(CONTEXT_CLEAR_SOURCE_BACKGROUND_RESET))
        ));
        assert!(!context_clear_projection_source_allows_supervisor_action(
            &context_clear_projection_with_source(None)
        ));
        assert!(!supervisor_background_context_clear_enabled());
    }

    #[test]
    fn paused_queue_head_uses_saved_document_drainability() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(
            &doc,
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:queue preset=\"#spec-test-commit-push\" go -->\n",
                "- 🚧 [#missing]\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        assert_eq!(
            idle_watch_paused_queue_head(&doc),
            QueueHeadObservation::Observed {
                head: None,
                transition: IdleQueueTransition::Converged
            },
            "undefined backlog ids in a paused queue must not force a live editor probe"
        );

        std::fs::write(
            &doc,
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#a] saved backlog work\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:queue preset=\"#spec-test-commit-push\" go -->\n",
                "- 🚧 [#a]\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        assert_eq!(
            idle_watch_paused_queue_head(&doc),
            QueueHeadObservation::Observed {
                head: Some("a".to_string()),
                transition: IdleQueueTransition::Converged
            }
        );
    }

    #[test]
    fn open_agent_doc_cycle_defers_self_recycle_committed_cycle_allows_it() {
        // `#midturn-recycle-resume` regression: a recycle in the preflight→finalize
        // window must NOT fire (it would `execve` mid-cycle, sever the in-flight IPC
        // ack connection, and drive the next finalize into
        // `live_prompt_drift_after_preflight`). This exercises the EXACT `cycle_open`
        // expression the idle-watch computes from live `cycle_state` and feeds into
        // `supervisor_recycle_action`, proving the wiring — not just the pure policy.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "# plan\n").unwrap();

        // Open a cycle on disk (preflight taken, finalize not committed). The
        // production `cycle_open` reads this via `cycle_state::load(..).is_open()`.
        let opened =
            agent_doc_cycle_state_io::start_preflight(&file, Some("# plan\n"), Some("# plan\n"))
                .unwrap();
        assert!(opened.is_open(), "fresh preflight cycle must be open");

        let cycle_open_while_open = agent_doc_cycle_state_io::load(&file)
            .ok()
            .flatten()
            .map(|state| state.is_open())
            .unwrap_or(false)
            || test_cycle_open_from_inflight(0);
        assert!(
            cycle_open_while_open,
            "an open agent-doc cycle on disk must compute cycle_open=true"
        );

        // Stale binary + auto-recycle + a head pending at a harness turn boundary —
        // the exact #supautoinstall self-recycle inputs — must DEFER while the cycle
        // is open.
        assert_eq!(
            supervisor_recycle_action(
                /* stale */ true,
                /* auto_recycle */ true,
                /* turn_boundary */ true,
                /* head_pending */ true,
                /* explicit_admin */ false,
                /* write_wedged */ false,
                /* editor_delivery_stale */ false,
                /* reexec_failed */ false,
                cycle_open_while_open,
            ),
            SupervisorRecycleAction::DeferCycleOpen,
            "an open cycle must defer the execve recycle so it cannot sever the live finalize"
        );

        // A stale editor delivery worker is different: the open cycle is waiting
        // for that worker to ACK, so deferring recycle on the cycle would be a
        // circular wait. At a handler-safe checkpoint the exact idle-watch wiring
        // must recycle immediately while the capture-backed cycle remains durable.
        assert_eq!(
            supervisor_recycle_action(
                /* stale */ false,
                /* auto_recycle */ false,
                /* turn_boundary */ false,
                /* head_pending */ false,
                /* explicit_admin */ true,
                /* write_wedged */ false,
                /* editor_delivery_stale */ true,
                /* reexec_failed */ false,
                cycle_open_while_open,
            ),
            SupervisorRecycleAction::RecycleImmediate,
            "stale editor delivery must break the open-cycle circular wait at a safe checkpoint"
        );

        // Commit the cycle: now it is no longer open and (in this single-threaded
        // test) no IPC handler is in flight, so cycle_open is false and the same
        // inputs recycle — the deferred recycle fires at the true quiescent boundary.
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &file,
            "committed",
            Some("# plan\n"),
            Some("# plan\n"),
        )
        .unwrap();
        let cycle_open_after_commit = agent_doc_cycle_state_io::load(&file)
            .ok()
            .flatten()
            .map(|state| state.is_open())
            .unwrap_or(false)
            || test_cycle_open_from_inflight(0);
        assert!(
            !cycle_open_after_commit,
            "a committed cycle with no in-flight IPC must compute cycle_open=false"
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                true,
                true,
                false,
                false,
                false,
                false,
                cycle_open_after_commit,
            ),
            SupervisorRecycleAction::RecycleImmediate,
            "once the cycle commits and IPC drains, the deferred recycle fires"
        );
    }

    fn test_cycle_open_from_inflight(inflight: u64) -> bool {
        inflight > 0
    }

    #[test]
    fn cycle_open_expression_treats_inflight_ipc_as_open() {
        assert!(test_cycle_open_from_inflight(1));
        assert!(!test_cycle_open_from_inflight(0));
    }

    #[test]
    fn pending_payload_enter_resubmit_is_scoped_and_one_shot() {
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(true),
            false
        ));
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "claude",
            Some(true),
            false
        ));
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "opencode",
            Some(true),
            false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(false),
            false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex", None, false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(true),
            true
        ));
    }

    #[test]
    fn pending_payload_action_fails_closed_without_same_capture_readiness() {
        assert_eq!(
            idle_queue_pending_payload_action("codex", None, None, false),
            IdleQueuePendingPayloadAction::DeferUnobservable
        );
        assert_eq!(
            idle_queue_pending_payload_action("codex", Some(false), Some(false), false),
            IdleQueuePendingPayloadAction::DeferComposerOwned
        );
        assert_eq!(
            idle_queue_pending_payload_action("codex", Some(false), Some(true), false),
            IdleQueuePendingPayloadAction::DispatchFresh
        );
    }

    #[test]
    fn pending_payload_action_resubmits_once_then_keeps_proven_draft_owned() {
        assert_eq!(
            idle_queue_pending_payload_action("codex", Some(true), Some(false), false),
            IdleQueuePendingPayloadAction::ResubmitEnter
        );
        assert_eq!(
            idle_queue_pending_payload_action("codex", Some(true), Some(false), true),
            IdleQueuePendingPayloadAction::SkipProvenPending
        );
    }

    #[test]
    fn context_reset_in_flight_dedupes_active_head_edits() {
        let edited_head = Some("operator is still typing the active queue head");
        let previous_head = Some("operator is still typing");

        assert_eq!(
            context_reset_dedupe_head(edited_head, previous_head, true),
            edited_head
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                edited_head,
                context_reset_dedupe_head(edited_head, previous_head, true),
                true,
            ),
            IdleQueueContextResetDecision::SkipAlreadyResetHead
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                edited_head,
                context_reset_dedupe_head(edited_head, previous_head, false),
                true,
            ),
            IdleQueueContextResetDecision::Reset
        );
    }

    #[test]
    fn codex_opted_in_context_reset_is_suppressed_for_ordinary_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        std::fs::write(&doc, "doc\n").unwrap();
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();

        let head = "ordinary queue head";
        let reason = agent_doc_codex_hook_io::codex_queue_context_reset_reason(&doc, None)
            .unwrap()
            .expect("opted-in compaction should require fresh Codex context");
        assert!(
            reason.contains("exchange was compacted"),
            "unexpected reset reason: {reason}"
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some(head),
                None,
                supervisor_background_context_clear_enabled() && !reason.is_empty(),
            ),
            IdleQueueContextResetDecision::SkipNoResetNeeded
        );

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("idle_queue_watch_context_reset"),
            "ordinary opted-in reset reason must not send automatic /clear:\n{ops_log}"
        );
    }

    #[test]
    fn clean_session_head_drains_without_background_clear() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(&doc, "doc\n").unwrap();

        let head = "do [#cleandrainsup-agent]";
        let harness = agent_doc_harness::HarnessConfig::codex();
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            None,
            "codex",
            None,
            None,
            Some("%25".to_string()),
        );

        assert!(clean_session_head_forces_context_reset(true, false,));
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some(head),
                None,
                supervisor_background_context_clear_enabled(),
            ),
            IdleQueueContextResetDecision::SkipNoResetNeeded
        );

        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, Some(head), None,),
            IdleQueueDrainDecision::Dispatch
        );
        log_idle_queue_drain_submit(
            &doc,
            &shared,
            &harness,
            "trigger",
            head,
            &format!("agent-doc {}", doc.display()),
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !ops_log.contains("idle_queue_watch_context_reset"),
            "clean-session/focused-cycle tags must not send automatic /clear: {ops_log}"
        );
        assert!(ops_log.contains("idle_queue_watch_drain"));
        assert!(ops_log.contains("proof=go_drain_dispatch"));
        assert!(ops_log.contains("head_sha256="));
    }

    #[test]
    fn focused_cycle_head_drains_without_background_clear() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            &doc,
            "## Queue\n\n<!-- agent:queue go -->\n- do [#focus]\n<!-- /agent:queue -->\n\n## Pending\n\n<!-- agent:backlog -->\n- [ ] [#focus] [focused-cycle] dedicated proof\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let head = "do [#focus]";
        let reason =
            forced_context_reset_reason_for_head(&doc, head).expect("focused-cycle reason");
        assert_eq!(reason, FOCUSED_CYCLE_CONTEXT_RESET_REASON);

        let harness = agent_doc_harness::HarnessConfig::codex();
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            None,
            "codex",
            None,
            None,
            Some("%25".to_string()),
        );
        log_idle_queue_drain_submit(
            &doc,
            &shared,
            &harness,
            "trigger",
            head,
            &format!("agent-doc {}", doc.display()),
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !ops_log.contains("idle_queue_watch_context_reset"),
            "focused-cycle must not send automatic /clear: {ops_log}"
        );
        assert!(ops_log.contains("idle_queue_watch_drain"));
        assert!(ops_log.contains("proof=go_drain_dispatch"));
        assert!(ops_log.contains("head_sha256="));
    }

    // ----- `#fbwire` / `#fullboundary` Phase 2: convergence-gated boundary -----

    use agent_doc_document_realtime::convergence_gate::{
        ConvergenceFacts, ConvergenceGateDecision, convergence_gate_decision,
    };

    fn quiescent_facts() -> ConvergenceFacts {
        ConvergenceFacts {
            committed: true,
            editor_converged: true,
            inflight: 0,
            actor_idle: true,
            elapsed_ms: 0,
            timeout_ms: CONVERGENCE_GATE_TIMEOUT_MS,
        }
    }

    #[test]
    fn editor_buffer_converged_to_head_defaults_true_for_non_git_doc() {
        // A document outside any git repo must never wedge the drain: `show_head`
        // fails to resolve a git root, so convergence reports `true` and the gate
        // falls through to dispatch (editorless CLI sessions live here).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "hello\n").unwrap();
        assert!(editor_buffer_converged_to_head(&doc));
    }

    #[test]
    fn editor_buffer_converged_to_head_tracks_disk_vs_head() {
        // In a real repo the fact is the `#pcwc` postcommit ground truth: disk ==
        // HEAD ⇒ converged; an uncommitted disk edit ⇒ not converged.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .current_dir(repo)
                .args(&args)
                .output()
                .unwrap();
        }
        let doc = repo.join("task.md");
        std::fs::write(&doc, "committed body\n").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "--", "task.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "c", "--no-verify"])
            .output()
            .unwrap();
        // Disk == HEAD → converged.
        assert!(editor_buffer_converged_to_head(&doc));
        // Diverge the working tree → not converged (a wedge candidate).
        std::fs::write(&doc, "committed body\nuncommitted editor drift\n").unwrap();
        assert!(!editor_buffer_converged_to_head(&doc));
    }

    #[test]
    fn editor_buffer_converged_to_head_uses_live_current_document() {
        // Disk still equals HEAD, but the editor-visible buffer is ahead. The
        // convergence gate must see the current document authority, not only the
        // detached disk replica.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join(".agent-doc")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .current_dir(repo)
                .args(&args)
                .output()
                .unwrap();
        }
        let doc = repo.join("task.md");
        let committed = "committed body\n";
        std::fs::write(&doc, committed).unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "--", "task.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "c", "--no-verify"])
            .output()
            .unwrap();
        assert!(editor_buffer_converged_to_head(&doc));

        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            "idle-watch-test-editor",
            "committed body\nunsaved editor queue head\n",
        );

        assert!(
            !editor_buffer_converged_to_head(&doc),
            "unsaved editor-current content must block HEAD convergence even while disk still equals HEAD"
        );
    }

    #[test]
    fn convergence_gate_slow_ack_drain_never_dispatches_while_inflight_positive() {
        // SimWorld acceptance: a slow editor ACK keeps `inflight` positive for
        // several ticks; item N+1 must keep deferring (never dispatch, so inflight
        // never piles past ~1) until the ACK drains it to 0, then dispatch.
        let mut decisions = Vec::new();
        for inflight in [3_u64, 2, 1] {
            let facts = ConvergenceFacts {
                inflight,
                elapsed_ms: 1_000,
                ..quiescent_facts()
            };
            decisions.push(convergence_gate_decision(&facts));
        }
        // Drained — quiescent.
        decisions.push(convergence_gate_decision(&quiescent_facts()));

        for d in &decisions[..3] {
            match d {
                ConvergenceGateDecision::Defer { unmet } => {
                    assert!(unmet.contains(
                        &agent_doc_document_realtime::convergence_gate::proof::INFLIGHT_DRAINED
                    ));
                }
                other => panic!("inflight>0 must Defer, not dispatch: {other:?}"),
            }
        }
        assert!(matches!(decisions[3], ConvergenceGateDecision::Dispatch));
    }

    #[test]
    fn convergence_gate_wedged_editor_blocks_after_timeout() {
        // SimWorld acceptance: a wedged editor never ACKs (editor_converged=false,
        // inflight pinned). Within the timeout the gate Defers; once elapsed crosses
        // the bound it blocks dispatch naming the unmet proofs.
        let within = ConvergenceFacts {
            editor_converged: false,
            inflight: 5,
            elapsed_ms: CONVERGENCE_GATE_TIMEOUT_MS - 1,
            ..quiescent_facts()
        };
        assert!(matches!(
            convergence_gate_decision(&within),
            ConvergenceGateDecision::Defer { .. }
        ));
        let wedged = ConvergenceFacts {
            elapsed_ms: CONVERGENCE_GATE_TIMEOUT_MS,
            ..within
        };
        match convergence_gate_decision(&wedged) {
            ConvergenceGateDecision::Blocked { unmet } => {
                assert!(unmet.contains(
                    &agent_doc_document_realtime::convergence_gate::proof::EDITOR_CONVERGED
                ));
                assert!(unmet.contains(
                    &agent_doc_document_realtime::convergence_gate::proof::INFLIGHT_DRAINED
                ));
            }
            other => panic!("wedged editor past timeout must block dispatch: {other:?}"),
        }
    }

    #[test]
    fn record_convergence_gate_blocked_emits_loud_error_and_loadable_playback() {
        // The blocked boundary must be loud + fully diagnosable: an ERROR-level ops.log line
        // referencing a persisted, loadable playback artifact under
        // `.agent-doc/playback/<hash>/<cycle>.json`.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(&doc, "body\n").unwrap();
        let facts = ConvergenceFacts {
            committed: true,
            editor_converged: false,
            inflight: 5,
            actor_idle: true,
            elapsed_ms: CONVERGENCE_GATE_TIMEOUT_MS,
            timeout_ms: CONVERGENCE_GATE_TIMEOUT_MS,
        };
        let unmet = vec![
            agent_doc_document_realtime::convergence_gate::proof::EDITOR_CONVERGED,
            agent_doc_document_realtime::convergence_gate::proof::INFLIGHT_DRAINED,
        ];
        record_convergence_gate_blocked(&doc, &facts, &unmet);

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("convergence_gate_blocked severity=error"),
            "must emit the loud ERROR line: {ops_log}"
        );
        assert!(
            ops_log.contains("action=fail_closed"),
            "blocked boundary must not be reported as a disk fallback: {ops_log}"
        );
        assert!(ops_log.contains("playback="), "must reference the artifact");

        // The referenced artifact must exist and deserialize back into a playback.
        let playback_path = ops_log
            .lines()
            .find_map(|l| l.split("playback=").nth(1))
            .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
            .filter(|s| s != "unwritten")
            .expect("ERROR line must name a written playback path");
        let raw = std::fs::read_to_string(&playback_path).unwrap();
        let loaded: agent_doc_workflow_io::convergence_playback::ConvergencePlayback =
            serde_json::from_str(&raw).expect("playback artifact must be loadable");
        assert_eq!(loaded.inflight, 5);
        assert!(loaded.unmet_proofs.iter().any(|p| p == "editor_converged"));
    }

    #[test]
    fn convergence_facts_use_committed_ledger_projection() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(&doc, "body\n").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body\n",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some("body\n"), Some("body\n")).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some("body\n"),
            Some("body\n"),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some("body\n"),
            Some("body\n"),
        )
        .unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed
        );

        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            None,
            "codex",
            None,
            None,
            Some("%25".to_string()),
        );
        let facts = gather_convergence_facts(&doc, &shared, None, CONVERGENCE_GATE_TIMEOUT_MS);
        assert!(
            facts.committed,
            "stale open JSON must not make the convergence gate wait after lazily commit"
        );
    }
}
