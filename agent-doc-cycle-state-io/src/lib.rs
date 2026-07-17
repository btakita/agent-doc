//! # Module: cycle_state
//!
//! ## Spec
//! - Persists per-document cycle state under `.agent-doc/state/cycles/<doc-hash>.json`.
//! - Tracks the exact phase of the current or most recent response cycle:
//!   `preflight_started` → `response_captured` → `write_applied` → `committed`.
//!   A stale `preflight_started` cycle with no response artifact may become
//!   `abandoned` so a later preflight can start a fresh cycle for the same
//!   unresolved prompt.
//! - Stores cycle-scoped snapshot/file content hashes so callers can reason
//!   about exact cycle state instead of inferring from file-size drift or only
//!   the last `ops.log` line.
//! - Stores the preflight baseline path, prompt targets, queue head identity,
//!   pending-operation facts, and response capture hash as one durable turn
//!   checkpoint so restart/recycle paths can resume or fail closed from disk.
//! - `start_preflight()` opens a new cycle for a document and overwrites any
//!   prior committed state for that document.
//! - `mark_response_captured()` advances the open cycle to `response_captured`
//!   once the final parsed response has been durably stored.
//! - `mark_write_applied()` advances the open cycle to `write_applied` (or
//!   creates a synthetic cycle if a write lands without a prior preflight).
//! - `mark_committed()` advances the cycle to `committed` (or creates a
//!   synthetic committed cycle if commit happens without a closable open state,
//!   including after a stale preflight was abandoned).
//! - Lower-rank bookkeeping must never mutate an already-committed or abandoned
//!   cycle; duplicate terminal bookkeeping stays idempotent for already-committed
//!   cycles.
//! - Phase transitions are accepted through `cycle_state_machine`; the sidecar
//!   remains a compatibility crash-recovery projection emitted after that
//!   transition table accepts an event, and accepted transitions also append
//!   typed closeout facts into the state backbone.
//! - `load()` returns the current persisted JSON compatibility projection when
//!   present.
//! - `load_closeout_projection()` returns the state-backbone closeout projection
//!   when lazily facts have been recorded for the document.
//!
//! ## Agentic Contracts
//! - State is per-document, never global across the repo.
//! - Writes are deterministic JSON file replacements.
//! - Missing project root or state file returns `Ok(None)`.
//! - `is_open()` is true for any phase except `Committed` or `Abandoned`.
//!
//! ## Evals
//! - `start_preflight_persists_open_cycle`
//! - `mark_response_captured_sets_capture_metadata`
//! - `mark_write_applied_advances_existing_cycle`
//! - `mark_committed_closes_cycle`
//! - `mark_write_applied_creates_synthetic_cycle_when_missing`

use agent_doc_document::transient_markers::replay_content_hash;
use agent_doc_element_backlog::backlog::normalize_pending_id;
use agent_doc_turn::cycle_policy::{
    is_stable_commit_event, normalize_checkpoint_task_id, normalize_checkpoint_text_list,
};
use agent_doc_turn::{CycleEvent, CyclePhase, CyclePhaseMachine};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod pipeline_frontmatter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BacklogTargetRequirement {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baseline_item_ids: Vec<String>,
}

/// `#semmerge-ack-turn` (document_cell_merge Phase 4): a node-keyed acknowledgement
/// that a node-disjoint semantic merge could NOT apply the agent's change
/// verbatim — the operator deleted an agent-edited node, overrode the same node,
/// or revived an agent-deleted node, and the operator value won in `merged_doc`.
/// The agent's content is never silently discarded: the next cycle surfaces these
/// so the agent emits an exchange turn acknowledging the non-applied change.
///
/// `reason` is the stable [`agent_doc_merge::document_cell_merge::AckReason`]
/// token (see [`AckReason::token`](agent_doc_merge::document_cell_merge::AckReason::token)).
/// `recorded_cycle_id` is the cycle whose convergence recorded the ack (forensic
/// info). `surfaced` drives the one-cycle lifecycle: [`start_preflight_with_task`]
/// carries forward only un-surfaced acks and marks them surfaced, so each ack
/// reaches the agent exactly once and drops the cycle after. The flag is used
/// instead of comparing cycle ids because `cycle_id` is millisecond-derived and
/// two cycles can collide within the same millisecond.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingSemanticMergeAck {
    pub component: String,
    pub id: String,
    pub reason: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_cycle_id: Option<String>,
    /// True once this ack has been carried into a cycle for the agent to surface.
    /// Set by [`start_preflight_with_task`] when it carries the ack forward.
    #[serde(default)]
    pub surfaced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockedCloseout {
    pub kind: String,
    pub reason: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CycleState {
    pub cycle_id: String,
    pub file: String,
    pub phase: CyclePhase,
    pub last_event: String,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_file_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    #[serde(default)]
    pub had_pending_mutations: bool,
    #[serde(default)]
    pub requires_backlog_capture: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_backlog_targets: Vec<BacklogTargetRequirement>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub required_explicit_backlog_item_count: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub required_plan_reference_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// `#midturn-recycle-resume` Phase B: latched true once a fresh supervisor boot
    /// has re-dispatched this exact open cycle's interrupted turn from the
    /// `#durablerecycle` checkpoint (the harness child died across the `execve`
    /// recycle). The idempotency guard: a SECOND boot reading the same still-open
    /// checkpoint must not re-dispatch the turn again (it would double-run it). A
    /// committed cycle is never open so it never re-dispatches regardless of this
    /// flag; this flag covers the rarer case where two boots both observe the same
    /// open-but-child-dead checkpoint before the re-dispatched turn commits.
    #[serde(default, skip_serializing_if = "is_false")]
    pub recycle_resume_consumed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_done_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_kept_open_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaped_pending_ids: Vec<String>,
    /// `#do-id-closeout-open-backlog`: tracked-work ids named by an explicit
    /// `do [#id]` / `do #id` prompt directive that were still open in the live
    /// `agent:backlog` at preflight time. A successful closeout must end each of
    /// these with an explicit lifecycle outcome (`--done`, `--pending-gate`, an
    /// explicit kept-open edit, or reap); otherwise `session-check` fails closed
    /// so a directive cannot clear the queue while leaving its target `[ ]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expect_done_or_gate_ids: Vec<String>,
    /// `#blocked-closeout-followup-capture`: tracked ids moved to the
    /// review/gated component this cycle via `--pending-gate`. A gate removes an
    /// id from active `agent:backlog`, so when the response also signals the
    /// work is blocked / still needs future action, `session-check` requires a
    /// captured follow-up (kept-open edit, new backlog item, or explicit
    /// no-follow-up justification) before clean closeout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_gated_ids: Vec<String>,
    /// `#blocked-closeout-followup-capture`: true when this cycle added at least
    /// one follow-up item via any `--pending-add*` primitive (active backlog or
    /// gated). Satisfies the blocked-closeout follow-up requirement.
    #[serde(default)]
    pub pending_added_this_cycle: bool,
    /// `#opsproof-samecycle-add`: ids of tracked-work items added THIS cycle via
    /// `--pending-add` / `--pending-add-gated` / `--review-add`. Opportunistic
    /// ops-proof auto-completion must never reap an id that first appeared this
    /// cycle — the on-disk snapshot it compares against is updated by the same
    /// write invocation, so the snapshot baseline alone cannot distinguish a
    /// brand-new same-cycle add from a pre-existing item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_added_ids: Vec<String>,
    /// True when the preflight-current document already had completed tracked
    /// work that closeout pending maintenance should reap. This lets closeout
    /// distinguish status-only cycles from tracked-work cycles without reading
    /// a non-authoritative disk replica when editor authority is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracked_work_maintenance_required_at_preflight: Option<bool>,
    #[serde(default)]
    pub ipc_snapshot_adoption_blocked: bool,
    /// `#exchange-prompt-dropped-on-merge`: user-authored exchange prompt lines
    /// that were dropped when `content_ours` was adopted over a divergent IPC
    /// candidate (live prompt drift after preflight). Recorded at adoption time
    /// so `session-check` can fail closed on the data-loss class even if the
    /// editor later overwrites the disk prompt via IPC buffer convergence
    /// (the silent-loss race the post-commit disk diff cannot win).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped_exchange_prompts: Vec<String>,
    /// `#queue-user-edit-overwrite`: user-authored `agent:queue` prompt lines
    /// (e.g. `- do [#gscaccess]`) that were dropped when `content_ours` was
    /// adopted over a divergent IPC candidate. Recorded at adoption time so
    /// `session-check` can fail closed if a user queue edit was silently
    /// deleted by write/reset/commit convergence instead of being consumed by
    /// the current response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped_queue_prompts: Vec<String>,
    /// `#queue-clear-unrun-items`: `do [#id]` queue head prompt texts present in
    /// the visible `agent:queue` at preflight time. An active queue head is
    /// executable user intent, so a closeout / reset / commit may delete one only
    /// with proof that it was consumed (this cycle's directive target), resolved
    /// (its `#id` left `agent:backlog` via done/gate/reap), or removed by an
    /// explicit user edit. `session-check` fails closed when a recorded head
    /// disappears from the committed queue while its `#id` is still open in
    /// `agent:backlog` and the cycle never targeted it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_queue_heads: Vec<String>,
    /// `#lr-queue-patchback-miss`: free-text (non-`do [#id]`) queue head prompt
    /// texts present in the visible `agent:queue` at preflight time. Unlike
    /// `active_queue_heads` (which carries id-backed directive heads), these
    /// free-text heads have no backlog id to track — the guard instead checks
    /// for a committed response, binary consume, or explicit deferral proof.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_free_text_queue_heads: Vec<String>,
    /// `#semmerge-ack-turn` (document_cell_merge Phase 4): node-keyed acks carried into
    /// the NEXT cycle's response. Recorded at convergence time
    /// ([`record_semantic_merge_acks`]) and carried forward exactly one cycle by
    /// [`start_preflight_with_task`], which preflight surfaces as
    /// `document_cell_merge_acks` so the agent emits an acknowledgement exchange turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_semantic_merge_acks: Vec<PendingSemanticMergeAck>,
    /// `#closeoutstall`: typed operator-gated closeout state. A response may be
    /// safely captured and queued for editor IPC while the live editor has not
    /// proven application. This keeps the cycle open and gives session-check,
    /// doctor, and hooks one canonical recovery surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_closeout: Option<BlockedCloseout>,
    /// `#queueskip`: id-backed queue heads the binary has SKIPPED — each was
    /// dispatched, came back unconsumed, and the queue advanced past it to a
    /// non-dependent drainable head so the loop does not wedge re-dispatching a
    /// dead ref. Accumulated across cycles (carried forward by
    /// [`start_preflight_with_task`]) and cleared for an id once it is consumed
    /// (resolved/reaped) or no longer present. Preflight stamps the visible `⏭️`
    /// skip marker on these heads and excludes them from selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_queue_head_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmitOutput {
    pub admitted: bool,
    pub file: String,
    pub cycle_id: String,
    pub cycle_phase: String,
    pub last_event: String,
    pub source: String,
    pub maintenance_required: bool,
    pub preflight_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
}

/// `#suprecyclespin` — seconds an open cycle may sit untouched (no IPC ack
/// connection in flight) at a harness turn boundary before the supervisor
/// recycle/restart defer path force-closes it as abandoned. Bounded so a
/// crashed/superseded older turn cannot wedge the recycle in `DeferCycleOpen`
/// (~2/sec `cycle_open` spin-loop, `idle_watch.rs`) forever.
///
/// `#suprecyclespin-falseabandon`: the earlier 45s value assumed "each phase
/// ticks `updated_at` and finalize holds IPC inflight", so a live cycle could
/// never look stale. That assumption is FALSE for a slow first response: the
/// cycle sits in `preflight_started` with an untouched `updated_at` and
/// `inflight == 0` for the ENTIRE harness generation, which for a large prompt
/// routinely exceeds 45s. Combined with a transiently-misread `turn_boundary`
/// (`prompt_visible && !turn_active`, both pane-scraped) that abandoned a live
/// turn mid-generation. The deadline is now generous enough to clear normal
/// first-response latency; the primary guard is the consecutive-tick debounce
/// below (a genuine generation does not hold `turn_boundary && stalled` across
/// many back-to-back polls) plus the `MAX_CYCLE_OPEN_DEFER_TICKS` recycle
/// escalation backstop, which force-recycles a never-closing cycle WITHOUT
/// abandoning it (the durable checkpoint survives for the fresh boot).
pub const STALLED_CYCLE_RESOLVE_SECS: u64 = 120;

/// `#suprecyclespin-falseabandon` — consecutive idle-watch polls
/// (`AUTO_TRIGGER_POLL_INTERVAL`, 500ms) for which `stalled_pre_response_cycle`
/// must hold at a `turn_boundary` before the supervisor force-abandons the
/// cycle. A live harness generation never sustains `turn_boundary && stalled`
/// for this long (the pane is not at a ready prompt while it streams output), so
/// the debounce rejects the transient boundary misread that abandoned a live
/// turn, while a genuinely orphaned/superseded cycle stays stalled every poll
/// and still resolves after the confirm window (~10s at 20 ticks).
pub const STALLED_CYCLE_RESOLVE_CONFIRM_TICKS: u32 = 20;

impl CycleState {
    pub fn is_open(&self) -> bool {
        !matches!(self.phase, CyclePhase::Committed | CyclePhase::Abandoned)
    }

    /// `#suprecyclespin` — whether this open cycle has stalled past
    /// `deadline_secs`: still open, no IPC ack connection in flight, and untouched
    /// (`now_secs - updated_at > deadline_secs`). A stalled open cycle is an
    /// abandoned older turn that a newer committed cycle has superseded; the
    /// supervisor recycle-defer path force-closes such a cycle (`mark_abandoned`)
    /// so the `execve` recycle reaches its boundary instead of deferring forever.
    /// Pure so the deadline policy is unit-testable without the live idle watch.
    pub fn open_stalled(&self, inflight: u64, now_secs: u64, deadline_secs: u64) -> bool {
        self.is_open() && inflight == 0 && now_secs.saturating_sub(self.updated_at) > deadline_secs
    }

    /// `#suprecyclespin`: whether a stalled open cycle may be force-abandoned at
    /// a supervisor recycle boundary. Only pre-response cycles are disposable.
    /// Once a response has been captured or written, the open cycle is durable
    /// recovery evidence and must survive recycle so the fresh process can retry
    /// or reconcile it instead of silently losing the response.
    pub fn stalled_pre_response_cycle(
        &self,
        inflight: u64,
        now_secs: u64,
        deadline_secs: u64,
    ) -> bool {
        self.open_stalled(inflight, now_secs, deadline_secs)
            && matches!(self.phase, CyclePhase::PreflightStarted)
            && self.capture_id.is_none()
            && self.response_sha256.is_none()
    }

    /// Derive the live finalize-pipeline view (`#fm-run-id-step` / `#fmrunid-wire`)
    /// from the authoritative cycle-state fields: `run_id` = cycle id, `step` =
    /// lowercase phase, plus the recorded `turn_id` / `queue_task_id`.
    ///
    /// This is the read-side mirror — preflight surfaces it so any invocation or
    /// editor plugin can see where the cycle is without parsing the sidecar JSON.
    /// Cycle-state stays authoritative; the document `agent_doc_pipeline:` block
    /// is only a fallback hint when no live cycle-state exists.
    pub fn to_pipeline(&self) -> agent_doc_frontmatter::frontmatter::AgentDocPipeline {
        agent_doc_frontmatter::frontmatter::AgentDocPipeline {
            run_id: Some(self.cycle_id.clone()),
            step: Some(self.phase.as_str().to_string()),
            turn_id: self.turn_id.clone(),
            queue_task_id: self.queue_task_id.clone(),
        }
    }
}

pub fn load(file: &Path) -> Result<Option<CycleState>> {
    let Some(path) = agent_doc_fs::cycle_state_path_for(file)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    // `#lzsidecaratomic`: the cycle-state JSON is a compatibility crash-recovery
    // projection, not authoritative state (the state machine + state backbone own
    // that). A corrupt, torn, or legacy-format sidecar must never fail a
    // hot/critical-path caller: treat it as absent so the caller falls back to the
    // durable state-backbone projection (and git), and the next accepted transition
    // re-persists a clean sidecar.
    match serde_json::from_str::<CycleState>(&content) {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            eprintln!(
                "[cycle-state] WARNING: ignoring unreadable cycle-state sidecar {} (treating as absent): {err}",
                path.display()
            );
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCloseoutState {
    pub cycle_id: Option<String>,
    pub session_id: Option<String>,
    pub phase: Option<CyclePhase>,
    pub capture_id: Option<String>,
    pub response_sha256: Option<String>,
    pub captured_response: Option<ProjectedCapturedResponse>,
    pub patch_id: Option<String>,
    pub response_cell: Option<ProjectedResponseCell>,
    pub commit: Option<String>,
    pub session_check_passed: bool,
    pub tracked_work_maintenance_required: Option<bool>,
    pub abandoned_reason: Option<String>,
    pub pending_semantic_merge_acks: Vec<PendingSemanticMergeAck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedResponseCell {
    pub operation_id: String,
    pub cell_id: String,
    pub response_sha256: String,
    pub content_hash: String,
    pub applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCapturedResponse {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    pub file_hash: Option<String>,
    pub snapshot_hash: Option<String>,
    pub baseline_content: Option<String>,
}

/// One content-bearing response checkpoint from the append-only Lazily
/// ledger. Unlike [`ProjectedCapturedResponse`], this retains history order so
/// structural recovery can inspect an older valid checkpoint after a newer
/// whole-document materialization was corrupted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedResponseCheckpoint {
    pub sequence: u64,
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    pub file_hash: Option<String>,
    pub snapshot_hash: Option<String>,
    pub baseline_content: Option<String>,
}

impl ProjectedCloseoutState {
    pub fn phase_is_open(&self) -> Option<bool> {
        self.phase.map(CyclePhase::is_open)
    }

    pub fn matches_cycle(&self, cycle_id: &str) -> bool {
        self.cycle_id.as_deref() == Some(cycle_id)
    }

    pub fn event_label(&self, phase: CyclePhase) -> String {
        match phase {
            CyclePhase::PreflightStarted => "state_backbone_preflight_started".to_string(),
            CyclePhase::ResponseCaptured => self
                .capture_id
                .as_deref()
                .map(|capture_id| {
                    format!("state_backbone_response_captured capture_id={capture_id}")
                })
                .unwrap_or_else(|| "state_backbone_response_captured".to_string()),
            CyclePhase::WriteApplied => self
                .patch_id
                .as_deref()
                .map(|patch_id| format!("state_backbone_write_applied patch_id={patch_id}"))
                .unwrap_or_else(|| "state_backbone_write_applied".to_string()),
            CyclePhase::Committed => self
                .commit
                .as_deref()
                .map(|commit| format!("state_backbone_commit_observed commit={commit}"))
                .unwrap_or_else(|| "state_backbone_commit_observed".to_string()),
            CyclePhase::Abandoned => self
                .abandoned_reason
                .as_deref()
                .map(|reason| format!("state_backbone_cycle_abandoned reason={reason}"))
                .unwrap_or_else(|| "state_backbone_cycle_abandoned".to_string()),
        }
    }
}

impl From<agent_doc_state_backbone::CloseoutProjection> for ProjectedCloseoutState {
    fn from(projection: agent_doc_state_backbone::CloseoutProjection) -> Self {
        Self {
            cycle_id: projection.cycle_id,
            session_id: projection.session_id,
            phase: projection.phase,
            capture_id: projection.capture_id,
            response_sha256: projection.response_sha256,
            captured_response: projection.captured_response.map(|capture| {
                ProjectedCapturedResponse {
                    cycle_id: capture.cycle_id,
                    capture_id: capture.capture_id,
                    response_sha256: capture.response_sha256,
                    response_body: capture.response_body,
                    file_hash: capture.file_hash,
                    snapshot_hash: capture.snapshot_hash,
                    baseline_content: capture.baseline_content,
                }
            }),
            patch_id: projection.patch_id,
            response_cell: projection.response_cell.map(|cell| ProjectedResponseCell {
                operation_id: cell.operation_id,
                cell_id: cell.cell_id,
                response_sha256: cell.response_sha256,
                content_hash: cell.content_hash,
                applied: cell.applied,
            }),
            commit: projection.commit,
            session_check_passed: projection.session_check_passed,
            tracked_work_maintenance_required: projection.tracked_work_maintenance_required,
            abandoned_reason: projection.abandoned_reason,
            pending_semantic_merge_acks: projection
                .pending_semantic_merge_acks
                .into_iter()
                .map(|ack| PendingSemanticMergeAck {
                    component: ack.component,
                    id: ack.id,
                    reason: ack.reason,
                    detail: ack.detail,
                    recorded_cycle_id: ack.recorded_cycle_id,
                    surfaced: ack.surfaced,
                })
                .collect(),
        }
    }
}

pub fn load_closeout_projection(file: &Path) -> Result<Option<ProjectedCloseoutState>> {
    Ok(load_document_projection(file)?
        .map(|document| ProjectedCloseoutState::from(document.closeout)))
}

pub fn load_projected_captured_response(
    file: &Path,
    capture_id: &str,
) -> Result<Option<ProjectedCapturedResponse>> {
    Ok(load_closeout_projection(file)?
        .and_then(|projection| projection.captured_response)
        .filter(|capture| capture.capture_id == capture_id))
}

/// Load a bounded newest-first history of content-bearing response captures.
///
/// Recovery previously had only the latest projection and JSON sidecars. A
/// malformed latest baseline can therefore hide the valid checkpoint directly
/// before it. This query keeps Lazily authoritative without replaying the full
/// event ledger on every repair attempt.
pub fn load_recent_captured_response_checkpoints(
    file: &Path,
    limit: usize,
) -> Result<Vec<CapturedResponseCheckpoint>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(Vec::new());
    };
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    if !agent_doc_sqlite::state_store::state_db_path(&project_root).exists() {
        return Ok(Vec::new());
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let rows = agent_doc_sqlite::state_store::load_recent_state_events_by_fact_type_from_db(
        &conn,
        &document_hash,
        "response_captured",
        limit,
    )?;
    let mut checkpoints = Vec::new();
    for row in rows {
        let event: agent_doc_state_backbone::StateEvent =
            serde_json::from_str(&row.payload_json)
                .with_context(|| format!("decode state event {}", row.event_id))?;
        if let agent_doc_state_backbone::StateFact::ResponseCaptured {
            cycle_id,
            capture_id,
            response_sha256,
            response_body: Some(response_body),
            file_hash,
            snapshot_hash,
            baseline_content,
            ..
        } = event.fact
        {
            checkpoints.push(CapturedResponseCheckpoint {
                sequence: row.sequence,
                cycle_id,
                capture_id,
                response_sha256,
                response_body,
                file_hash,
                snapshot_hash,
                baseline_content,
            });
        }
    }
    Ok(checkpoints)
}

fn load_document_projection(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::DocumentStateProjection>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(None);
    };
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    if !agent_doc_sqlite::state_store::state_db_path(&project_root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let rows = agent_doc_sqlite::state_store::load_state_events_for_cycle_projection_from_db(
        &conn,
        &document_hash,
    )?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in rows {
        let event: agent_doc_state_backbone::StateEvent =
            serde_json::from_str(&row.payload_json)
                .with_context(|| format!("decode state event {}", row.event_id))?;
        ledger.append(event);
    }

    Ok(ledger.project_document(&document_hash))
}

pub fn apply_closeout_projection_to_cycle_state(
    state: &mut CycleState,
    projection: &ProjectedCloseoutState,
) -> bool {
    let Some(projected_phase) = projection.phase else {
        return false;
    };
    if !projection.matches_cycle(&state.cycle_id) {
        return false;
    }

    let preserve_noop_commit_event = state.phase == projected_phase
        && agent_doc_turn::cycle_policy::is_noop_commit_event(&state.last_event);
    let changed = state.phase != projected_phase;
    state.phase = projected_phase;
    if !preserve_noop_commit_event {
        state.last_event = projection.event_label(projected_phase);
    }
    if state.capture_id.is_none() {
        state.capture_id = projection.capture_id.clone();
    }
    if state.response_sha256.is_none() {
        state.response_sha256 = projection.response_sha256.clone();
    }
    state.tracked_work_maintenance_required_at_preflight =
        projection.tracked_work_maintenance_required;
    changed
}

pub fn load_with_closeout_projection(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    // The closeout projection is read from the durable state backbone (state.db). If
    // that store is unreadable or corrupt it must not fail the hot/critical path
    // either — degrade to the sidecar-only state rather than propagating the error.
    let projection = load_closeout_projection(file).unwrap_or_else(|err| {
        eprintln!(
            "[cycle-state] WARNING: ignoring unreadable closeout projection for {} (using sidecar state only): {err}",
            file.display()
        );
        None
    });
    if let Some(projection) = projection
        && projection.matches_cycle(&state.cycle_id)
    {
        apply_closeout_projection_to_cycle_state(&mut state, &projection);
    }
    Ok(Some(state))
}

#[derive(Debug, Clone, Copy)]
pub struct TerminalCloseoutProofInput<'a> {
    pub cycle_id: &'a str,
    pub last_event: &'a str,
    pub did_commit: bool,
    pub file_hash: &'a str,
    pub snapshot_hash: &'a str,
    pub head_hash: &'a str,
    pub state_file_hash_matches: bool,
    pub state_snapshot_hash_matches: bool,
    pub agreement: &'a str,
    pub capture_id: Option<&'a str>,
    pub response_sha256: Option<&'a str>,
    pub recorded_at_ms: u64,
}

pub fn append_terminal_closeout_proof(
    file: &Path,
    proof: TerminalCloseoutProofInput<'_>,
) -> Result<bool> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let event_id = format!(
        "terminal-closeout-proof:{document_hash}:{}:{}:{}:{}:{}",
        proof.cycle_id, proof.file_hash, proof.snapshot_hash, proof.head_hash, proof.recorded_at_ms
    );
    append_state_fact(
        file,
        event_id,
        agent_doc_state_backbone::StateFact::TerminalCloseoutProofRecorded {
            document_hash,
            cycle_id: proof.cycle_id.to_string(),
            last_event: proof.last_event.to_string(),
            did_commit: proof.did_commit,
            file_hash: proof.file_hash.to_string(),
            snapshot_hash: proof.snapshot_hash.to_string(),
            head_hash: proof.head_hash.to_string(),
            state_file_hash_matches: proof.state_file_hash_matches,
            state_snapshot_hash_matches: proof.state_snapshot_hash_matches,
            agreement: proof.agreement.to_string(),
            capture_id: proof.capture_id.map(str::to_string),
            response_sha256: proof.response_sha256.map(str::to_string),
            recorded_at_ms: proof.recorded_at_ms,
        },
    )
}

pub fn load_latest_terminal_closeout_proof(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::TerminalCloseoutProofProjection>> {
    let Some(document) = load_document_projection(file)? else {
        return Ok(None);
    };
    let Some(cycle_id) = document.proof.latest_terminal_closeout_cycle_id else {
        return Ok(None);
    };
    Ok(document.proof.terminal_closeouts.get(&cycle_id).cloned())
}

pub struct CloseoutRecoveryEvidenceInput<'a> {
    pub visible_markdown_hash: &'a str,
    pub snapshot_hash: Option<&'a str>,
    pub active_cycle_id: Option<&'a str>,
    pub active_cycle_phase: Option<CyclePhase>,
    pub active_capture_id: Option<&'a str>,
    pub active_capture_cycle_id: Option<&'a str>,
    pub active_capture_state: Option<&'a str>,
    pub active_capture_response_sha256: Option<&'a str>,
    pub response_body: agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence,
    pub queue_only_drift: Option<agent_doc_state_backbone::CloseoutRecoveryQueueOnlyDriftEvidence>,
    pub snapshot_head_drift: Option<agent_doc_state_backbone::CloseoutRecoveryDriftEvidence>,
    pub snapshot_visible_drift: Option<agent_doc_state_backbone::CloseoutRecoveryDriftEvidence>,
    pub editor_ipc: agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence,
    pub binary_freshness: agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence,
    pub recorded_at_ms: u64,
}

pub fn append_closeout_recovery_evidence(
    file: &Path,
    evidence: CloseoutRecoveryEvidenceInput<'_>,
) -> Result<bool> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let evidence_key = closeout_recovery_evidence_key(&evidence)?;
    let event_id = format!("closeout-recovery-evidence:{document_hash}:{evidence_key}");
    append_state_fact(
        file,
        event_id,
        agent_doc_state_backbone::StateFact::CloseoutRecoveryEvidenceRecorded {
            document_hash,
            evidence_key,
            visible_markdown_hash: evidence.visible_markdown_hash.to_string(),
            snapshot_hash: evidence.snapshot_hash.map(str::to_string),
            active_cycle_id: evidence.active_cycle_id.map(str::to_string),
            active_cycle_phase: evidence.active_cycle_phase,
            active_capture_id: evidence.active_capture_id.map(str::to_string),
            active_capture_cycle_id: evidence.active_capture_cycle_id.map(str::to_string),
            active_capture_state: evidence.active_capture_state.map(str::to_string),
            active_capture_response_sha256: evidence
                .active_capture_response_sha256
                .map(str::to_string),
            response_body: evidence.response_body,
            queue_only_drift: evidence.queue_only_drift,
            snapshot_head_drift: evidence.snapshot_head_drift,
            snapshot_visible_drift: evidence.snapshot_visible_drift,
            editor_ipc: evidence.editor_ipc,
            binary_freshness: evidence.binary_freshness,
            recorded_at_ms: evidence.recorded_at_ms,
        },
    )
}

pub fn load_latest_closeout_recovery_evidence(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::CloseoutRecoveryEvidenceProjection>> {
    let Some(document) = load_document_projection(file)? else {
        return Ok(None);
    };
    let Some(key) = document.proof.latest_closeout_recovery_evidence_key else {
        return Ok(None);
    };
    Ok(document.proof.closeout_recovery_evidence.get(&key).cloned())
}

fn closeout_recovery_evidence_key(evidence: &CloseoutRecoveryEvidenceInput<'_>) -> Result<String> {
    let payload = serde_json::to_string(&serde_json::json!({
        "visible_markdown_hash": evidence.visible_markdown_hash,
        "snapshot_hash": evidence.snapshot_hash,
        "active_cycle_id": evidence.active_cycle_id,
        "active_cycle_phase": evidence.active_cycle_phase,
        "active_capture_id": evidence.active_capture_id,
        "active_capture_cycle_id": evidence.active_capture_cycle_id,
        "active_capture_state": evidence.active_capture_state,
        "active_capture_response_sha256": evidence.active_capture_response_sha256,
        "response_body": &evidence.response_body,
        "queue_only_drift": &evidence.queue_only_drift,
        "snapshot_head_drift": &evidence.snapshot_head_drift,
        "snapshot_visible_drift": &evidence.snapshot_visible_drift,
        "editor_ipc": &evidence.editor_ipc,
        "binary_freshness": &evidence.binary_freshness,
    }))
    .context("serialize closeout recovery evidence key")?;
    Ok(agent_doc_hash::content_hash(&payload))
}

pub fn load_pending_semantic_merge_acks(file: &Path) -> Result<Vec<PendingSemanticMergeAck>> {
    Ok(load_semantic_merge_ack_queue_source(file)?
        .map(|source| source.pending_semantic_merge_acks())
        .unwrap_or_default())
}

fn document_cell_merge_acks_to_carry(file: &Path) -> Result<Vec<PendingSemanticMergeAck>> {
    Ok(load_semantic_merge_ack_queue_source(file)?
        .map(|source| source.document_cell_merge_acks_to_carry())
        .unwrap_or_default())
}

enum DocumentCellMergeAckQueueSource {
    Projection(Box<ProjectedCloseoutState>),
    Cycle(Box<CycleState>),
}

impl DocumentCellMergeAckQueueSource {
    fn pending_semantic_merge_acks(&self) -> Vec<PendingSemanticMergeAck> {
        self.document_cell_merge_ack_queue().to_vec()
    }

    fn document_cell_merge_acks_to_carry(&self) -> Vec<PendingSemanticMergeAck> {
        carry_forward_unsurfaced_semantic_merge_acks(self.document_cell_merge_ack_queue())
    }

    fn document_cell_merge_ack_queue(&self) -> &[PendingSemanticMergeAck] {
        match self {
            Self::Projection(projection) => &projection.pending_semantic_merge_acks,
            Self::Cycle(state) => &state.pending_semantic_merge_acks,
        }
    }
}

fn load_semantic_merge_ack_queue_source(
    file: &Path,
) -> Result<Option<DocumentCellMergeAckQueueSource>> {
    let raw = load(file)?;
    let Some(projection) = load_closeout_projection(file)? else {
        return Ok(raw.map(|state| DocumentCellMergeAckQueueSource::Cycle(Box::new(state))));
    };
    if !projection.pending_semantic_merge_acks.is_empty() {
        return Ok(Some(DocumentCellMergeAckQueueSource::Projection(Box::new(
            projection,
        ))));
    }
    if let Some(raw) = raw
        && projection.matches_cycle(&raw.cycle_id)
    {
        return Ok(Some(DocumentCellMergeAckQueueSource::Cycle(Box::new(raw))));
    }
    Ok(Some(DocumentCellMergeAckQueueSource::Projection(Box::new(
        projection,
    ))))
}

fn carry_forward_unsurfaced_semantic_merge_acks(
    acks: &[PendingSemanticMergeAck],
) -> Vec<PendingSemanticMergeAck> {
    acks.iter()
        .filter(|ack| !ack.surfaced)
        .cloned()
        .map(|mut ack| {
            ack.surfaced = true;
            ack
        })
        .collect()
}

pub fn admit_with_current_resolver<R, S, L>(
    file: &Path,
    mut resolve_current: R,
    mut load_snapshot: S,
    mut log_admission: L,
) -> Result<AdmitOutput>
where
    R: FnMut(&Path) -> Result<String>,
    S: FnMut(&Path) -> Result<Option<String>>,
    L: FnMut(&Path, &str),
{
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let current = resolve_current(file)
        .with_context(|| format!("failed to resolve current document for {}", file.display()))?;
    let snapshot = load_snapshot(file)
        .with_context(|| format!("failed to load snapshot for {}", file.display()))?;

    let state = start_preflight(file, snapshot.as_deref(), Some(&current))?;
    let phase = state.phase.as_str().to_string();
    log_admission(
        file,
        &format!(
            "realtime_admit file={} cycle_id={} phase={} source=admit action=accepted maintenance_required=false preflight_required=false",
            file.display(),
            state.cycle_id,
            phase
        ),
    );

    Ok(AdmitOutput {
        admitted: true,
        file: file
            .canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .display()
            .to_string(),
        cycle_id: state.cycle_id,
        cycle_phase: phase,
        last_event: state.last_event,
        source: "admit".to_string(),
        maintenance_required: false,
        preflight_required: false,
        snapshot_hash: state.snapshot_hash,
        file_hash: state.file_hash,
    })
}

pub fn start_preflight(
    file: &Path,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    start_preflight_with_task(file, snapshot_content, file_content, None, None)
}

/// (#reentrant-finalize Phase 5) Start preflight with optional queue task
/// identifiers. `queue_task_id` is the backlog ID (e.g. `#reentrant-phase2`).
/// `turn_id` is derived from the backlog ID or auto-generated. Both are stored
/// in cycle state so crash recovery can correlate the pipeline step with the
/// original task.
pub fn start_preflight_with_task(
    file: &Path,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    queue_task_id: Option<&str>,
    turn_id: Option<&str>,
) -> Result<CycleState> {
    let now = now_secs();
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    // `#semmerge-ack-turn` (Phase 4): carry forward acks recorded by the prior
    // cycle's convergence so this cycle's response can acknowledge the non-applied
    // agent change. Carry only un-surfaced acks and mark them surfaced here — an
    // ack the prior cycle itself carried IN was already surfaced there, so it
    // drops. Driven by the `surfaced` flag rather than a cycle-id comparison
    // because `cycle_id` is millisecond-derived and can collide across cycles.
    let carried_semantic_merge_acks = document_cell_merge_acks_to_carry(file).unwrap_or_default();
    // `#queueskip`: carry forward the skipped-head accumulator so a head marked
    // skippable in a prior cycle stays skipped until it is consumed or removed
    // (preflight recomputes/clears it each cycle). Without this the flag would
    // reset every cycle and the dead head would be re-dispatched on alternating
    // cycles instead of staying skipped.
    let carried_skipped_queue_head_ids = load(file)
        .ok()
        .flatten()
        .map(|prior| prior.skipped_queue_head_ids)
        .unwrap_or_default();
    let phase = CyclePhaseMachine::transition(CyclePhase::Committed, CycleEvent::StartPreflight)
        .unwrap_or(CyclePhase::PreflightStarted);
    let state = CycleState {
        cycle_id: format!("cycle-{}", now_millis()),
        file: canonical.display().to_string(),
        phase,
        last_event: "preflight_started".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: snapshot_content.map(agent_doc_hash::content_hash),
        file_hash: file_content.map(agent_doc_hash::content_hash),
        normalized_snapshot_hash: snapshot_content.map(replay_content_hash),
        normalized_file_hash: file_content.map(replay_content_hash),
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        requires_backlog_capture: false,
        required_backlog_targets: Vec::new(),
        required_explicit_backlog_item_count: 0,
        required_plan_reference_count: 0,
        baseline_file: None,
        prompt_targets: Vec::new(),
        queue_task_id: queue_task_id.map(|s| s.to_string()),
        turn_id: turn_id.map(|s| s.to_string()),
        recycle_resume_consumed: false,
        pending_done_ids: Vec::new(),
        pending_kept_open_ids: Vec::new(),
        reaped_pending_ids: Vec::new(),
        expect_done_or_gate_ids: Vec::new(),
        pending_gated_ids: Vec::new(),
        pending_added_this_cycle: false,
        pending_added_ids: Vec::new(),
        tracked_work_maintenance_required_at_preflight: file_content
            .map(agent_doc_document::tracked_work_projection::tracked_work_maintenance_required),
        ipc_snapshot_adoption_blocked: false,
        dropped_exchange_prompts: Vec::new(),
        dropped_queue_prompts: Vec::new(),
        active_queue_heads: file_content
            .map(agent_doc_queue::queue_heads::active_queue_heads)
            .unwrap_or_default(),
        active_free_text_queue_heads: file_content
            .map(agent_doc_queue::queue_heads::active_free_text_queue_heads)
            .unwrap_or_default(),
        pending_semantic_merge_acks: carried_semantic_merge_acks,
        blocked_closeout: None,
        skipped_queue_head_ids: carried_skipped_queue_head_ids,
    };
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::PreflightStarted)?;
    append_semantic_merge_ack_carried_forward_events(
        file,
        &state.cycle_id,
        &state.pending_semantic_merge_acks,
    )?;
    append_phase_event_to_session_log(file, &state, file_content);
    Ok(state)
}

/// Record (or overwrite) the runnable `agent:queue` heads that were active at
/// the start of the cycle (`#queue-clear-unrun-items`). Normally populated by
/// `start_preflight` from the visible document; exposed so a caller that only
/// has the pre-cycle queue text later can backfill the proof anchor.
pub fn record_active_queue_heads(file: &Path, heads: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let normalized: Vec<String> = heads
        .iter()
        .map(|head| head.trim().to_string())
        .filter(|head| !head.is_empty())
        .collect();
    if state.active_queue_heads != normalized {
        state.active_queue_heads = normalized;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#manual-queue-head-loss`: union the live working-tree `agent:queue` directive
/// heads into the recorded removal-proof anchor. `start_preflight` records
/// `active_queue_heads` once, from the document as it stood at preflight. A user
/// who types a fresh `do [#id]` into `agent:queue` AFTER that point — e.g. while a
/// dispatch attempt is stalled on owner-pane recursion or a busy authoritative
/// actor — is invisible to the `#queue-clear-unrun-items` removal guard, so a
/// later closeout convergence can silently drop that runnable manual head while
/// its backlog item stays open. Observing the live heads at the write/commit
/// boundary (before any pending mutation or queue convergence) extends the same
/// durable-proof requirement to manually inserted heads. Only ADDS heads; never
/// removes an already-recorded head, so a legitimately consumed head still
/// resolves through the existing done/gate/reap proof in the removal guard.
pub fn observe_live_queue_heads(file: &Path, doc: &str) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let mut changed = false;
    for head in agent_doc_queue::queue_heads::active_queue_heads(doc) {
        let head = head.trim().to_string();
        if head.is_empty() {
            continue;
        }
        if !state
            .active_queue_heads
            .iter()
            .any(|existing| existing == &head)
        {
            state.active_queue_heads.push(head);
            changed = true;
        }
    }
    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#durablerecycle`: persist the prompt-facing turn checkpoint once preflight
/// has computed the stable merge baseline and prompt target identity. The
/// initial `start_preflight` record opens early; this fills the restart-critical
/// fields that are only known after queue and diff analysis.
pub fn record_turn_checkpoint(
    file: &Path,
    baseline_file: Option<&str>,
    prompt_targets: &[String],
    queue_task_id: Option<&str>,
    turn_id: Option<&str>,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(Some(state));
    }

    let normalized_baseline = baseline_file
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned);
    let normalized_prompt_targets = normalize_checkpoint_text_list(prompt_targets);
    let normalized_queue_task_id = queue_task_id
        .map(normalize_checkpoint_task_id)
        .filter(|id| !id.is_empty());
    let normalized_turn_id = turn_id
        .map(normalize_checkpoint_task_id)
        .filter(|id| !id.is_empty());

    let mut changed = false;
    if state.baseline_file != normalized_baseline {
        state.baseline_file = normalized_baseline;
        changed = true;
    }
    if state.prompt_targets != normalized_prompt_targets {
        state.prompt_targets = normalized_prompt_targets;
        changed = true;
    }
    if state.queue_task_id != normalized_queue_task_id {
        state.queue_task_id = normalized_queue_task_id;
        changed = true;
    }
    if state.turn_id != normalized_turn_id {
        state.turn_id = normalized_turn_id;
        changed = true;
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#midturn-recycle-resume` Phase B: latch that a fresh supervisor boot has
/// re-dispatched this open cycle's interrupted turn from the `#durablerecycle`
/// checkpoint (the harness child died across the `execve` recycle). Idempotency
/// guard for the boot-resume path — a later boot reading the same still-open
/// checkpoint sees the latch and does NOT re-dispatch the turn again. A committed
/// cycle is never open so this is a no-op there. Returns the (possibly updated)
/// state, or `Ok(None)` when no cycle state exists.
pub fn mark_recycle_resume_consumed(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.recycle_resume_consumed {
        state.recycle_resume_consumed = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn mark_write_applied(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
    let Some(next_phase) = CyclePhaseMachine::transition(state.phase, CycleEvent::WriteApplied)
    else {
        return Ok(state);
    };
    state.phase = next_phase;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(agent_doc_hash::content_hash);
    state.file_hash = file_content.map(agent_doc_hash::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(replay_content_hash);
    state.normalized_file_hash = file_content.map(replay_content_hash);
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::WriteApplied)?;
    append_phase_event_to_session_log(file, &state, file_content);
    Ok(state)
}

pub fn mark_response_captured(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    response_sha256: &str,
    cycle_id_hint: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| {
        synthetic_state_with_id(file, CyclePhase::PreflightStarted, cycle_id_hint)
    });
    let Some(next_phase) = CyclePhaseMachine::transition(state.phase, CycleEvent::ResponseCaptured)
    else {
        return Ok(state);
    };
    state.phase = next_phase;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(agent_doc_hash::content_hash);
    state.file_hash = file_content.map(agent_doc_hash::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(replay_content_hash);
    state.normalized_file_hash = file_content.map(replay_content_hash);
    state.capture_id = Some(state.cycle_id.clone());
    state.response_sha256 = Some(response_sha256.to_string());
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::ResponseCaptured)?;
    append_phase_event_to_session_log(file, &state, file_content);
    // NB: intentionally do NOT mirror the pipeline block here. A captured response
    // is the complete final payload, but capture durability is not visible document
    // authority. The mirror runs at `write_applied` only after final placement is
    // proven. Recovery-only partial checkpoints never call this transition (#22a8).
    Ok(state)
}

pub fn mark_pending_mutations(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.had_pending_mutations {
        state.had_pending_mutations = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_pending_done_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_done_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_done_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_pending_kept_open_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_kept_open_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_kept_open_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_reaped_pending_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .reaped_pending_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.reaped_pending_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#queueskip`: overwrite the skipped-head accumulator with the fully-recomputed
/// set for this cycle (carry-forward + newly-stalled heads, minus any that were
/// consumed or are no longer present). Preflight owns the recompute, so this is a
/// replace, not an append.
pub fn set_skipped_queue_head_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let mut seen = std::collections::HashSet::new();
    let normalized: Vec<String> = ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect();
    if state.skipped_queue_head_ids != normalized {
        state.skipped_queue_head_ids = normalized;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_expect_done_or_gate_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .expect_done_or_gate_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.expect_done_or_gate_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#blocked-closeout-followup-capture`: record tracked ids gated this cycle.
pub fn record_pending_gated_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_gated_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_gated_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#opsproof-samecycle-add`: record the ids of tracked-work items added this
/// cycle (via `--pending-add` / `--pending-add-gated` / `--review-add`) so the
/// opportunistic ops-proof auto-completion can exclude brand-new same-cycle adds.
pub fn record_pending_added_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_added_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_added_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#opsproof-samecycle-add`: ids of tracked-work items added this cycle. Empty
/// when no cycle state exists. Used to exclude brand-new same-cycle adds from
/// opportunistic ops-proof auto-completion.
pub fn pending_added_ids(file: &Path) -> std::collections::HashSet<String> {
    load(file)
        .ok()
        .flatten()
        .map(|state| state.pending_added_ids.into_iter().collect())
        .unwrap_or_default()
}

/// `#blocked-closeout-followup-capture`: mark that this cycle added at least one
/// follow-up backlog item via a `--pending-add*` primitive.
pub fn mark_pending_added(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.pending_added_this_cycle {
        state.pending_added_this_cycle = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn resolved_pending_ids(file: &Path) -> Result<std::collections::HashSet<String>> {
    let Some(state) = load(file)? else {
        return Ok(std::collections::HashSet::new());
    };

    Ok(state
        .pending_done_ids
        .into_iter()
        .chain(state.reaped_pending_ids)
        .collect())
}

pub fn record_backlog_capture_requirement(
    file: &Path,
    required: bool,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.requires_backlog_capture != required {
        state.requires_backlog_capture = required;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_backlog_target_requirements(
    file: &Path,
    requirements: &[BacklogTargetRequirement],
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_backlog_targets != requirements {
        state.required_backlog_targets = requirements.to_vec();
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_required_explicit_backlog_item_count(
    file: &Path,
    count: usize,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_explicit_backlog_item_count != count {
        state.required_explicit_backlog_item_count = count;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_required_plan_reference_count(
    file: &Path,
    count: usize,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_plan_reference_count != count {
        state.required_plan_reference_count = count;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn mark_recoverable_preflight_timeout(file: &Path, event: &str) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(Some(state));
    }
    let Some(next_phase) =
        CyclePhaseMachine::transition(state.phase, CycleEvent::RecoverablePreflightTimeout)
    else {
        return Ok(Some(state));
    };
    state.phase = next_phase;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state, None);
    Ok(Some(state))
}

pub fn record_open_cycle_progress(file: &Path, event: &str) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(Some(state));
    }
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state, None);
    Ok(Some(state))
}

pub fn record_ipc_snapshot_adoption_blocked(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.ipc_snapshot_adoption_blocked {
        state.ipc_snapshot_adoption_blocked = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#exchange-prompt-dropped-on-merge`: record user-authored exchange prompt
/// line(s) dropped when `content_ours` was adopted over a divergent IPC
/// candidate, so `session-check` can fail closed even after the editor
/// overwrites the disk prompt. Appends only previously-unseen prompt lines.
pub fn record_dropped_exchange_prompts(
    file: &Path,
    prompts: &[String],
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let mut changed = false;
    for prompt in prompts {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !state
            .dropped_exchange_prompts
            .iter()
            .any(|existing| existing == trimmed)
        {
            state.dropped_exchange_prompts.push(trimmed.to_string());
            changed = true;
        }
    }
    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// Clear the recorded dropped-prompt markers once they are resolved (the prompt
/// reached the committed document on a later cycle).
pub fn clear_dropped_exchange_prompts(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.dropped_exchange_prompts.is_empty() {
        state.dropped_exchange_prompts.clear();
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#queue-user-edit-overwrite`: record user-authored `agent:queue` prompt
/// line(s) dropped when `content_ours` was adopted over a divergent IPC
/// candidate, so `session-check` can fail closed if a user queue edit was
/// silently deleted instead of consumed. Appends only previously-unseen lines.
pub fn record_dropped_queue_prompts(file: &Path, prompts: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let mut changed = false;
    for prompt in prompts {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !state
            .dropped_queue_prompts
            .iter()
            .any(|existing| existing == trimmed)
        {
            state.dropped_queue_prompts.push(trimmed.to_string());
            changed = true;
        }
    }
    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// `#semmerge-ack-turn` (document_cell_merge Phase 4): record node-keyed acks emitted
/// by the convergence semantic merge so the NEXT cycle can acknowledge the
/// non-applied agent change in an exchange turn. Tags each ack with the current
/// cycle id ([`start_preflight_with_task`] carries it forward exactly one cycle).
/// Appends only previously-unseen `(component, id, reason)` triples.
pub fn record_semantic_merge_acks(
    file: &Path,
    acks: &[agent_doc_merge::document_cell_merge::AckRequest],
) -> Result<Option<CycleState>> {
    let Some(mut state) = load_with_closeout_projection(file)? else {
        if let Some(projection) = load_closeout_projection(file)?
            && let Some(cycle_id) = projection.cycle_id.as_deref()
        {
            for ack in acks {
                append_semantic_merge_ack_recorded_event(
                    file,
                    cycle_id,
                    &PendingSemanticMergeAck {
                        component: ack.component.clone(),
                        id: ack.id.clone(),
                        reason: ack.reason.token().to_string(),
                        detail: ack.detail.clone(),
                        recorded_cycle_id: Some(cycle_id.to_string()),
                        surfaced: false,
                    },
                )?;
            }
        }
        return Ok(None);
    };
    let cycle_id = state.cycle_id.clone();
    let mut changed = false;
    for ack in acks {
        let reason = ack.reason.token().to_string();
        let pending_ack = PendingSemanticMergeAck {
            component: ack.component.clone(),
            id: ack.id.clone(),
            reason: reason.clone(),
            detail: ack.detail.clone(),
            recorded_cycle_id: Some(cycle_id.clone()),
            surfaced: false,
        };
        append_semantic_merge_ack_recorded_event(file, &cycle_id, &pending_ack)?;
        if !state.pending_semantic_merge_acks.iter().any(|existing| {
            existing.component == ack.component
                && existing.id == ack.id
                && existing.reason == reason
        }) {
            state.pending_semantic_merge_acks.push(pending_ack);
            changed = true;
        }
    }
    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

/// Clear the recorded dropped-queue markers once they are resolved (the queue
/// edit reached the committed document or was legitimately consumed).
pub fn clear_dropped_queue_prompts(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.dropped_queue_prompts.is_empty() {
        state.dropped_queue_prompts.clear();
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_editor_convergence_required(
    file: &Path,
    source: &str,
    reason: &str,
    patch_id: Option<&str>,
    detail: Option<&str>,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let blocked = BlockedCloseout {
        kind: "editor_convergence_required".to_string(),
        reason: reason.to_string(),
        source: source.to_string(),
        patch_id: patch_id.map(str::to_string),
        recovery: Some("retry_without_disk_write".to_string()),
        // A captured response is resumed by the keyed supervisor worker. A
        // manual closeout command here caused agents to stack another write on
        // top of the still-live capture.
        recovery_command: None,
        detail: detail.map(str::to_string),
    };
    if state.blocked_closeout.as_ref() != Some(&blocked) {
        state.blocked_closeout = Some(blocked);
        state.updated_at = now_secs();
        save(file, &state)?;
        append_phase_event_to_session_log(file, &state, None);
    }
    Ok(Some(state))
}

pub fn mark_committed(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state = match load(file)? {
        Some(state) if state.phase == CyclePhase::Abandoned => {
            synthetic_state(file, CyclePhase::WriteApplied)
        }
        Some(state) => state,
        None => synthetic_state(file, CyclePhase::WriteApplied),
    };
    if matches!(state.phase, CyclePhase::Committed)
        && (state.last_event == event || is_stable_commit_event(&state.last_event))
    {
        append_closeout_projection_event(file, &state, CloseoutProjectionEvent::Committed)?;
        return Ok(state);
    }
    let Some(next_phase) = CyclePhaseMachine::transition(state.phase, CycleEvent::Committed) else {
        return Ok(state);
    };
    state.phase = next_phase;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.blocked_closeout = None;
    if let Some(snapshot) = snapshot_content {
        state.snapshot_hash = Some(agent_doc_hash::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(replay_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(agent_doc_hash::content_hash(content));
        state.normalized_file_hash = Some(replay_content_hash(content));
    }
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::Committed)?;
    append_phase_event_to_session_log(file, &state, file_content);
    Ok(state)
}

pub fn mark_abandoned(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
    if state.phase == CyclePhase::Abandoned {
        append_closeout_projection_event(file, &state, CloseoutProjectionEvent::Abandoned)?;
        return Ok(state);
    }
    if !state.is_open() {
        return Ok(state);
    }
    let Some(next_phase) = CyclePhaseMachine::transition(state.phase, CycleEvent::Abandoned) else {
        return Ok(state);
    };
    state.phase = next_phase;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    if let Some(snapshot) = snapshot_content {
        state.snapshot_hash = Some(agent_doc_hash::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(replay_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(agent_doc_hash::content_hash(content));
        state.normalized_file_hash = Some(replay_content_hash(content));
    }
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::Abandoned)?;
    append_phase_event_to_session_log(file, &state, file_content);
    Ok(state)
}

/// Restore the exact cycle that was incorrectly abandoned by the
/// `retire_superseded_captured_only_orphan` repair while a retained document
/// write was still in flight. This is deliberately narrower than an ordinary
/// abandoned-cycle reopen: both durable identities and the precise retirement
/// event must match.
pub fn reactivate_false_stale_capture_retirement(
    file: &Path,
    capture_id: &str,
    response_sha256: &str,
) -> Result<bool> {
    let Some(mut state) = load(file)? else {
        return Ok(false);
    };
    if state.phase == CyclePhase::ResponseCaptured
        && state.capture_id.as_deref() == Some(capture_id)
        && state.response_sha256.as_deref() == Some(response_sha256)
    {
        return Ok(true);
    }
    if state.phase != CyclePhase::Abandoned
        || state.last_event != "repair_retire_superseded_captured_only_orphan"
        || state.cycle_id != capture_id
        || state.capture_id.as_deref() != Some(capture_id)
        || state.response_sha256.as_deref() != Some(response_sha256)
    {
        return Ok(false);
    }

    state.phase = CyclePhase::ResponseCaptured;
    state.last_event = "session_check_reactivated_false_stale_capture_retirement".to_string();
    state.updated_at = now_secs();
    state.blocked_closeout = None;
    save(file, &state)?;
    append_closeout_projection_event(file, &state, CloseoutProjectionEvent::FalseStaleReactivated)?;
    append_phase_event_to_session_log(file, &state, None);
    Ok(true)
}

fn append_phase_event_to_session_log(file: &Path, state: &CycleState, file_content: Option<&str>) {
    let Some(content) = file_content else {
        return;
    };
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return;
    };
    let Some(session_id) = fm
        .session
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return;
    };

    let mut event = format!(
        "document_cycle phase={} cycle={} event={}",
        state.phase.as_str(),
        state.cycle_id,
        state.last_event
    );
    if let Some(capture_id) = state.capture_id.as_deref() {
        event.push_str(&format!(" capture_id={capture_id}"));
    }
    let _ =
        agent_doc_supervisor_io::startup_miss::append_session_log_event(file, session_id, &event);
}

fn save(file: &Path, state: &CycleState) -> Result<()> {
    let Some(path) = agent_doc_fs::cycle_state_path_for(file)? else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(state)?;
    write_atomic(&path, &json)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum CloseoutProjectionEvent {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
    FalseStaleReactivated,
}

fn append_closeout_projection_event(
    file: &Path,
    state: &CycleState,
    event: CloseoutProjectionEvent,
) -> Result<bool> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let fact = match event {
        CloseoutProjectionEvent::PreflightStarted => {
            agent_doc_state_backbone::StateFact::PreflightStarted {
                document_hash: document_hash.clone(),
                cycle_id: state.cycle_id.clone(),
                session_id: None,
                tracked_work_maintenance_required: state
                    .tracked_work_maintenance_required_at_preflight,
            }
        }
        CloseoutProjectionEvent::ResponseCaptured => {
            let Some(capture_id) = state.capture_id.clone() else {
                return Ok(false);
            };
            let Some(response_sha256) = state.response_sha256.clone() else {
                return Ok(false);
            };
            agent_doc_state_backbone::StateFact::ResponseCaptured {
                document_hash: document_hash.clone(),
                cycle_id: state.cycle_id.clone(),
                capture_id,
                response_sha256,
                response_body: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            }
        }
        CloseoutProjectionEvent::WriteApplied => {
            agent_doc_state_backbone::StateFact::WriteApplied {
                document_hash: document_hash.clone(),
                cycle_id: state.cycle_id.clone(),
                patch_id: None,
            }
        }
        CloseoutProjectionEvent::Committed => agent_doc_state_backbone::StateFact::CommitObserved {
            document_hash: document_hash.clone(),
            cycle_id: state.cycle_id.clone(),
            commit: state
                .file_hash
                .as_ref()
                .map(|hash| format!("content:{hash}"))
                .unwrap_or_else(|| format!("event:{}", state.last_event)),
        },
        CloseoutProjectionEvent::Abandoned => agent_doc_state_backbone::StateFact::CycleAbandoned {
            document_hash: document_hash.clone(),
            cycle_id: state.cycle_id.clone(),
            reason: state.last_event.clone(),
        },
        CloseoutProjectionEvent::FalseStaleReactivated => {
            let Some(capture_id) = state.capture_id.clone() else {
                return Ok(false);
            };
            let Some(response_sha256) = state.response_sha256.clone() else {
                return Ok(false);
            };
            agent_doc_state_backbone::StateFact::FalseStaleCaptureReactivated {
                document_hash: document_hash.clone(),
                cycle_id: state.cycle_id.clone(),
                capture_id,
                response_sha256,
                retirement_reason: "repair_retire_superseded_captured_only_orphan".to_string(),
            }
        }
    };
    let event_id = closeout_projection_event_id(&document_hash, state, event);
    append_state_fact(file, event_id, fact)
}

pub struct CapturedResponseFactInput<'a> {
    pub cycle_id: &'a str,
    pub capture_id: &'a str,
    pub response_sha256: &'a str,
    pub response_body: &'a str,
    pub file_hash: Option<&'a str>,
    pub snapshot_hash: Option<&'a str>,
    pub baseline_content: Option<&'a str>,
}

pub fn append_response_captured_body(
    file: &Path,
    input: CapturedResponseFactInput<'_>,
) -> Result<bool> {
    let CapturedResponseFactInput {
        cycle_id,
        capture_id,
        response_sha256,
        response_body,
        file_hash,
        snapshot_hash,
        baseline_content,
    } = input;
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let baseline_hash = baseline_content
        .map(agent_doc_hash::content_hash)
        .unwrap_or_else(|| "legacy-hash-only".to_string());
    let event_id = format!(
        "closeout-response-captured-body:v2:{document_hash}:{cycle_id}:{capture_id}:{response_sha256}:{baseline_hash}"
    );
    append_state_fact(
        file,
        event_id,
        agent_doc_state_backbone::StateFact::ResponseCaptured {
            document_hash,
            cycle_id: cycle_id.to_string(),
            capture_id: capture_id.to_string(),
            response_sha256: response_sha256.to_string(),
            response_body: Some(response_body.to_string()),
            file_hash: file_hash.map(str::to_string),
            snapshot_hash: snapshot_hash.map(str::to_string),
            baseline_content: baseline_content.map(str::to_string),
        },
    )
}

/// Persist the atomic assistant-response CRDT operation and advance closeout to
/// `write_applied` in one idempotent backbone fact.
pub fn append_response_cell_added(
    file: &Path,
    cycle_id: &str,
    operation_id: &str,
    cell_id: &str,
    response_sha256: &str,
    content_hash: &str,
    applied: bool,
) -> Result<bool> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let event_id =
        format!("response-cell-added:{document_hash}:{cycle_id}:{operation_id}:{cell_id}");
    append_state_fact(
        file,
        event_id,
        agent_doc_state_backbone::StateFact::ResponseCellAdded {
            document_hash,
            cycle_id: cycle_id.to_string(),
            operation_id: operation_id.to_string(),
            cell_id: cell_id.to_string(),
            response_sha256: response_sha256.to_string(),
            content_hash: content_hash.to_string(),
            applied,
        },
    )
}

fn append_state_fact(
    file: &Path,
    event_id: String,
    fact: agent_doc_state_backbone::StateFact,
) -> Result<bool> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let fact_label = fact.label();
    let event = agent_doc_state_backbone::StateEvent::new(event_id, fact);
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let payload_json = serde_json::to_string(&event).context("serialize closeout state event")?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &conn,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: fact_label,
            payload_json: &payload_json,
        },
    )
}

fn append_semantic_merge_ack_recorded_event(
    file: &Path,
    cycle_id: &str,
    ack: &PendingSemanticMergeAck,
) -> Result<bool> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(false);
    };
    let event_id = format!(
        "semantic-merge-ack-recorded:{document_hash}:{cycle_id}:{}:{}:{}",
        ack.component, ack.id, ack.reason
    );
    append_state_fact(
        file,
        event_id,
        agent_doc_state_backbone::StateFact::DocumentCellMergeAckRecorded {
            document_hash,
            cycle_id: cycle_id.to_string(),
            component: ack.component.clone(),
            id: ack.id.clone(),
            reason: ack.reason.clone(),
            detail: ack.detail.clone(),
        },
    )
}

fn append_semantic_merge_ack_carried_forward_events(
    file: &Path,
    target_cycle_id: &str,
    acks: &[PendingSemanticMergeAck],
) -> Result<()> {
    let Some(document_hash) = cycle_document_hash(file)? else {
        return Ok(());
    };
    for ack in acks {
        let source_cycle_id = ack.recorded_cycle_id.clone();
        let source = source_cycle_id.as_deref().unwrap_or("unknown");
        let event_id = format!(
            "semantic-merge-ack-carried:{document_hash}:{target_cycle_id}:{source}:{}:{}:{}",
            ack.component, ack.id, ack.reason
        );
        append_state_fact(
            file,
            event_id,
            agent_doc_state_backbone::StateFact::DocumentCellMergeAckCarriedForward {
                document_hash: document_hash.clone(),
                source_cycle_id,
                target_cycle_id: target_cycle_id.to_string(),
                component: ack.component.clone(),
                id: ack.id.clone(),
                reason: ack.reason.clone(),
                detail: ack.detail.clone(),
            },
        )?;
    }
    Ok(())
}

fn cycle_document_hash(file: &Path) -> Result<Option<String>> {
    Ok(agent_doc_fs::cycle_state_path_for(file)?.and_then(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.to_string())
    }))
}

fn closeout_projection_event_id(
    document_hash: &str,
    state: &CycleState,
    event: CloseoutProjectionEvent,
) -> String {
    match event {
        CloseoutProjectionEvent::PreflightStarted => {
            format!(
                "closeout-preflight-started:{document_hash}:{}",
                state.cycle_id
            )
        }
        CloseoutProjectionEvent::ResponseCaptured => format!(
            "closeout-response-captured:{document_hash}:{}:{}",
            state.cycle_id,
            state.response_sha256.as_deref().unwrap_or("missing")
        ),
        CloseoutProjectionEvent::WriteApplied => format!(
            "closeout-write-applied:{document_hash}:{}:{}",
            state.cycle_id,
            state.file_hash.as_deref().unwrap_or("missing")
        ),
        CloseoutProjectionEvent::Committed => format!(
            "closeout-committed:{document_hash}:{}:{}",
            state.cycle_id,
            state.file_hash.as_deref().unwrap_or("missing")
        ),
        CloseoutProjectionEvent::Abandoned => format!(
            "closeout-abandoned:{document_hash}:{}:{}",
            state.cycle_id, state.last_event
        ),
        CloseoutProjectionEvent::FalseStaleReactivated => format!(
            "closeout-false-stale-reactivated:{document_hash}:{}:{}",
            state.cycle_id,
            state.response_sha256.as_deref().unwrap_or("missing")
        ),
    }
}

/// `#lzsidecaratomic`: write `bytes` to `path` via a temp file in the same
/// directory plus an atomic `rename`, so a concurrent reader (including the PCP
/// closeout projection) can never observe a partial file (torn read). `load()`
/// maps `NotFound` to clean absence, but a truncated mid-write file surfaces as
/// a parse error; the atomic persist closes that window. Falls back to a direct
/// write only when the path has no parent to stage a temp file in.
fn write_atomic(path: &Path, bytes: &str) -> Result<()> {
    match path.parent() {
        Some(parent) => {
            std::fs::create_dir_all(parent)?;
            let temp = tempfile::NamedTempFile::new_in(parent)?;
            std::fs::write(temp.path(), bytes)?;
            temp.persist(path)?;
        }
        None => {
            std::fs::write(path, bytes)?;
        }
    }
    Ok(())
}

fn synthetic_state(file: &Path, phase: CyclePhase) -> CycleState {
    synthetic_state_with_id(file, phase, None)
}

fn synthetic_state_with_id(
    file: &Path,
    phase: CyclePhase,
    cycle_id_hint: Option<&str>,
) -> CycleState {
    let now = now_secs();
    CycleState {
        cycle_id: cycle_id_hint
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("synthetic-{}", now_millis())),
        file: file.display().to_string(),
        phase,
        last_event: "synthetic_state".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: None,
        file_hash: None,
        normalized_snapshot_hash: None,
        normalized_file_hash: None,
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        requires_backlog_capture: false,
        required_backlog_targets: Vec::new(),
        required_explicit_backlog_item_count: 0,
        required_plan_reference_count: 0,
        baseline_file: None,
        prompt_targets: Vec::new(),
        queue_task_id: None,
        turn_id: None,
        recycle_resume_consumed: false,
        pending_done_ids: Vec::new(),
        pending_kept_open_ids: Vec::new(),
        reaped_pending_ids: Vec::new(),
        expect_done_or_gate_ids: Vec::new(),
        pending_gated_ids: Vec::new(),
        pending_added_this_cycle: false,
        pending_added_ids: Vec::new(),
        tracked_work_maintenance_required_at_preflight: Some(false),
        ipc_snapshot_adoption_blocked: false,
        dropped_exchange_prompts: Vec::new(),
        dropped_queue_prompts: Vec::new(),
        active_queue_heads: Vec::new(),
        active_free_text_queue_heads: Vec::new(),
        pending_semantic_merge_acks: Vec::new(),
        blocked_closeout: None,
        skipped_queue_head_ids: Vec::new(),
    }
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn open_stalled_resolves_only_abandoned_older_turns() {
        // `#suprecyclespin`: the recycle-defer gate must distinguish a live cycle
        // (keep deferring) from a stalled/superseded older turn (force-close).
        let mut state = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::PreflightStarted);
        let deadline = STALLED_CYCLE_RESOLVE_SECS;
        // updated_at far in the past, no IPC inflight → stalled (resolvable).
        state.updated_at = 1_000;
        assert!(state.open_stalled(0, 1_000 + deadline + 1, deadline));
        assert!(state.stalled_pre_response_cycle(0, 1_000 + deadline + 1, deadline));
        // Exactly at the deadline is NOT yet stalled (strict `>`).
        assert!(!state.open_stalled(0, 1_000 + deadline, deadline));
        assert!(!state.stalled_pre_response_cycle(0, 1_000 + deadline, deadline));
        // Fresh `updated_at` → a live cycle, never force-closed.
        state.updated_at = 1_000 + deadline + 1;
        assert!(!state.open_stalled(0, 1_000 + deadline + 5, deadline));
        assert!(!state.stalled_pre_response_cycle(0, 1_000 + deadline + 5, deadline));
        // IPC ack connection in flight → finalize is live, never force-closed even
        // if the cycle has been open a long time.
        state.updated_at = 1_000;
        assert!(!state.open_stalled(1, 1_000 + deadline + 100, deadline));
        assert!(!state.stalled_pre_response_cycle(1, 1_000 + deadline + 100, deadline));
        // Once a response is captured, the stalled open cycle is durable recovery
        // evidence. It may keep the recycle gate open until escalation, but must
        // not be abandoned as disposable pre-response state.
        let mut captured = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::ResponseCaptured);
        captured.updated_at = 1_000;
        captured.capture_id = Some("cycle-1".to_string());
        captured.response_sha256 = Some("abc".to_string());
        assert!(captured.open_stalled(0, 1_000 + deadline + 1, deadline));
        assert!(!captured.stalled_pre_response_cycle(0, 1_000 + deadline + 1, deadline));
        // A committed cycle is not open, so it is never "stalled".
        let committed = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::Committed);
        assert!(!committed.open_stalled(0, u64::MAX, deadline));
        assert!(!committed.stalled_pre_response_cycle(0, u64::MAX, deadline));
    }

    #[test]
    fn stalled_resolve_deadline_survives_slow_first_response() {
        // `#suprecyclespin-falseabandon` regression: a live preflight cycle whose
        // harness is still generating its first response sits untouched with
        // `inflight == 0` and an unadvanced `updated_at`. At an observed 46s the
        // old 45s deadline abandoned this live turn; the bumped deadline must
        // clear normal first-response latency.
        let mut state = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::PreflightStarted);
        state.updated_at = 1_000;
        let incident_stalled_secs = 46; // observed stalled_secs that falsely abandoned a live turn
        assert!(
            !state.stalled_pre_response_cycle(
                0,
                1_000 + incident_stalled_secs,
                STALLED_CYCLE_RESOLVE_SECS,
            ),
            "a 46s-stale live preflight cycle must not be resolvable after the deadline bump"
        );
        // Still bounded: a genuinely orphaned cycle past the (larger) deadline
        // remains resolvable so the recycle spin cannot wedge forever.
        assert!(state.stalled_pre_response_cycle(
            0,
            1_000 + STALLED_CYCLE_RESOLVE_SECS + 1,
            STALLED_CYCLE_RESOLVE_SECS,
        ));
        // The consecutive-tick debounce is a second, independent gate on top of
        // the deadline — a stale sidecar cannot abandon a live turn on a single
        // transiently-misread boundary poll.
        assert!(STALLED_CYCLE_RESOLVE_CONFIRM_TICKS > 0);
    }

    #[test]
    fn save_is_atomic_leaves_no_temp_residue() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let cycles_dir = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let entry_count = || fs::read_dir(&cycles_dir).unwrap().count();

        let _ = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(
            entry_count(),
            1,
            "exactly one cycle file after the first atomic save"
        );

        let state = mark_committed(&doc, "evt", Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert_eq!(
            entry_count(),
            1,
            "atomic overwrite leaves no temp-file residue"
        );
        assert_eq!(load(&doc).unwrap().unwrap().phase, CyclePhase::Committed);
    }

    #[test]
    fn start_preflight_persists_open_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::PreflightStarted);
        assert_eq!(
            load(&doc).unwrap().unwrap().phase,
            CyclePhase::PreflightStarted
        );
        assert!(load(&doc).unwrap().unwrap().is_open());
    }

    #[test]
    fn admit_with_current_resolver_opens_cycle_without_preflight_maintenance() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let original = "# Session\n\nOperator prompt.\n";
        fs::write(&doc, original).unwrap();
        let mut logs = Vec::new();

        let output = admit_with_current_resolver(
            &doc,
            |file| std::fs::read_to_string(file).context("test resolver should read document"),
            |_file| Ok(None),
            |file, message| logs.push((file.display().to_string(), message.to_string())),
        )
        .unwrap();

        assert!(output.admitted);
        assert_eq!(output.source, "admit");
        assert!(!output.maintenance_required);
        assert!(!output.preflight_required);
        assert_eq!(output.cycle_phase, "preflight_started");
        assert_eq!(fs::read_to_string(&doc).unwrap(), original);

        let state = load(&doc).unwrap().unwrap();
        assert_eq!(state.cycle_id, output.cycle_id);
        assert_eq!(state.phase, CyclePhase::PreflightStarted);
        assert_eq!(state.last_event, "preflight_started");

        assert_eq!(logs.len(), 1);
        let log = &logs[0].1;
        assert!(log.contains("realtime_admit"), "admission log:\n{log}");
        assert!(
            log.contains("maintenance_required=false"),
            "admission log:\n{log}"
        );
        assert!(
            log.contains("preflight_required=false"),
            "admission log:\n{log}"
        );
        assert!(
            !log.contains("preflight_diff_start"),
            "admit must not run preflight diff start:\n{log}"
        );
        assert!(
            !log.contains("deprecated_queue_active_line_dropped"),
            "admit must not run queue maintenance:\n{log}"
        );
        assert!(
            !log.contains("layout repair"),
            "admit must not run preflight layout repair:\n{log}"
        );
    }

    fn ack(
        component: &str,
        id: &str,
        reason: agent_doc_merge::document_cell_merge::AckReason,
    ) -> agent_doc_merge::document_cell_merge::AckRequest {
        agent_doc_merge::document_cell_merge::AckRequest {
            component: component.to_string(),
            id: id.to_string(),
            reason,
            detail: format!("{component}:{id} detail"),
        }
    }

    #[test]
    fn record_semantic_merge_acks_tags_current_cycle_and_dedupes() {
        use agent_doc_merge::document_cell_merge::AckReason;
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        record_semantic_merge_acks(
            &doc,
            &[ack("exchange", "a", AckReason::SameNodeOperatorOverride)],
        )
        .unwrap();
        // Re-recording the same (component, id, reason) is a no-op.
        record_semantic_merge_acks(
            &doc,
            &[ack("exchange", "a", AckReason::SameNodeOperatorOverride)],
        )
        .unwrap();

        let state = load(&doc).unwrap().unwrap();
        assert_eq!(state.pending_semantic_merge_acks.len(), 1);
        let recorded = &state.pending_semantic_merge_acks[0];
        assert_eq!(recorded.component, "exchange");
        assert_eq!(recorded.id, "a");
        assert_eq!(recorded.reason, "same_node_operator_override");
        assert_eq!(
            recorded.recorded_cycle_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert!(
            !recorded.surfaced,
            "freshly recorded ack is not yet surfaced"
        );
    }

    #[test]
    fn corrupt_cycle_sidecar_is_treated_as_absent_not_a_hot_path_error() {
        // `#lzsidecaratomic`: a corrupt/torn/legacy-format cycle-state sidecar must
        // never fail a hot/critical-path read. `load` treats it as absent, and
        // `load_with_closeout_projection` (used across commit/capture/write) does the
        // same instead of propagating a serde parse error.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle sidecar path");
        // Overwrite the durable sidecar with unparseable bytes.
        fs::write(&sidecar_path, "{ this is not valid cycle-state json").unwrap();

        assert!(
            load(&doc).unwrap().is_none(),
            "corrupt sidecar must read as absent, not error"
        );
        assert!(
            load_with_closeout_projection(&doc).unwrap().is_none(),
            "projection-backed read must also degrade to absent on a corrupt sidecar"
        );
    }

    #[test]
    fn document_cell_merge_acks_survive_missing_cycle_sidecar_via_projection() {
        use agent_doc_merge::document_cell_merge::AckReason;
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        record_semantic_merge_acks(
            &doc,
            &[ack("exchange", "a", AckReason::SameNodeOperatorOverride)],
        )
        .unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle sidecar path");
        fs::remove_file(&sidecar_path).unwrap();
        assert!(load(&doc).unwrap().is_none());

        let pending = load_pending_semantic_merge_acks(&doc).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].surfaced);

        let cycle2 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(cycle2.pending_semantic_merge_acks.len(), 1);
        assert!(cycle2.pending_semantic_merge_acks[0].surfaced);

        fs::remove_file(&sidecar_path).unwrap();
        let surfaced = load_pending_semantic_merge_acks(&doc).unwrap();
        assert_eq!(surfaced.len(), 1);
        assert!(surfaced[0].surfaced);

        let cycle3 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert!(cycle3.pending_semantic_merge_acks.is_empty());
    }

    #[test]
    fn start_preflight_carries_prior_cycle_acks_forward_exactly_once() {
        use agent_doc_merge::document_cell_merge::AckReason;
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        // Cycle 1: converge records an ack.
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        record_semantic_merge_acks(
            &doc,
            &[ack(
                "exchange",
                "a",
                AckReason::OperatorDeletedAgentEditedNode,
            )],
        )
        .unwrap();

        // Cycle 2: start_preflight must carry the ack forward so it is surfaced.
        let cycle2 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(
            cycle2.pending_semantic_merge_acks.len(),
            1,
            "ack carried into the immediately-following cycle"
        );
        assert!(
            cycle2.pending_semantic_merge_acks[0].surfaced,
            "carried ack is marked surfaced"
        );

        // Cycle 3: the ack was already surfaced in cycle 2, so it must drop.
        let cycle3 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert!(
            cycle3.pending_semantic_merge_acks.is_empty(),
            "ack surfaced once then dropped: {:?}",
            cycle3.pending_semantic_merge_acks
        );
    }

    #[test]
    fn document_cell_merge_ack_recorded_after_carry_chains_to_next_cycle() {
        use agent_doc_merge::document_cell_merge::AckReason;
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        // Cycle 1 records ack A.
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        record_semantic_merge_acks(
            &doc,
            &[ack("exchange", "a", AckReason::SameNodeOperatorOverride)],
        )
        .unwrap();

        // Cycle 2 surfaces A (carried) AND its own convergence records ack B.
        let cycle2 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(cycle2.pending_semantic_merge_acks.len(), 1);
        record_semantic_merge_acks(
            &doc,
            &[ack(
                "exchange",
                "b",
                AckReason::OperatorRevivedAgentDeletedNode,
            )],
        )
        .unwrap();

        // Cycle 3 surfaces only B (A was surfaced in cycle 2 and dropped).
        let cycle3 = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(cycle3.pending_semantic_merge_acks.len(), 1);
        assert_eq!(cycle3.pending_semantic_merge_acks[0].id, "b");
    }

    #[test]
    fn mark_write_applied_advances_existing_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert_eq!(state.last_event, "write_template");
        assert!(state.snapshot_hash.is_some());
    }

    #[test]
    fn record_ipc_snapshot_adoption_blocked_sets_cycle_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_ipc_snapshot_adoption_blocked(&doc)
            .unwrap()
            .expect("state should exist");

        assert!(state.ipc_snapshot_adoption_blocked);
        assert!(
            load(&doc)
                .unwrap()
                .expect("state should persist")
                .ipc_snapshot_adoption_blocked
        );
    }

    #[test]
    fn record_and_clear_dropped_exchange_prompts() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        record_dropped_exchange_prompts(&doc, &["go".to_string(), "go".to_string()]).unwrap();
        record_dropped_exchange_prompts(&doc, &["go".to_string(), "do #x".to_string()]).unwrap();
        let state = load(&doc).unwrap().expect("state");
        // De-duplicated across calls.
        assert_eq!(state.dropped_exchange_prompts, vec!["go", "do #x"]);

        clear_dropped_exchange_prompts(&doc).unwrap();
        assert!(
            load(&doc)
                .unwrap()
                .expect("state")
                .dropped_exchange_prompts
                .is_empty()
        );
    }

    #[test]
    fn mark_response_captured_sets_capture_metadata() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "abc",
            None,
        )
        .unwrap();
        assert_eq!(state.phase, CyclePhase::ResponseCaptured);
        assert_eq!(state.capture_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(state.response_sha256.as_deref(), Some("abc"));
    }

    #[test]
    fn mark_pending_mutations_sets_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_pending_mutations(&doc).unwrap().unwrap();
        assert!(state.had_pending_mutations);
        assert!(load(&doc).unwrap().unwrap().had_pending_mutations);
    }

    #[test]
    fn record_pending_done_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_pending_done_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_pending_kept_open_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_pending_kept_open_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.pending_kept_open_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().pending_kept_open_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_reaped_pending_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_reaped_pending_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.reaped_pending_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().reaped_pending_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_backlog_capture_requirement_sets_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_backlog_capture_requirement(&doc, true)
            .unwrap()
            .unwrap();
        assert!(state.requires_backlog_capture);
        assert!(load(&doc).unwrap().unwrap().requires_backlog_capture);
    }

    #[test]
    fn record_backlog_target_requirements_persists_targets() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let requirements = vec![BacklogTargetRequirement {
            path: dir.path().join("tasks/bugs.md").display().to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("abc".to_string()),
            baseline_item_ids: vec!["bug1".to_string()],
        }];

        let state = record_backlog_target_requirements(&doc, &requirements)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_backlog_targets, requirements);
        assert_eq!(
            load(&doc).unwrap().unwrap().required_backlog_targets,
            requirements
        );
    }

    #[test]
    fn record_required_explicit_backlog_item_count_persists_count() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_required_explicit_backlog_item_count(&doc, 3)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_explicit_backlog_item_count, 3);
        assert_eq!(
            load(&doc)
                .unwrap()
                .unwrap()
                .required_explicit_backlog_item_count,
            3
        );
    }

    #[test]
    fn record_required_plan_reference_count_persists_count() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_required_plan_reference_count(&doc, 2)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_plan_reference_count, 2);
        assert_eq!(
            load(&doc).unwrap().unwrap().required_plan_reference_count,
            2
        );
    }

    #[test]
    fn mark_committed_closes_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();

        let state = mark_committed(&doc, "commit", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert!(!state.is_open());
    }

    #[test]
    fn cycle_transitions_feed_state_backbone_closeout_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "response-sha",
            None,
        )
        .unwrap();
        mark_write_applied(&doc, "write_applied", Some("written"), Some("written")).unwrap();
        mark_committed(&doc, "commit_success", Some("written"), Some("written")).unwrap();

        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let mut ledger = agent_doc_state_backbone::EventLedger::new();
        for row in agent_doc_sqlite::state_store::load_state_events_from_db(&conn, None).unwrap() {
            let event: agent_doc_state_backbone::StateEvent =
                serde_json::from_str(&row.payload_json).unwrap();
            ledger.append(event);
        }
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let closeout = ledger
            .project_document(&document_hash)
            .expect("cycle transition events should project document state")
            .closeout;
        assert_eq!(
            closeout.cycle_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert_eq!(closeout.phase, Some(CyclePhase::Committed));
        assert_eq!(
            closeout.capture_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert_eq!(closeout.response_sha256.as_deref(), Some("response-sha"));
        assert!(
            closeout
                .commit
                .as_deref()
                .is_some_and(|commit| commit.starts_with("content:"))
        );
    }

    #[test]
    fn preflight_tracks_pending_maintenance_need_in_closeout_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, content).unwrap();

        let started = start_preflight(&doc, Some(content), Some(content)).unwrap();
        assert_eq!(
            started.tracked_work_maintenance_required_at_preflight,
            Some(true)
        );

        let projected = load_closeout_projection(&doc)
            .unwrap()
            .expect("preflight should feed closeout projection");
        assert_eq!(projected.tracked_work_maintenance_required, Some(true));

        let loaded = load_with_closeout_projection(&doc)
            .unwrap()
            .expect("cycle state should load");
        assert_eq!(
            loaded.tracked_work_maintenance_required_at_preflight,
            Some(true)
        );
    }

    #[test]
    fn response_capture_body_feeds_closeout_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "response-sha",
            None,
        )
        .unwrap();

        append_response_captured_body(
            &doc,
            CapturedResponseFactInput {
                cycle_id: &started.cycle_id,
                capture_id: &started.cycle_id,
                response_sha256: "response-sha",
                response_body: "### Re: topic - gpt-5\n\nDone.\n",
                file_hash: Some("file-sha"),
                snapshot_hash: Some("snapshot-sha"),
                baseline_content: Some("body"),
            },
        )
        .unwrap();

        let projected = load_projected_captured_response(&doc, &started.cycle_id)
            .unwrap()
            .expect("captured response projection");
        assert_eq!(projected.cycle_id, started.cycle_id);
        assert_eq!(projected.capture_id, started.cycle_id);
        assert_eq!(projected.response_sha256, "response-sha");
        assert_eq!(projected.file_hash.as_deref(), Some("file-sha"));
        assert_eq!(projected.snapshot_hash.as_deref(), Some("snapshot-sha"));
        assert_eq!(projected.baseline_content.as_deref(), Some("body"));
        assert_eq!(projected.response_body, "### Re: topic - gpt-5\n\nDone.\n");
    }

    #[test]
    fn recent_response_capture_history_is_bounded_and_newest_first() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        for (response_sha256, response_body, baseline) in [
            ("response-1", "first response", "baseline-1"),
            ("response-2", "second response", "baseline-2"),
            ("response-3", "third response", "baseline-3"),
        ] {
            append_response_captured_body(
                &doc,
                CapturedResponseFactInput {
                    cycle_id: &started.cycle_id,
                    capture_id: &started.cycle_id,
                    response_sha256,
                    response_body,
                    file_hash: Some(response_sha256),
                    snapshot_hash: None,
                    baseline_content: Some(baseline),
                },
            )
            .unwrap();
        }

        let history = load_recent_captured_response_checkpoints(&doc, 2).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].response_sha256, "response-3");
        assert_eq!(history[0].baseline_content.as_deref(), Some("baseline-3"));
        assert_eq!(history[1].response_sha256, "response-2");
        assert!(history[0].sequence > history[1].sequence);
    }

    #[test]
    fn terminal_closeout_proof_feeds_state_backbone_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        append_terminal_closeout_proof(
            &doc,
            TerminalCloseoutProofInput {
                cycle_id: &started.cycle_id,
                last_event: "commit_success",
                did_commit: true,
                file_hash: "file-sha",
                snapshot_hash: "head-sha",
                head_hash: "head-sha",
                state_file_hash_matches: false,
                state_snapshot_hash_matches: true,
                agreement: "snapshot_head_visible_drift",
                capture_id: Some("capture-1"),
                response_sha256: Some("response-sha"),
                recorded_at_ms: 42,
            },
        )
        .unwrap();

        let proof = load_latest_terminal_closeout_proof(&doc)
            .unwrap()
            .expect("terminal closeout proof");
        assert_eq!(proof.cycle_id, started.cycle_id);
        assert_eq!(proof.file_hash, "file-sha");
        assert_eq!(proof.snapshot_hash, "head-sha");
        assert_eq!(proof.head_hash, "head-sha");
        assert_eq!(proof.agreement, "snapshot_head_visible_drift");
        assert_eq!(proof.capture_id.as_deref(), Some("capture-1"));
        assert_eq!(proof.response_sha256.as_deref(), Some("response-sha"));
        assert_eq!(proof.recorded_at_ms, 42);
    }

    #[test]
    fn closeout_recovery_evidence_feeds_state_backbone_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        append_closeout_recovery_evidence(
            &doc,
            CloseoutRecoveryEvidenceInput {
                visible_markdown_hash: "visible-sha",
                snapshot_hash: Some("snapshot-sha"),
                active_cycle_id: Some(&started.cycle_id),
                active_cycle_phase: Some(CyclePhase::ResponseCaptured),
                active_capture_id: Some("capture-1"),
                active_capture_cycle_id: Some(&started.cycle_id),
                active_capture_state: Some("captured"),
                active_capture_response_sha256: Some("response-sha"),
                response_body: agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::PresentInVisible {
                    capture_id: "capture-1".into(),
                },
                queue_only_drift: Some(
                    agent_doc_state_backbone::CloseoutRecoveryQueueOnlyDriftEvidence {
                        file_hash_mismatch: true,
                        snapshot_hash_mismatch: false,
                        proven_queue_only: true,
                    },
                ),
                snapshot_head_drift: Some(
                    agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::MetadataOnly,
                ),
                snapshot_visible_drift: Some(
                    agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::BoundaryOnly,
                ),
                editor_ipc: agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::FreshLiveBuffer {
                    live_buffer_count: 1,
                    socket_degraded: false,
                },
                binary_freshness: agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence::NoStaleWarning,
                recorded_at_ms: 42,
            },
        )
        .unwrap();

        let evidence = load_latest_closeout_recovery_evidence(&doc)
            .unwrap()
            .expect("closeout recovery evidence projection");
        assert_eq!(evidence.visible_markdown_hash, "visible-sha");
        assert_eq!(evidence.snapshot_hash.as_deref(), Some("snapshot-sha"));
        assert_eq!(
            evidence.active_cycle_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert_eq!(
            evidence.active_cycle_phase,
            Some(CyclePhase::ResponseCaptured)
        );
        assert_eq!(evidence.active_capture_id.as_deref(), Some("capture-1"));
        assert_eq!(
            evidence.active_capture_response_sha256.as_deref(),
            Some("response-sha")
        );
        assert_eq!(
            evidence
                .queue_only_drift
                .as_ref()
                .map(|drift| drift.proven_queue_only),
            Some(true)
        );
        assert_eq!(
            evidence.snapshot_head_drift,
            Some(agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::MetadataOnly)
        );
        assert_eq!(
            evidence.snapshot_visible_drift,
            Some(agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::BoundaryOnly)
        );
        assert_eq!(evidence.recorded_at_ms, 42);
    }

    #[test]
    fn load_closeout_projection_absent_store_does_not_create_state_db() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let state_db = agent_doc_sqlite::state_store::state_db_path(dir.path());
        assert!(!state_db.exists());

        assert_eq!(load_closeout_projection(&doc).unwrap(), None);
        assert!(
            !state_db.exists(),
            "read-only projection lookup must not initialize state.db"
        );
    }

    #[test]
    fn load_closeout_projection_replays_state_backbone_events() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "response-sha",
            None,
        )
        .unwrap();
        mark_write_applied(&doc, "write_applied", Some("written"), Some("written")).unwrap();
        mark_committed(&doc, "commit_success", Some("written"), Some("written")).unwrap();

        let projected = load_closeout_projection(&doc)
            .unwrap()
            .expect("closeout projection should replay from state backbone");
        assert_eq!(
            projected.cycle_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert_eq!(projected.phase, Some(CyclePhase::Committed));
        assert_eq!(
            projected.capture_id.as_deref(),
            Some(started.cycle_id.as_str())
        );
        assert_eq!(projected.response_sha256.as_deref(), Some("response-sha"));
        assert!(
            projected
                .commit
                .as_deref()
                .is_some_and(|commit| commit.starts_with("content:"))
        );
    }

    #[test]
    fn load_with_closeout_projection_overlays_stale_matching_sidecar_phase() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let opened = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle sidecar path");
        fs::write(sidecar_path, serde_json::to_string_pretty(&opened).unwrap()).unwrap();
        assert_eq!(
            load(&doc).unwrap().unwrap().phase,
            CyclePhase::PreflightStarted
        );

        let projected = load_with_closeout_projection(&doc)
            .unwrap()
            .expect("cycle state");
        assert_eq!(projected.cycle_id, opened.cycle_id);
        assert_eq!(projected.phase, CyclePhase::Committed);
        assert!(
            projected
                .last_event
                .contains("state_backbone_commit_observed")
        );
    }

    #[test]
    fn load_with_closeout_projection_preserves_noop_commit_event_on_matching_phase() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("body"), Some("body")).unwrap();
        mark_committed(&doc, "commit_already_current", Some("body"), Some("body")).unwrap();

        let projected = load_with_closeout_projection(&doc)
            .unwrap()
            .expect("cycle state");
        assert_eq!(projected.phase, CyclePhase::Committed);
        assert_eq!(projected.last_event, "commit_already_current");
    }

    #[test]
    fn committed_reentry_repairs_missing_terminal_closeout_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "response-sha",
            None,
        )
        .unwrap();
        mark_write_applied(&doc, "write_applied", Some("written"), Some("written")).unwrap();
        mark_committed(&doc, "commit_success", Some("written"), Some("written")).unwrap();

        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let deleted = conn
            .execute(
                "DELETE FROM state_events WHERE fact_type = 'commit_observed'",
                [],
            )
            .unwrap();
        assert_eq!(deleted, 1);

        mark_committed(&doc, "commit_success", Some("written"), Some("written")).unwrap();
        mark_committed(&doc, "commit_success", Some("written"), Some("written")).unwrap();

        let rows = agent_doc_sqlite::state_store::load_state_events_from_db(&conn, None).unwrap();
        assert_eq!(
            rows.iter()
                .filter(|row| row.fact_type == "commit_observed")
                .count(),
            1,
            "terminal re-entry should restore the missing commit fact exactly once"
        );
        let mut ledger = agent_doc_state_backbone::EventLedger::new();
        for row in rows {
            let event: agent_doc_state_backbone::StateEvent =
                serde_json::from_str(&row.payload_json).unwrap();
            ledger.append(event);
        }
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let closeout = ledger
            .project_document(&document_hash)
            .expect("repaired terminal event should project document state")
            .closeout;
        assert_eq!(closeout.phase, Some(CyclePhase::Committed));
        assert!(
            closeout
                .commit
                .as_deref()
                .is_some_and(|commit| commit.starts_with("content:"))
        );
    }

    #[test]
    fn mark_recycle_resume_consumed_latches_and_is_idempotent() {
        // `#midturn-recycle-resume` Phase B: the boot-resume consume latch flips once
        // and stays latched, so a second boot reading the same still-open checkpoint
        // does not re-dispatch the interrupted turn again.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        // Fresh open cycle: not yet consumed.
        let opened = load(&doc).unwrap().unwrap();
        assert!(opened.is_open());
        assert!(!opened.recycle_resume_consumed);

        let consumed = mark_recycle_resume_consumed(&doc).unwrap().unwrap();
        assert!(consumed.recycle_resume_consumed);
        assert!(consumed.is_open(), "consuming does not close the cycle");
        // Persisted.
        assert!(load(&doc).unwrap().unwrap().recycle_resume_consumed);

        // Idempotent: a second consume keeps the latch set (no panic, no reset).
        let again = mark_recycle_resume_consumed(&doc).unwrap().unwrap();
        assert!(again.recycle_resume_consumed);

        // No cycle state → Ok(None), never an error.
        let other = dir.path().join("other.md");
        fs::write(&other, "x").unwrap();
        assert!(mark_recycle_resume_consumed(&other).unwrap().is_none());
    }

    #[test]
    fn mark_committed_is_idempotent_for_terminal_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();
        let replay = mark_committed(&doc, "repair_applied", Some("new"), Some("new")).unwrap();

        assert_eq!(replay, committed);
        assert_eq!(load(&doc).unwrap().unwrap(), committed);
    }

    #[test]
    fn mark_committed_supersedes_abandoned_cycle_with_synthetic_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let abandoned =
            mark_abandoned(&doc, "stalled_preflight", Some("snap"), Some("body")).unwrap();

        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        assert_eq!(committed.phase, CyclePhase::Committed);
        assert_eq!(committed.last_event, "commit_success");
        assert_ne!(committed.cycle_id, abandoned.cycle_id);
        assert!(committed.cycle_id.starts_with("synthetic-"));
        assert_eq!(load(&doc).unwrap().unwrap(), committed);
    }

    #[test]
    fn abandoned_and_timeout_bookkeeping_do_not_reopen_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();
        let abandoned = mark_abandoned(&doc, "stale_empty", Some("new"), Some("new")).unwrap();
        let timeout = mark_recoverable_preflight_timeout(&doc, "recoverable_timeout")
            .unwrap()
            .unwrap();

        assert_eq!(abandoned, committed);
        assert_eq!(timeout, committed);
        assert_eq!(load(&doc).unwrap().unwrap(), committed);
    }

    #[test]
    fn mark_abandoned_closes_cycle_without_commit() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state =
            mark_abandoned(&doc, "abandon_empty_preflight", Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::Abandoned);
        assert_eq!(state.last_event, "abandon_empty_preflight");
        assert!(!state.is_open());
    }

    #[test]
    fn exact_false_stale_retirement_can_reactivate_same_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let captured = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "response-sha",
            Some(&started.cycle_id),
        )
        .unwrap();
        mark_abandoned(
            &doc,
            "repair_retire_superseded_captured_only_orphan",
            Some("snap"),
            Some("body"),
        )
        .unwrap();

        assert!(
            reactivate_false_stale_capture_retirement(
                &doc,
                captured.capture_id.as_deref().unwrap(),
                "response-sha",
            )
            .unwrap()
        );
        let reactivated = load(&doc).unwrap().unwrap();
        assert_eq!(reactivated.cycle_id, started.cycle_id);
        assert_eq!(reactivated.phase, CyclePhase::ResponseCaptured);
        assert_eq!(
            reactivated.last_event,
            "session_check_reactivated_false_stale_capture_retirement"
        );
        assert!(
            reactivate_false_stale_capture_retirement(
                &doc,
                captured.capture_id.as_deref().unwrap(),
                "response-sha",
            )
            .unwrap(),
            "recovery must remain idempotent after the cycle projection advances first"
        );
    }

    #[test]
    fn mark_write_applied_creates_synthetic_cycle_when_missing() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = mark_write_applied(&doc, "recover_apply", Some("body"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert!(state.cycle_id.starts_with("synthetic-"));
    }

    #[test]
    fn mark_write_applied_does_not_regress_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let state = mark_write_applied(&doc, "repair_applied", Some("new"), Some("new")).unwrap();
        let body_hash = agent_doc_hash::content_hash("body");
        assert_eq!(state.phase, CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        assert_eq!(state.snapshot_hash.as_deref(), Some(body_hash.as_str()));
        assert_eq!(state.file_hash.as_deref(), Some(body_hash.as_str()));
    }

    #[test]
    fn mark_response_captured_does_not_regress_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let state = mark_response_captured(
            &doc,
            "response_captured",
            Some("new"),
            Some("new"),
            "abc",
            Some(&committed.cycle_id),
        )
        .unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        assert_eq!(state.capture_id, committed.capture_id);
        assert_eq!(state.response_sha256, committed.response_sha256);
    }

    #[test]
    fn cycle_phase_transitions_append_to_session_log() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let doc_content = "---\nagent_doc_session: sess-123\n---\n\nbody\n";
        fs::write(&doc, doc_content).unwrap();

        let started = start_preflight(&doc, Some("snap"), Some(doc_content)).unwrap();
        let captured = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some(doc_content),
            "abc123",
            Some(&started.cycle_id),
        )
        .unwrap();
        let written =
            mark_write_applied(&doc, "write_template", Some(doc_content), Some(doc_content))
                .unwrap();
        let committed =
            mark_committed(&doc, "commit_success", Some(doc_content), Some(doc_content)).unwrap();

        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/sess-123.log")).unwrap();
        assert!(log.contains(&format!(
            "document_cycle phase=preflight_started cycle={} event=preflight_started",
            started.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=response_captured cycle={} event=response_captured capture_id={}",
            captured.cycle_id, captured.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=write_applied cycle={} event=write_template",
            written.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=committed cycle={} event=commit_success",
            committed.cycle_id
        )));
    }

    #[test]
    fn cycle_phase_transitions_skip_session_log_without_session_id() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body\n").unwrap();

        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        assert!(
            !dir.path().join(".agent-doc/logs").exists(),
            "plain documents without a session id should not create session logs"
        );
    }

    #[test]
    fn start_preflight_with_task_stores_queue_task_id_and_turn_id() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight_with_task(
            &doc,
            Some("snap"),
            Some("body"),
            Some("#reentrant-phase2"),
            Some("#reentrant-phase2"),
        )
        .unwrap();
        assert_eq!(state.queue_task_id.as_deref(), Some("#reentrant-phase2"));
        assert_eq!(state.turn_id.as_deref(), Some("#reentrant-phase2"));

        let loaded = load(&doc).unwrap().expect("state should persist");
        assert_eq!(loaded.queue_task_id, state.queue_task_id);
        assert_eq!(loaded.turn_id, state.turn_id);
    }

    #[test]
    fn record_turn_checkpoint_persists_resume_identity_and_baseline() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let prompts = vec![
            "  do [#DurableRecycle]  ".to_string(),
            "do [#DurableRecycle]".to_string(),
            "free text prompt".to_string(),
        ];
        let state = record_turn_checkpoint(
            &doc,
            Some("/tmp/baseline.md"),
            &prompts,
            Some("#DurableRecycle"),
            Some("#DurableRecycle"),
        )
        .unwrap()
        .expect("state should exist");

        assert_eq!(state.baseline_file.as_deref(), Some("/tmp/baseline.md"));
        assert_eq!(
            state.prompt_targets,
            vec![
                "do [#DurableRecycle]".to_string(),
                "free text prompt".to_string()
            ]
        );
        assert_eq!(state.queue_task_id.as_deref(), Some("#durablerecycle"));
        assert_eq!(state.turn_id.as_deref(), Some("#durablerecycle"));

        let loaded = load(&doc).unwrap().unwrap();
        assert_eq!(loaded.baseline_file, state.baseline_file);
        assert_eq!(loaded.prompt_targets, state.prompt_targets);
        assert_eq!(loaded.queue_task_id, state.queue_task_id);
        assert_eq!(loaded.turn_id, state.turn_id);
    }

    #[test]
    fn start_preflight_without_task_has_none_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert!(state.queue_task_id.is_none());
        assert!(state.turn_id.is_none());
    }

    #[test]
    fn to_pipeline_mirrors_cycle_state_fields_and_tracks_phase() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight_with_task(
            &doc,
            Some("snap"),
            Some("body"),
            Some("#fmrunid-wire"),
            Some("#fmrunid-wire"),
        )
        .unwrap();

        let pipeline = state.to_pipeline();
        assert_eq!(pipeline.run_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(pipeline.step.as_deref(), Some("preflight_started"));
        assert_eq!(pipeline.turn_id.as_deref(), Some("#fmrunid-wire"));
        assert_eq!(pipeline.queue_task_id.as_deref(), Some("#fmrunid-wire"));
        assert!(!pipeline.is_empty());

        // `step` follows the authoritative phase transition.
        let captured =
            mark_response_captured(&doc, "captured", Some("snap"), Some("body"), "abc", None)
                .unwrap();
        assert_eq!(
            captured.to_pipeline().step.as_deref(),
            Some("response_captured")
        );
        // run_id stays stable across transitions (same cycle).
        assert_eq!(captured.to_pipeline().run_id, pipeline.run_id);
    }
}
