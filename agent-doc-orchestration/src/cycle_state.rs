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
//!   synthetic committed cycle if commit happens without a prior state file).
//! - Lower-rank bookkeeping and duplicate terminal bookkeeping must never
//!   mutate an already-committed or abandoned cycle.
//! - Phase transitions are accepted through `cycle_state_machine`; the sidecar
//!   remains the durable crash-recovery log emitted after that transition table
//!   accepts an event.
//! - `load()` returns the current persisted state when present.
//!
//! ## Agentic Contracts
//! - State is per-document, never global across the repo.
//! - Writes are deterministic JSON file replacements.
//! - Missing project root or state file returns `Ok(None)`.
//! - `is_open()` is true for any phase except `Committed`.
//!
//! ## Evals
//! - `start_preflight_persists_open_cycle`
//! - `mark_response_captured_sets_capture_metadata`
//! - `mark_write_applied_advances_existing_cycle`
//! - `mark_committed_closes_cycle`
//! - `mark_write_applied_creates_synthetic_cycle_when_missing`

use agent_doc_turn::{CycleEvent, CyclePhaseMachine};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use agent_doc_turn::CyclePhase;

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

/// `#semmerge-ack-turn` (semantic_merge Phase 4): a node-keyed acknowledgement
/// that a node-disjoint semantic merge could NOT apply the agent's change
/// verbatim — the operator deleted an agent-edited node, overrode the same node,
/// or revived an agent-deleted node, and the operator value won in `merged_doc`.
/// The agent's content is never silently discarded: the next cycle surfaces these
/// so the agent emits an exchange turn acknowledging the non-applied change.
///
/// `reason` is the stable [`agent_doc_markdown_ast::semantic_merge::AckReason`]
/// token (see [`AckReason::token`](agent_doc_markdown_ast::semantic_merge::AckReason::token)).
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
    /// `#semmerge-ack-turn` (semantic_merge Phase 4): node-keyed acks carried into
    /// the NEXT cycle's response. Recorded at convergence time
    /// ([`record_semantic_merge_acks`]) and carried forward exactly one cycle by
    /// [`start_preflight_with_task`], which preflight surfaces as
    /// `semantic_merge_acks` so the agent emits an acknowledgement exchange turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_semantic_merge_acks: Vec<PendingSemanticMergeAck>,
    /// `#closeoutstall`: typed operator-gated closeout state. A response may be
    /// safely captured and queued for editor IPC while the live editor has not
    /// proven application. This keeps the cycle open and gives session-check,
    /// doctor, and hooks one canonical recovery surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_closeout: Option<BlockedCloseout>,
}

/// Extract the queue prompt head texts (e.g. `do [#convqa-rerun]`) from a
/// document's `agent:queue` component. Recorded at preflight so `session-check`
/// can detect unproved deletion of runnable heads (`#queue-clear-unrun-items`).
/// Freeform / non-prompt entries are skipped; id semantics are applied later by
/// the guard via `do_directive_target_ids`.
pub fn active_queue_directive_heads(doc: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let Ok(entries) = agent_doc_queue::document_queue::parse(queue.content(doc)) else {
        return Vec::new();
    };
    agent_doc_queue::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Extract free-text (non-`do [#id]`) queue prompt head texts from a
/// document's `agent:queue` component. These are recorded alongside
/// `active_queue_heads` so `session-check` can require committed-response /
/// consume / deferral proof for free-text queue heads that have no backlog id.
pub fn active_free_text_queue_heads(doc: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let Ok(entries) = agent_doc_queue::document_queue::parse(queue.content(doc)) else {
        return Vec::new();
    };
    agent_doc_queue::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.trim().to_string())
        .filter(|text| !text.is_empty() && !is_do_directive(text))
        .collect()
}

fn is_do_directive(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    lower.starts_with("do [#") || lower.starts_with("do #") || leads_with_bare_id_directive(&lower)
}

/// Optional-`do` grammar (Stage 1): a queue head that leads with a bare id token
/// (`[#id]` or `#id`) is id-backed — the `do` verb is optional. A trailing `:`
/// (`[#id]: note`) keeps the line inert as prose annotation rather than a
/// directive, matching the existing bracket-id prompt-guard convention. `text`
/// is expected to be lowercased and already stripped of list/checkbox markers
/// (queue prompt texts arrive that way).
fn leads_with_bare_id_directive(lower: &str) -> bool {
    let (rest, bracketed) = if let Some(r) = lower.strip_prefix("[#") {
        (r, true)
    } else if let Some(r) = lower.strip_prefix('#') {
        (r, false)
    } else {
        return false;
    };
    let id_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .count();
    if id_len == 0 {
        return false;
    }
    let after = &rest[id_len..];
    if bracketed {
        // Require a closing `]`; a trailing `:` marks prose, not a directive.
        match after.strip_prefix(']') {
            Some(tail) => !tail.starts_with(':'),
            None => false,
        }
    } else {
        // Bare `#id` must be a standalone leading token: end-of-line or a
        // whitespace / `.` separator (excludes `#id:` prose and `# heading`).
        after.is_empty() || after.starts_with([' ', '\t', '.'])
    }
}

/// `#suprecyclespin` — seconds an open cycle may sit untouched (no IPC ack
/// connection in flight) at a harness turn boundary before the supervisor
/// recycle/restart defer path force-closes it as abandoned. Generous enough never
/// to abandon a live `preflight → finalize` cycle (each phase ticks `updated_at`
/// and finalize holds IPC inflight), but bounded so a crashed/superseded older
/// turn cannot wedge the recycle in `DeferCycleOpen` (~2/sec `cycle_open`
/// spin-loop, `idle_watch.rs`) forever.
pub const STALLED_CYCLE_RESOLVE_SECS: u64 = 45;

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
            step: Some(cycle_phase_label(self.phase).to_string()),
            turn_id: self.turn_id.clone(),
            queue_task_id: self.queue_task_id.clone(),
        }
    }
}

pub fn load(file: &Path) -> Result<Option<CycleState>> {
    let Some(path) = state_path(file)? else {
        return Ok(None);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let state: CycleState = serde_json::from_str(&content)?;
    Ok(Some(state))
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
    let carried_semantic_merge_acks = load(file)
        .ok()
        .flatten()
        .map(|prior| {
            prior
                .pending_semantic_merge_acks
                .into_iter()
                .filter(|ack| !ack.surfaced)
                .map(|mut ack| {
                    ack.surfaced = true;
                    ack
                })
                .collect::<Vec<_>>()
        })
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
        snapshot_hash: snapshot_content.map(crate::ops_log::content_hash),
        file_hash: file_content.map(crate::ops_log::content_hash),
        normalized_snapshot_hash: snapshot_content.map(normalized_content_hash),
        normalized_file_hash: file_content.map(normalized_content_hash),
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
        ipc_snapshot_adoption_blocked: false,
        dropped_exchange_prompts: Vec::new(),
        dropped_queue_prompts: Vec::new(),
        active_queue_heads: file_content
            .map(active_queue_directive_heads)
            .unwrap_or_default(),
        active_free_text_queue_heads: file_content
            .map(active_free_text_queue_heads)
            .unwrap_or_default(),
        pending_semantic_merge_acks: carried_semantic_merge_acks,
        blocked_closeout: None,
    };
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
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
    for head in active_queue_directive_heads(doc) {
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
    let normalized_prompt_targets = normalize_text_list(prompt_targets);
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
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
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
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    state.capture_id = Some(state.cycle_id.clone());
    state.response_sha256 = Some(response_sha256.to_string());
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    // NB: intentionally do NOT mirror the pipeline block here. `response_captured`
    // can fire mid-stream (partial checkpoints), and a naive read-modify-write of
    // the document would race the streaming write-back loop and clobber it. The
    // mirror runs at `write_applied` instead — once the response is fully on disk
    // — which covers the same recovery window without the race (#22a8).
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
    append_phase_event_to_session_log(file, &state);
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
    append_phase_event_to_session_log(file, &state);
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

/// `#semmerge-ack-turn` (semantic_merge Phase 4): record node-keyed acks emitted
/// by the convergence semantic merge so the NEXT cycle can acknowledge the
/// non-applied agent change in an exchange turn. Tags each ack with the current
/// cycle id ([`start_preflight_with_task`] carries it forward exactly one cycle).
/// Appends only previously-unseen `(component, id, reason)` triples.
pub fn record_semantic_merge_acks(
    file: &Path,
    acks: &[agent_doc_markdown_ast::semantic_merge::AckRequest],
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    let cycle_id = state.cycle_id.clone();
    let mut changed = false;
    for ack in acks {
        let reason = ack.reason.token().to_string();
        if !state.pending_semantic_merge_acks.iter().any(|existing| {
            existing.component == ack.component
                && existing.id == ack.id
                && existing.reason == reason
        }) {
            state
                .pending_semantic_merge_acks
                .push(PendingSemanticMergeAck {
                    component: ack.component.clone(),
                    id: ack.id.clone(),
                    reason,
                    detail: ack.detail.clone(),
                    recorded_cycle_id: Some(cycle_id.clone()),
                    surfaced: false,
                });
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
    let recovery_command = format!("agent-doc write --commit {}", file.display());
    let blocked = BlockedCloseout {
        kind: "editor_convergence_required".to_string(),
        reason: reason.to_string(),
        source: source.to_string(),
        patch_id: patch_id.map(str::to_string),
        recovery: Some("retry_without_disk_write".to_string()),
        recovery_command: Some(recovery_command),
        detail: detail.map(str::to_string),
    };
    if state.blocked_closeout.as_ref() != Some(&blocked) {
        state.blocked_closeout = Some(blocked);
        state.updated_at = now_secs();
        save(file, &state)?;
        append_phase_event_to_session_log(file, &state);
    }
    Ok(Some(state))
}

pub fn mark_committed(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::WriteApplied));
    if matches!(state.phase, CyclePhase::Committed)
        && (state.last_event == event || is_stable_commit_event(&state.last_event))
    {
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
        state.snapshot_hash = Some(crate::ops_log::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(normalized_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state.normalized_file_hash = Some(normalized_content_hash(content));
    }
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    clear_pipeline_frontmatter(file);
    Ok(state)
}

/// #22a8 (Phase 5b write-side): mirror the live cycle phase into the session
/// document's `agent_doc_pipeline:` frontmatter block so any later invocation or
/// editor can read where the pipeline is without parsing the sidecar JSON.
///
/// Best-effort and non-fatal — a mirror failure is logged but never breaks the
/// cycle. The write is byte-precise (only the managed block changes) and every
/// doc-baseline comparison strips that block (diff layer + capture replay +
/// session_check), so the mirror is invisible to change detection / replay /
/// patchback guards.
///
/// Called from the write-path (`write.rs`) after the response is fully on disk —
/// NOT from `mark_write_applied`, so a direct state-transition (e.g. repair unit
/// tests) does not mutate the document, and the streaming write-back loop is
/// never raced.
pub(crate) fn mirror_pipeline_frontmatter(file: &Path, state: &CycleState) {
    if let Err(e) = (|| -> Result<()> {
        let content = std::fs::read_to_string(file)?;
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &state.to_pipeline(),
        )?;
        if updated != content {
            // `#fcc0`: converge the frontmatter mirror through the editor IPC when a
            // live JB listener is active so the post-response mirror write never
            // raises a `File Cache Conflict`; plain disk write otherwise.
            crate::write::converge_or_disk_write(file, &content, &updated, "pipeline_mirror")?;
        }
        Ok(())
    })() {
        crate::ops_log::log_op(
            file,
            &format!("pipeline_mirror_failed file={} err={}", file.display(), e),
        );
    }
}

/// #22a8 (Phase 5b write-side): clear the `agent_doc_pipeline:` block at a
/// terminal phase — the default auto-delete-when-the-turn-ends lifecycle — so a
/// drained cycle leaves no stale tracker behind. Best-effort and non-fatal; only
/// writes when a block is actually present so the frontmatter is otherwise
/// untouched.
fn clear_pipeline_frontmatter(file: &Path) {
    if let Err(e) = (|| -> Result<()> {
        let content = std::fs::read_to_string(file)?;
        if !content.contains("agent_doc_pipeline:") {
            return Ok(());
        }
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &Default::default(),
        )?;
        if updated != content {
            // `#fcc0`: converge the frontmatter clear through the editor IPC when a
            // live JB listener is active (no `File Cache Conflict`); plain disk
            // write otherwise.
            crate::write::converge_or_disk_write(file, &content, &updated, "pipeline_clear")?;
        }
        Ok(())
    })() {
        crate::ops_log::log_op(
            file,
            &format!("pipeline_clear_failed file={} err={}", file.display(), e),
        );
    }
}

pub fn mark_abandoned(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
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
        state.snapshot_hash = Some(crate::ops_log::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(normalized_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state.normalized_file_hash = Some(normalized_content_hash(content));
    }
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    clear_pipeline_frontmatter(file);
    Ok(state)
}

fn append_phase_event_to_session_log(file: &Path, state: &CycleState) {
    let Ok(content) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(&content) else {
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
        cycle_phase_label(state.phase),
        state.cycle_id,
        state.last_event
    );
    if let Some(capture_id) = state.capture_id.as_deref() {
        event.push_str(&format!(" capture_id={capture_id}"));
    }
    let _ = crate::startup_miss::append_session_log_event(file, session_id, &event);
}

fn save(file: &Path, state: &CycleState) -> Result<()> {
    let Some(path) = state_path(file)? else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(state)?;
    write_atomic(&path, &json)?;
    Ok(())
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

fn state_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(&canonical)?;
    Ok(Some(
        root.join(".agent-doc/state/cycles")
            .join(format!("{hash}.json")),
    ))
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
        ipc_snapshot_adoption_blocked: false,
        dropped_exchange_prompts: Vec::new(),
        dropped_queue_prompts: Vec::new(),
        active_queue_heads: Vec::new(),
        active_free_text_queue_heads: Vec::new(),
        pending_semantic_merge_acks: Vec::new(),
        blocked_closeout: None,
    }
}

pub fn cycle_phase_label(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::PreflightStarted => "preflight_started",
        CyclePhase::ResponseCaptured => "response_captured",
        CyclePhase::WriteApplied => "write_applied",
        CyclePhase::Committed => "committed",
        CyclePhase::Abandoned => "abandoned",
    }
}

fn is_stable_commit_event(event: &str) -> bool {
    matches!(
        event,
        "commit" | "commit_success" | "commit_already_current"
    )
}

/// A no-op commit event: the closeout commit found the snapshot already equal to
/// `HEAD`, so this cycle committed *no* new binary-owned work this turn. Distinct
/// from `commit` / `commit_success`, which committed real content. Used by
/// closeout guards that must not treat a no-op commit as committed-work-without-a-response.
pub fn is_noop_commit_event(event: &str) -> bool {
    event == "commit_already_current"
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn normalized_content_hash(content: &str) -> String {
    // Neutralizes transient markers AND the independently-maintained agent:queue
    // component so response-replay / stale-lock recovery stays stable across
    // queue-maintenance churn (#adoc-queue-ipc-buffer-divergence #4). Must match
    // repair.rs's compare-side normalization exactly.
    crate::ops_log::content_hash(&crate::git::normalize_for_replay_hash(content))
}

fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

fn normalize_checkpoint_task_id(id: &str) -> String {
    let normalized = normalize_pending_id(id);
    if normalized.is_empty() {
        String::new()
    } else {
        format!("#{normalized}")
    }
}

fn normalize_text_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .fold(Vec::new(), |mut acc, value| {
            if !acc.iter().any(|existing| existing == &value) {
                acc.push(value);
            }
            acc
        })
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

    #[test]
    fn is_do_directive_accepts_do_and_bare_id_forms() {
        // Back-compat: explicit `do` forms.
        assert!(is_do_directive("do [#opt]"));
        assert!(is_do_directive("do #opt"));
        assert!(is_do_directive("DO [#opt]. trailing note"));
        // Optional-`do` Stage 1: bare id token is id-backed.
        assert!(is_do_directive("[#opt]"));
        assert!(is_do_directive("[#opt]. do the small fix"));
        assert!(is_do_directive("#opt"));
        assert!(is_do_directive("#opt do the thing"));
        // Inert: prose annotation (`[#id]:`), references, plain prose, headings.
        assert!(!is_do_directive("[#opt]: just a note"));
        assert!(!is_do_directive("#opt: just a note"));
        assert!(!is_do_directive("re [#opt]"));
        assert!(!is_do_directive("see [#opt] for context"));
        assert!(!is_do_directive("# heading"));
        assert!(!is_do_directive("just a free-text prompt"));
    }

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn open_stalled_resolves_only_abandoned_older_turns() {
        // `#suprecyclespin`: the recycle-defer gate must distinguish a live cycle
        // (keep deferring) from a stalled/superseded older turn (force-close).
        let mut state = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::ResponseCaptured);
        let deadline = STALLED_CYCLE_RESOLVE_SECS;
        // updated_at far in the past, no IPC inflight → stalled (resolvable).
        state.updated_at = 1_000;
        assert!(state.open_stalled(0, 1_000 + deadline + 1, deadline));
        // Exactly at the deadline is NOT yet stalled (strict `>`).
        assert!(!state.open_stalled(0, 1_000 + deadline, deadline));
        // Fresh `updated_at` → a live cycle, never force-closed.
        state.updated_at = 1_000 + deadline + 1;
        assert!(!state.open_stalled(0, 1_000 + deadline + 5, deadline));
        // IPC ack connection in flight → finalize is live, never force-closed even
        // if the cycle has been open a long time.
        state.updated_at = 1_000;
        assert!(!state.open_stalled(1, 1_000 + deadline + 100, deadline));
        // A committed cycle is not open, so it is never "stalled".
        let committed = synthetic_state(Path::new("/tmp/doc.md"), CyclePhase::Committed);
        assert!(!committed.open_stalled(0, u64::MAX, deadline));
    }

    #[test]
    fn save_is_atomic_leaves_no_temp_residue() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let cycles_dir = state_path(&doc)
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

    fn ack(
        component: &str,
        id: &str,
        reason: agent_doc_markdown_ast::semantic_merge::AckReason,
    ) -> agent_doc_markdown_ast::semantic_merge::AckRequest {
        agent_doc_markdown_ast::semantic_merge::AckRequest {
            component: component.to_string(),
            id: id.to_string(),
            reason,
            detail: format!("{component}:{id} detail"),
        }
    }

    #[test]
    fn record_semantic_merge_acks_tags_current_cycle_and_dedupes() {
        use agent_doc_markdown_ast::semantic_merge::AckReason;
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
    fn start_preflight_carries_prior_cycle_acks_forward_exactly_once() {
        use agent_doc_markdown_ast::semantic_merge::AckReason;
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
    fn semantic_merge_ack_recorded_after_carry_chains_to_next_cycle() {
        use agent_doc_markdown_ast::semantic_merge::AckReason;
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
        let body_hash = crate::ops_log::content_hash("body");
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
        fs::write(&doc, "---\nagent_doc_session: sess-123\n---\n\nbody\n").unwrap();

        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let captured = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "abc123",
            Some(&started.cycle_id),
        )
        .unwrap();
        let written =
            mark_write_applied(&doc, "write_template", Some("body"), Some("body")).unwrap();
        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

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
