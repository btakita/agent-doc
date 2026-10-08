//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` reduces the `state.db` closeout projection and exits nonzero
//!   when the most recent cycle is still open (`preflight_started`,
//!   `response_captured`, or `write_applied`).
//! - Missing or corrupt ledger state fails closed. It does not elect an ops log
//!   or filesystem cycle record as phase authority.
//! - Distinguishes "cycle started but no write/commit followed" from
//!   "response write landed but no commit followed" in both cycle-state and
//!   ops-log fallback paths.
//! - When an open `preflight_started` cycle already has a visible response
//!   patchback in the working tree, reports that manual-repair / commit-boundary
//!   state explicitly instead of collapsing it into the generic open-cycle
//!   message.
//! - Also fails closed when the current document diverges from its snapshot in
//!   a way that looks like a direct assistant patchback (`### Re:` or
//!   `## Assistant`) without a corresponding `agent-doc` cycle.
//! - Also fails closed when the current document already has unresolved
//!   prompt-bearing user edits (`prompt_target`) relative to the snapshot, but
//!   no new `agent-doc` cycle ever started for them. Plain exchange-only
//!   content edits without a fresh prompt target do not reopen a committed
//!   cycle. `agent:queue` prompt edits are excluded from this guard because
//!   queue activation and consumption are owned by the next preflight cycle.
//! - Also fails closed when a closed cycle leaves the live `agent:exchange` tail
//!   ending in a prompt-looking block with no later assistant response. This
//!   catches direct-harness turns where the prompt was already committed into
//!   the snapshot/baseline but the final response patchback never happened.
//! - When that bypassed patchback also leaves prompt-target lines in the same
//!   diff without the binary-owned `❯ ` transcript prefix, `session-check`
//!   reports the bare prompt target in the failure marker so the write path can
//!   be repaired instead of silently accepted.
//! - Narrow self-heal: when that drift is already committed in `HEAD` and the
//!   current working tree matches `HEAD` modulo transient boundary / `(HEAD)`
//!   markers, `session-check` repairs the stale snapshot instead of reporting
//!   a fresh interruption forever. When it applies a proven lossless
//!   response-replay canonicalization, it must also commit those exact bytes
//!   through a private index before reporting `OK`; a process restart between
//!   repair and commit resumes that same exact-only settlement from `HEAD`.
//! - Exit 0 when the current cycle state is committed, when state/log files
//!   are missing, or when the fallback `ops.log` event is terminal and no
//!   likely bypassed patchback is present.
//! - Exit 1 when the current cycle state is still open, when the fallback last
//!   `ops.log` event is `preflight_diff_start`, when a likely direct
//!   assistant patchback bypassed `agent-doc write` / `finalize`, or when
//!   the cycle state says `committed` but the snapshot does not match HEAD
//!   in the owning git root (response patchback visible but never committed).
//! - Exit 2 on unexpected I/O errors.
//!
//! ## Agentic Contracts
//! - May also clear a persisted startup-miss marker when the marker is proven
//!   stale because a later registered session start has already superseded it.
//! - Otherwise mutates only the snapshot in the narrow committed-historical-drift
//!   repair case above, or the exact session document and its checkpoint while
//!   settling a proven response-replay canonicalization. Unrelated index and
//!   working-tree content is never included.
//! - Called by supervisors / watchdogs (and directly from skill) to
//!   detect the "started but never wrote" invariant violation flagged
//!   as bug #a011.
//!
//! ## Evals
//! - `session_check_empty_log_exits_zero`
//! - `session_check_open_cycle_state_exits_one`
//! - `session_check_committed_cycle_state_exits_zero`
//! - `detect_bypassed_response_write_flags_template_heading`
//! - `detect_bypassed_response_write_flags_inline_assistant_heading`
//! - `detect_bypassed_response_write_ignores_plain_user_prompt`
//! - `session_check_repairs_committed_historical_snapshot_drift`
//! - `session_check_missing_log_exits_zero`
//! - `session_check_snapshot_committed_guard_fails_when_snapshot_differs`
//! - `session_check_snapshot_committed_guard_passes_when_committed`
//! - `session_check_rejects_crdt_disk_divergence_after_committed_closeout`

use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_state_scope::LocalReadScope;
use agent_doc_turn::CyclePhase;
use agent_doc_turn::op_log::{
    OpsLogEvent, PREFLIGHT_START_EVENT, event_name, is_write_completed_commit_missing_event,
};
use agent_doc_workflow::session_check::{BlockedCloseoutMessage, GuardResult};
use anyhow::{Context, Result};
use lazily::{Computed, Source};
use std::collections::BTreeSet;
use std::path::Path;

use crate::{
    detect_jb_cache_conflict_accept_duplicate_replay,
    detect_jb_cache_conflict_cancel_recoverable_with_context,
    detect_late_ipc_response_overapplication, detect_unstarted_prompt_bearing_diff,
    operator_live_buffer_contains_heading,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    /// `#steerinterruptexit`: the last cycle committed cleanly and the operator
    /// has since added steering no cycle has answered yet. That is pending
    /// work, not a failed closeout: the CLI prints it and exits `0`, the
    /// embedded closeout (`respond` / `finalize` / `write --commit`) succeeds,
    /// and the message carries every item verbatim with its `dispatch`. It
    /// starts with [`agent_doc_turn::response_text::SESSION_CHECK_STEERING_PENDING_PREFIX`].
    SteeringPending(String),
    Interrupted(String),
}

impl SessionCheckStatus {
    /// Whether this outcome fails the turn boundary. Pending steering does not.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Interrupted(_))
    }

    /// The status line, whichever outcome it is.
    pub fn message(&self) -> &str {
        match self {
            Self::Ok(message) | Self::SteeringPending(message) | Self::Interrupted(message) => {
                message
            }
        }
    }
}

pub struct SessionCheckReport {
    pub status: SessionCheckStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedFinalizeResumeOutcome {
    NotApplicable,
    Committed,
    Superseded,
    Retained { reason: String },
    RetryAt { reason: String, retry_at_secs: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyTerminalProjectionDecision {
    Converged,
    AwaitEditorDelivery,
    RequestNativeEditorSave,
    ObserveOnly,
}

/// A direct `session-check` remains unable to replay, repair, or commit document
/// content. It may, however, ask the live editor to save its own authoritative
/// buffer when a durable `write_applied` closeout is blocked solely on the disk
/// projection. This is an editor-owned persistence effect, not a competing
/// document replacement.
pub fn decide_read_only_terminal_projection(
    authority_matches_required_scope: bool,
    cycle_phase: Option<CyclePhase>,
    retained_document_write_blocks: bool,
    editor_delivery_converged: bool,
) -> ReadOnlyTerminalProjectionDecision {
    if authority_matches_required_scope {
        return ReadOnlyTerminalProjectionDecision::Converged;
    }
    if retained_document_write_blocks && cycle_phase == Some(CyclePhase::WriteApplied) {
        if !editor_delivery_converged {
            return ReadOnlyTerminalProjectionDecision::AwaitEditorDelivery;
        }
        return ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave;
    }
    // `#unowneddivergence`: a divergence with no durable owner — the cycle is
    // terminal and no retained write blocks — cannot be waited out. Nothing
    // holds the write, so no controller state edge will ever fire, and
    // `ObserveOnly` turns that into a permanent INTERRUPT that stops the queue
    // drain and strands the visible edits (observed twice on
    // tasks/software/lazily.md, 2026-08-03). Asking the live editor to persist
    // its own buffer is the same editor-owned effect the retained path already
    // uses — never a competing document replacement — and it is the only action
    // that can converge this state without the agent writing the document.
    if !retained_document_write_blocks && cycle_phase.is_none_or(|phase| !phase.is_open()) {
        return ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave;
    }
    ReadOnlyTerminalProjectionDecision::ObserveOnly
}

/// Resolve terminal convergence using whole-document equality by default and
/// exact retained-transition ownership only under the explicit experiment.
///
/// Every pending intent contributes the component names changed between its
/// expected and target cuts. Missing legacy `expected_content` or malformed
/// component structure fails closed to whole-document equality.
fn terminal_projection_matches_required_scope(
    file: &Path,
    authority_content: &str,
    disk_content: &str,
) -> Result<bool> {
    if authority_content == disk_content {
        return Ok(true);
    }
    if !crate::guard_modes::resolve_per_component_convergence(file)? {
        return Ok(false);
    }

    let mut owned_component_names = BTreeSet::new();
    for intent in agent_doc_document_realtime_io::pending_document_write_journal(file) {
        let Some(expected_content) = intent.expected_content.as_deref() else {
            agent_doc_ops_log_io::log_op(
                file,
                "per_component_convergence_fallback reason=legacy_intent_missing_expected_content",
            );
            return Ok(false);
        };
        let changed = match agent_doc_document::authority_hashes::changed_component_names(
            expected_content,
            &intent.target_content,
        ) {
            Ok(changed) => changed,
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "per_component_convergence_fallback reason=transition_parse_failed error={error:#}"
                    ),
                );
                return Ok(false);
            }
        };
        owned_component_names.extend(changed);
    }

    match agent_doc_document::authority_hashes::owned_component_names_converged(
        authority_content,
        disk_content,
        &owned_component_names,
    ) {
        Ok(converged) => Ok(converged),
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "per_component_convergence_fallback reason=authority_parse_failed error={error:#}"
                ),
            );
            Ok(false)
        }
    }
}

fn derive_read_only_retained_closeout_resume(
    settlement_observed: bool,
    authority_matches_disk: bool,
    cycle_phase: Option<CyclePhase>,
    retained_document_write_blocks: bool,
) -> bool {
    settlement_observed
        && authority_matches_disk
        && cycle_phase == Some(CyclePhase::WriteApplied)
        && retained_document_write_blocks
}

/// Reactive permission for a status-only check to resume the same retained
/// closeout after its own editor-native save.
///
/// The observations live only for the current read pass. Durable retained-write
/// settlement remains controller-owned; this graph cannot authorize response
/// replay, repair, or an unrelated commit.
pub struct ReadOnlyRetainedCloseoutResumeProjection {
    scope: LocalReadScope,
    settlement_observed: Source<bool>,
    authority_matches_disk: Source<bool>,
    cycle_phase: Source<Option<CyclePhase>>,
    retained_document_write_blocks: Source<bool>,
    should_resume: Computed<bool>,
}

impl ReadOnlyRetainedCloseoutResumeProjection {
    pub fn new(
        authority_matches_disk: bool,
        cycle_phase: Option<CyclePhase>,
        retained_document_write_blocks: bool,
    ) -> Self {
        let scope = LocalReadScope::new();
        // `#preconvergedcapturecloseout`: editor delivery may settle before the
        // operator starts session-check. Exact authority/disk equality at the
        // initial observation is already the receipt this read-only transition
        // needs; requiring this invocation to request another native save leaves
        // an otherwise exact captured `write_applied` cycle open forever. The
        // runtime resume still fences the transition to the durable capture id,
        // response hash, exact response materialization, and current disk bytes.
        let settlement_observed = scope.ctx().source(authority_matches_disk);
        let authority_matches_disk = scope.ctx().source(authority_matches_disk);
        let cycle_phase = scope.ctx().source(cycle_phase);
        let retained_document_write_blocks = scope.ctx().source(retained_document_write_blocks);
        let should_resume = scope.ctx().computed(move |ctx| {
            derive_read_only_retained_closeout_resume(
                settlement_observed.get(ctx),
                authority_matches_disk.get(ctx),
                cycle_phase.get(ctx),
                retained_document_write_blocks.get(ctx),
            )
        });
        Self {
            scope,
            settlement_observed,
            authority_matches_disk,
            cycle_phase,
            retained_document_write_blocks,
            should_resume,
        }
    }

    pub fn observe_native_save(&self, settled: bool, authority_matches_disk: bool) {
        self.scope.ctx().set(&self.settlement_observed, settled);
        self.scope
            .ctx()
            .set(&self.authority_matches_disk, authority_matches_disk);
    }

    pub fn should_resume(&self) -> bool {
        self.should_resume.get(self.scope.ctx())
    }

    pub fn observe_cycle_phase(&self, cycle_phase: Option<CyclePhase>) {
        self.scope.ctx().set(&self.cycle_phase, cycle_phase);
    }

    pub fn observe_retained_document_write_blocks(&self, blocks: bool) {
        self.scope
            .ctx()
            .set(&self.retained_document_write_blocks, blocks);
    }
}

pub trait SessionCheckEffects {
    /// Whether this caller owns document/lifecycle recovery. The operator-facing
    /// `session-check` command sets this false for general recovery, while
    /// finalize/write/preflight own mutations.
    fn allows_recovery(&self) -> bool {
        true
    }
    /// Whether this caller may apply the one lossless integrity
    /// canonicalization that removes a proven same-topic response replay.
    ///
    /// Operator-facing session-check remains status-only for every ambiguous
    /// or lifecycle-changing repair. This exception is a CAS replacement of a
    /// redundant protocol response whose retained capture and baseline prove
    /// the canonical body; it is required so the integrity gate cannot block
    /// the exact interrupted-closeout recovery it recognizes.
    fn allows_response_replay_canonicalization(&self) -> bool {
        self.allows_recovery()
    }
    /// Close the binary-owned git/snapshot boundary for the exact lossless
    /// response-replay canonicalization applied by this session-check pass.
    ///
    /// The default fails closed: only the production runtime (and explicit
    /// test effects) may own this narrow settlement.
    fn settle_response_replay_canonicalization(&self, _file: &Path) -> Result<()> {
        anyhow::bail!("response-replay canonicalization settlement is unavailable")
    }
    fn closeout_recovery_hint(&self, file: &Path) -> String;
    /// Whether observed closeout drift is *document-only* — the recovery
    /// classifier finds nothing pending, so no response body is at risk and the
    /// binary-owned `commit` boundary closes it losslessly.
    ///
    /// This is the same classification [`Self::closeout_recovery_hint`] already
    /// uses to decide its wording (`#deadlockhint`); exposing it lets preflight
    /// *run* that remedy instead of only printing it (`#hookcontractlost`).
    /// Keeping both on one classifier is what stops the hint and the recovery
    /// from drifting apart.
    ///
    /// Defaults to `false` so only the production runtime (and explicit test
    /// effects) can authorize an in-place commit.
    fn document_only_drift_is_commit_recoverable(&self, _file: &Path) -> bool {
        false
    }
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn atomic_repair_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String>;
    fn settle_committed_projection(
        &self,
        file: &Path,
        committed_content: &str,
        expected_current: &str,
    ) -> Result<()>;
    fn settle_retained_committed_projection(
        &self,
        file: &Path,
        committed_content: &str,
        expected_disk: &str,
    ) -> Result<bool>;
    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &Path,
    ) -> Result<Option<&'static str>>;
    fn recover_missing_commit_boundary(
        &self,
        file: &Path,
        event: &str,
    ) -> Result<Option<&'static str>>;
    fn resume_captured_finalize(&self, file: &Path) -> Result<CapturedFinalizeResumeOutcome>;
    /// Resume only the same durable `write_applied` capture whose editor-native
    /// save was requested and proven exact by this session-check pass.
    ///
    /// The default keeps fixtures and non-runtime effects inert. Production
    /// explicitly delegates this narrow transition to captured closeout
    /// recovery without enabling the general recovery pipeline.
    fn resume_retained_closeout_after_native_save(
        &self,
        _file: &Path,
    ) -> Result<CapturedFinalizeResumeOutcome> {
        Ok(CapturedFinalizeResumeOutcome::NotApplicable)
    }
    /// Resume a durable document-write effect after terminal authority/disk
    /// convergence has made its semantic replay safe.
    ///
    /// The default keeps pure/read-only fixtures inert. Production closeout
    /// effects override this with the same recovery transition preflight uses.
    fn recover_retained_document_write(&self, _file: &Path) -> Result<bool> {
        Ok(false)
    }
    /// Shared derived gate consumed by both preflight and session-check.
    ///
    /// Returning `true` must prevent a clean closeout report: otherwise
    /// preflight can refuse a new cycle immediately after session-check says
    /// the prior one is clean.
    fn retained_document_write_blocks(&self, _file: &Path) -> bool {
        false
    }
    /// Resume the git effect of an exact tracked-work-only `write --commit`
    /// after its retained editor projection has converged.
    ///
    /// Implementations must fence the operation to durable cycle intent and
    /// exact authority/disk content. The default keeps fixtures inert.
    fn resume_retained_pending_only_commit(&self, _file: &Path) -> Result<bool> {
        Ok(false)
    }
}

/// Derive the terminal divergence owner from one coherent observation cut.
///
/// `retained_projection_blocks` deliberately precedes `unanswered_edit`: a
/// Compact Exchange continuation can remain live after the response cycle is
/// committed and while a partial native save happens to expose queue/backlog
/// drift. That edit is not admissible as the next prompt until the continuation
/// settles or is identity-matched as superseded.
fn terminal_divergence_ownership(
    observed: agent_doc_turn::write_ownership::RetainedWriteOwnership,
    cycle_phase: Option<CyclePhase>,
    retained_capture: bool,
    retained_projection_blocks: bool,
    unanswered_edit: bool,
) -> agent_doc_turn::write_ownership::RetainedWriteOwnership {
    agent_doc_turn::write_ownership::RetainedWriteOwnership::new_with_phase(
        cycle_phase.is_some_and(agent_doc_turn::CyclePhase::is_open),
        retained_capture,
        cycle_phase == Some(agent_doc_turn::CyclePhase::WriteApplied),
    )
    .with_retained_projection(retained_projection_blocks)
    .with_unanswered_edit(unanswered_edit)
    .with_capture_resume_unowned(observed.capture_resume_unowned)
    .with_delivery_rejected(observed.delivery_rejected)
    .with_editor_route_unowned(observed.editor_route_unowned)
    .with_replica_unserved(observed.replica_unserved)
}

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path, effects: &impl SessionCheckEffects) -> Result<()> {
    run_with_options(file, false, effects)
}

/// `#qpausemix`: the queue-continuation guidance `session-check` prints when
/// `queue_continuation_required == true`, resolved against the controller pause
/// state. Reads the effective `admin queue pause` reason and composes the
/// pause-aware [`agent_doc_queue::queue_continuation::continuation_guidance`] — the SAME
/// single source consumed by the preflight `queue_continuation_guidance` field.
///
/// Printing the bare `CONTINUATION_NO_STALL_GUIDANCE` constant here (the prior
/// behavior) let the mixed-signal resolution reach preflight JSON but not
/// session-check stdout, so an agent reading session-check saw `queue_paused:
/// true` next to `queue_continuation_required: true` with no explanation and
/// stalled deciding whether the pause was operator intent or transient
/// drain-coordination state. The guidance now carries the "queue_paused is NOT a
/// contradiction" preamble and recorded reason whenever the queue is paused.
fn continuation_guidance_for(file: &Path) -> String {
    let pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    agent_doc_queue::queue_continuation::continuation_guidance(pause_reason.as_deref())
}

/// `#realtime-steering-verbatim` / `#no-thrash-steering` / `#steerinterruptexit`:
/// deterministic guidance for a committed cycle whose document already carries
/// fresh operator steering (the operator edited while the turn ran, which is
/// the whole point of a realtime document). The prior response is committed;
/// the steering is the NEXT cycle's input, handled per item `dispatch`. It is
/// not a failure, so it neither asks the agent to redo the closeout nor frames
/// the steering as an interruption of the current turn. Handing the agent this
/// exact instruction is what prevents the thrash loop (repeated
/// preflight/finalize, empty cycles, force-disk clobbers).
fn realtime_steering_closeout_guidance(file: &Path) -> String {
    // `#steeringremedydeadlock` / `#admissionsplitmerge`: this used to substitute an
    // operator-convergence remedy whenever baseline/authority/disk had split three
    // ways, because the `UserPromptSubmit` hook's preflight refused `agent-doc
    // <FILE>` in that state and advising the trigger produced a closed loop —
    // refuse, re-advise, refuse. Admission now merges the split instead of refusing,
    // so the trigger below is the correct advice again and substituting a remedy
    // would stall a document the binary can reconcile on its own. The observation is
    // still recorded, because a three-plane split is worth seeing in the log.
    log_three_way_merge_steering_observation(file);
    format!(
        "This is pending operator steering, not a failed closeout: your prior response is already committed in HEAD. Handle each item by its `dispatch`: `address_now` is the next cycle's prompt (`agent-doc {file}` admits it to continue), `drain_after_current` runs in operator queue order through the normal drain, and `subagent` goes to a new background subagent (claim it first with `agent-doc queue claim {file} --item <id-or-line> --owner subagent:<label>`). Do NOT re-run finalize on the prior response, do NOT use `--force-disk` (that clobbers the operator's live edits), and do NOT re-answer any prompt already committed in HEAD (the realtime replica reconciles your committed response back into the live buffer).",
        file = file.display()
    )
}

/// `#steerinterruptexit`: the outcome for unanswered operator steering found
/// after a terminal cycle. A `committed` cycle's steering is pending work, not
/// a failure ([`SessionCheckStatus::SteeringPending`], exit `0`); any other
/// terminal phase keeps the fail-closed `INTERRUPTED` it always had, because
/// nothing proves its own turn was answered.
fn committed_cycle_steering_status(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    repaired_drift: Option<&str>,
    marker: &str,
) -> SessionCheckStatus {
    let repaired = repaired_drift
        .map(|reason| format!(", repaired committed historical {reason} snapshot drift,"))
        .unwrap_or_else(|| ",".to_string());
    let detail = format!(
        "cycle `{}` is `{}` ({}){repaired} but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {marker}",
        state.cycle_id,
        state.phase.as_str(),
        state.last_event,
    );
    let guidance = realtime_steering_closeout_guidance(file);
    if state.phase != CyclePhase::Committed {
        return SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: {detail}\n{guidance}"
        ));
    }
    let items = crate::resolve_current_document_content(file, "session_check_steering_pending")
        .ok()
        .map(|current| crate::midturn_steering::pending_steering_items(file, &current))
        .unwrap_or_default();
    // `#steerbaselineabsorb`: a pending prompt the committed baseline already
    // carries would read as `no_changes` to the next preflight; persist it so
    // the next contract carries it.
    crate::absorbed_steering::record_reported_logged(file, &items, &state.cycle_id);
    let items = crate::midturn_steering::render_pending_steering_items(&items)
        .map(|rendered| format!("\n{rendered}"))
        .unwrap_or_default();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_steering_pending file={} cycle_id={} (#steerinterruptexit)",
            file.display(),
            state.cycle_id,
        ),
    );
    SessionCheckStatus::SteeringPending(format!(
        "{} {detail}{items}\n{guidance}",
        agent_doc_turn::response_text::SESSION_CHECK_STEERING_PENDING_PREFIX,
    ))
}

/// Record a three-plane split that the next admission will reconcile by merge.
///
/// Returns without logging whenever the planes need no merge, whenever no live
/// editor is registered, or whenever any plane cannot be observed — an unobservable
/// plane is not evidence of a split.
///
/// This is deliberately log-only. It must not change the steering guidance: the
/// merge happens inside admission, so there is no action for the operator or the
/// agent to take, and prescribing one is what stalled the queue before
/// (`#admissionsplitmerge`).
fn log_three_way_merge_steering_observation(file: &Path) {
    let Some(baseline) = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
    else {
        return;
    };
    let Ok(disk) = crate::resolve_disk_document_content(file, "session_check_steering_admission")
    else {
        return;
    };
    let Ok(authority) =
        crate::resolve_current_document_content(file, "session_check_steering_admission")
    else {
        return;
    };
    let divergence = agent_doc_document::admission_divergence::classify(
        Some(&baseline),
        Some(&authority),
        &disk,
    );
    if !divergence.requires_disk_merge() {
        return;
    }
    let durable = |content: &str| {
        agent_doc_hash::short_content_hash(
            &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content),
        )
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_steering_admission_three_way_split file={} reason={} baseline_hash={} authority_hash={} disk_hash={} durable_baseline_hash={} durable_authority_hash={} durable_disk_hash={} resolution=admission_three_way_merge (#admissionsplitmerge)",
            file.display(),
            divergence.reason_token(),
            agent_doc_hash::short_content_hash(&baseline),
            agent_doc_hash::short_content_hash(&authority),
            agent_doc_hash::short_content_hash(&disk),
            durable(&baseline),
            durable(&authority),
            durable(&disk),
        ),
    );
}

/// `#refusalsteeringverbatim` (GH #75): operator steering carried by the
/// unmerged editor save, verbatim.
///
/// The authority/disk divergence INTERRUPT bails before the steering scan ever
/// runs, and a refused preflight names `session-check` as the one permitted way
/// to read pending operator prompts — so an operator edit living only in the
/// editor authority (the side the save could not merge) was invisible on BOTH
/// paths while each told the agent steering "may be waiting unanswered".
/// Disk is the older projection and the authority carries the operator's
/// edit, so the prompt-bearing diff disk → authority is exactly that steering.
/// Empty when the unmerged save carries none.
///
/// `#committedsteeringecho`: "disk is older, so disk → authority is the
/// operator's edit" is false when the authority is ahead of disk by a
/// COMMITTED binary write. Observed 2026-10-01 on
/// `tasks/agent-doc/agent-doc-bugs.md`: the response was in HEAD and in the
/// authority, only the disk projection lagged, and every paragraph of that
/// committed response was relayed as "5 concurrent operator steering
/// directives". Text already in `committed` (HEAD) is not unanswered operator
/// steering, so it is filtered out; with no HEAD the old behaviour stands.
///
/// `#retainedsteeringecho`: the same echo happens one step earlier, while the
/// response is captured and delivered to the editor but NOT yet committed —
/// HEAD does not carry it, so the HEAD filter alone let it through. Observed
/// 2026-10-01 on `tasks/agent-doc/agent-doc-bugs.md` after a mid-turn install
/// left the native save pending: four paragraphs of the agent's own retained
/// response were relayed as "4 concurrent operator steering directives", an
/// instruction to answer its own reply. So `binary_authored` carries every
/// text the binary itself wrote — HEAD and the active capture's response —
/// and a directive contained in any of them is not operator steering. An
/// empty slice keeps the old unfiltered behaviour.
pub fn unmerged_editor_steering_note(
    authority_content: &str,
    disk_content: &str,
    binary_authored: &[&str],
) -> String {
    unsaved_operator_steering_verbatim(authority_content, disk_content, binary_authored)
        .map(|verbatim| {
            format!(
                " Operator steering inside the unmerged editor save (not yet on disk; relay it to the operator verbatim, do not answer it until the save settles): {verbatim}"
            )
        })
        .unwrap_or_default()
}

/// `#operatorleadpendingsave`: the operator steering an unsaved editor lead
/// carries, as pending steering rather than an unmerged-save warning. The
/// edit is authoritative already; it reaches the running turn through mid-turn
/// steering, or becomes the next admission's input.
pub fn pending_operator_steering_note(
    authority_content: &str,
    disk_content: &str,
    binary_authored: &[&str],
) -> String {
    unsaved_operator_steering_verbatim(authority_content, disk_content, binary_authored)
        .map(|verbatim| {
            format!(
                " Pending operator steering in the editor authority (authoritative now; it reaches the running turn as mid-turn steering, or the next `agent-doc` admission as input): {verbatim}"
            )
        })
        .unwrap_or_default()
}

fn unsaved_operator_steering_verbatim(
    authority_content: &str,
    disk_content: &str,
    binary_authored: &[&str],
) -> Option<String> {
    let steering = agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
        disk_content,
        authority_content,
    )
    .realtime_steering_all();
    let steering = if binary_authored.is_empty() {
        steering
    } else {
        agent_doc_document_realtime::baseline_comparison::RealtimeSteeringSet::new(
            steering
                .directives()
                .iter()
                .filter(|directive| {
                    directive.verbatim().map(str::trim).is_none_or(|body| {
                        body.is_empty() || !binary_authored.iter().any(|text| text.contains(body))
                    })
                })
                .cloned()
                .collect(),
        )
    };
    steering.verbatim_aggregate()
}

/// `#operatorleadpendingsave`: whether an authority/disk divergence is only the
/// operator's edits ahead of a lagging disk projection.
///
/// While an editor holds authority the disk is irrelevant to the turn: the
/// operator typing into the queue or exchange leaves disk behind until the
/// controller's editor-native save lands, and reporting that as `INTERRUPTED`
/// stopped the turn over the operator's own edit (tasks/sdk.md, 2026-10-03:
/// `component_divergence=exchange:…,queue:…`). Every fact that would make the
/// divergence the BINARY's — an open cycle (including `write_applied`), a
/// retained capture or document write, this cycle's unlanded tracked-work
/// mutations — keeps the existing fail-closed path; the pure provenance rule
/// ([`agent_doc_document::admission_divergence::authority_leads_disk_by_operator_edits`])
/// decides the rest, keeping a disk that is ahead of or conflicting with the
/// authority a hard failure.
fn operator_edit_lead_over_disk(
    file: &Path,
    authority_content: &str,
    disk_content: &str,
    cycle_phase: Option<CyclePhase>,
    retained_document_write_blocks: bool,
) -> bool {
    if authority_content == disk_content
        || cycle_phase.is_some_and(CyclePhase::is_open)
        || retained_document_write_blocks
        || agent_doc_capture_io::load_active(file)
            .ok()
            .flatten()
            .is_some()
    {
        return false;
    }
    let unlanded_own_mutations = agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .is_some_and(|state| {
            agent_doc_turn::write_ownership::recorded_tracked_work_is_unlanded(
                agent_doc_turn::write_ownership::RecordedTrackedWork {
                    done_ids: &state.pending_done_ids,
                    added_ids: &state.pending_added_ids,
                    requested_done_ids: &state.requested_done_ids,
                    requested_added_ids: &state.requested_added_ids,
                    requested_mutations: state.requested_tracked_work_mutations,
                    mutations_applied: state.tracked_work_mutations_applied,
                },
                disk_content,
            )
        });
    if unlanded_own_mutations {
        return false;
    }
    let baseline = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let head = agent_doc_git_io::revision::show_head(file).ok().flatten();
    agent_doc_document::admission_divergence::authority_leads_disk_by_operator_edits(
        baseline.as_deref(),
        head.as_deref(),
        authority_content,
        disk_content,
    )
}

/// Report an operator-edit lead as a pending save (plus any pending steering),
/// never as an interruption.
fn report_operator_edit_pending_save(
    file: &Path,
    authority_content: &str,
    disk_content: &str,
    source: &str,
) {
    let divergence =
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            authority_content,
            disk_content,
        );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_operator_edit_pending_save file={} source={} authority_hash={} disk_hash={} component_divergence={} (#operatorleadpendingsave)",
            file.display(),
            source,
            agent_doc_hash::short_content_hash(authority_content),
            agent_doc_hash::short_content_hash(disk_content),
            divergence,
        ),
    );
    eprintln!(
        "[session-check] pending-save: the editor authority for {} leads its disk projection by operator edits only (component_divergence={}). This is not a turn failure: the editor buffer is authoritative and the controller persists it with an editor-native save as delivery allows. Do not patch, restore, or force-disk the file.{}",
        file.display(),
        divergence,
        pending_operator_steering_note(
            authority_content,
            disk_content,
            &binary_authored_texts(file)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
    );
}

/// `#retainedsteeringecho`: text the binary itself wrote to `file` — committed
/// HEAD plus the active capture's response — so the unmerged-save steering scan
/// never relays the binary's own words as operator steering.
fn binary_authored_texts(file: &Path) -> Vec<String> {
    let mut texts = Vec::new();
    match agent_doc_git_io::revision::show_head(file) {
        Ok(Some(head)) => texts.push(head),
        Ok(None) => {}
        Err(err) => eprintln!(
            "[session-check] could not read HEAD for {} to filter steering: {err:#}",
            file.display()
        ),
    }
    match agent_doc_capture_io::load_active(file) {
        Ok(Some(capture)) => texts.push(capture.response_body),
        Ok(None) => {}
        Err(err) => eprintln!(
            "[session-check] could not load the active capture for {} to filter steering: {err:#}",
            file.display()
        ),
    }
    texts
}

pub fn supervisor_drain_outcome_kind(
    readiness: agent_doc_controller::status::SupervisorDrainReadiness,
) -> agent_doc_flow::outcome::UserFacingOutcomeKind {
    if readiness.is_ready() {
        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForSupervisorDrain
    } else {
        agent_doc_flow::outcome::UserFacingOutcomeKind::SupervisorDrainUnavailable
    }
}

pub fn supervisor_drain_unavailable_message(
    file: &Path,
    readiness: agent_doc_controller::status::SupervisorDrainReadiness,
) -> String {
    supervisor_drain_unavailable_message_for_description(
        file,
        readiness,
        "a supervisor-scope queue head",
    )
}

fn supervisor_drain_unavailable_message_for_description(
    file: &Path,
    readiness: agent_doc_controller::status::SupervisorDrainReadiness,
    head_description: &str,
) -> String {
    let who = match readiness.supervisor_pid() {
        Some(pid) => format!("the supervisor (pid {pid})"),
        None => "no live supervisor".to_string(),
    };
    format!(
        "[session-check] queue continues IN-SESSION: {head_description} remains for the supervisor clear-and-continue path, but {who} cannot drain it (supervisor_drain_readiness={}). Yielding would leave it undrained, so drain it in this session now; do not hand it back to the operator or ask for a re-trigger ({}; #supdrainlive, #focusedfallback).",
        readiness.reason(),
        file.display(),
    )
}

/// Describe the actual execution-context tag on a supervisor-scope head.
///
/// Supervisor scope is broader than `[focused-cycle]`: subagent and preset
/// routing can hand it ordinary or `[clean-session]` heads as well.
pub fn supervisor_drain_head_description(content: &str, head: &str) -> &'static str {
    if agent_doc_queue::queue_continuation::head_requires_focused_cycle_in(content, head) {
        "a `[focused-cycle]` head"
    } else if agent_doc_queue::queue_continuation::head_requires_clean_session_in(content, head) {
        "a `[clean-session]` head"
    } else {
        "an untagged queue head"
    }
}

pub fn log_supervisor_drain_handoff(
    file: &Path,
    head: &str,
    readiness: agent_doc_controller::status::SupervisorDrainReadiness,
    outcome_fields: &str,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_supervisor_drain_handoff file={} head_bytes={} head_sha256={} supervisor_drain_readiness={} supervisor_pid={} {}",
            file.display(),
            head.len(),
            agent_doc_hash::content_hash(head),
            readiness.reason(),
            readiness
                .supervisor_pid()
                .map_or_else(|| "none".to_string(), |pid| pid.to_string()),
            outcome_fields
        ),
    );
}

fn clear_queue_drain_stall_after_valid_closeout(file: &Path, reason: &str) {
    if let Err(err) = agent_doc_controller_io::project_controller::clear_queue_drain_stall_continuation_pending_for_file(
        file, reason,
    ) {
        eprintln!(
            "[session-check] warning: failed to reconcile prior continuation projection: {err}"
        );
    }
}

fn no_in_session_continuation_resolves_prior_stall(
    supervisor_readiness: Option<agent_doc_controller::status::SupervisorDrainReadiness>,
) -> bool {
    supervisor_readiness.is_none_or(|readiness| readiness.is_ready())
}

fn ensure_terminal_authority_disk_convergence(
    file: &Path,
    authority_content: &str,
    disk_content: &str,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    if authority_content == disk_content {
        // Converged planes are not on their own proof that no retained intent
        // is outstanding. This path used to return ok without ever consulting
        // the retained write, so an intent whose byte target a concurrent
        // operator edit had rebased stayed in `pending_write` forever while
        // session-check reported success — and preflight, reading that same
        // intent, refused every new cycle.
        //
        // `#retainedclearreactive`: reading the shared derived verdict is the
        // whole of it now. The controller's per-document settle effect is
        // subscribed to that verdict cell, so a `Satisfied` intent is cleared
        // *because the fact says so* — there is no companion `settle_*` call
        // here to keep in sync with preflight's, which is the parity bug
        // `#preflightsettleparity` patched by adding the second call site.
        agent_doc_document_realtime_io::retained_write_settlement(
            file,
            "session_check_terminal_convergence_derived_settlement",
        );
        return Ok(());
    }
    let cycle_phase =
        agent_doc_cycle_state_io::load_with_closeout_projection(file)?.map(|state| state.phase);
    let non_capture_cycle = terminal_phase_allows_non_capture_projection_settlement(cycle_phase);
    if non_capture_cycle
        && agent_doc_document_realtime_io::settle_retained_non_capture_projection_through_authority(
            file,
            "session_check_terminal_non_capture_projection_settlement",
        )?
    {
        let settled_authority = crate::resolve_current_document_content(
            file,
            "session_check_terminal_non_capture_projection_settled",
        )?;
        let settled_disk = crate::resolve_disk_document_content(
            file,
            "session_check_terminal_non_capture_projection_settled",
        )?;
        anyhow::ensure!(
            settled_authority == authority_content && settled_disk == authority_content,
            "[session-check] retained non-capture projection settlement for {} returned without exact authority/disk convergence (authority_hash={}, disk_hash={}, component_divergence={})",
            file.display(),
            agent_doc_hash::content_hash(&settled_authority),
            agent_doc_hash::content_hash(&settled_disk),
            agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
                &settled_authority,
                &settled_disk,
            ),
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &settled_authority,
            agent_doc_ops_log_io::log_op,
        )?;
        return Ok(());
    }
    // Saving the exact live editor cut is a reactive authority-convergence
    // effect, not a closeout mutation. It is therefore safe in every cycle
    // phase, including a captured response: phase identity still gates
    // semantic replay/commit, while the editor remains the persistence
    // authority.
    if agent_doc_document_realtime_io::settle_live_editor_projection_through_authority(
        file,
        "session_check_terminal_live_editor_projection_settlement",
    )? {
        let settled_authority = crate::resolve_current_document_content(
            file,
            "session_check_terminal_live_editor_projection_settled",
        )?;
        let settled_disk = crate::resolve_disk_document_content(
            file,
            "session_check_terminal_live_editor_projection_settled",
        )?;
        if settled_authority == authority_content && settled_disk == authority_content {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &settled_authority,
                agent_doc_ops_log_io::log_op,
            )?;
            return Ok(());
        }
    }
    if agent_doc_git_io::revision::show_head(file)?.as_deref() == Some(authority_content)
        && effects.settle_retained_committed_projection(file, authority_content, disk_content)?
    {
        let settled_authority = crate::resolve_current_document_content(
            file,
            "session_check_retained_projection_settled",
        )?;
        let settled_disk = crate::resolve_disk_document_content(
            file,
            "session_check_retained_projection_settled",
        )?;
        anyhow::ensure!(
            settled_authority == authority_content && settled_disk == authority_content,
            "[session-check] retained committed projection settlement for {} returned without exact HEAD/authority/disk convergence (head_hash={}, authority_hash={}, disk_hash={}, component_divergence={})",
            file.display(),
            agent_doc_hash::content_hash(authority_content),
            agent_doc_hash::content_hash(&settled_authority),
            agent_doc_hash::content_hash(&settled_disk),
            agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
                &settled_authority,
                &settled_disk,
            ),
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            authority_content,
            agent_doc_ops_log_io::log_op,
        )?;
        return Ok(());
    }
    if operator_edit_lead_over_disk(
        file,
        authority_content,
        disk_content,
        cycle_phase,
        effects.retained_document_write_blocks(file),
    ) {
        report_operator_edit_pending_save(
            file,
            authority_content,
            disk_content,
            "terminal_convergence",
        );
        return Ok(());
    }
    let recovery_status =
        agent_doc_controller_io::project_controller::schedule_stale_editor_replica_cp_recycle(
            file,
            "session_check_terminal_convergence",
        );
    anyhow::bail!(
        "[session-check] INTERRUPTED: canonical editor authority and disk projection diverge for {} (authority_hash={}, disk_hash={}, component_divergence={}); refusing a false successful closeout. Automatic editor recovery status: {}. Replica re-registration and projection settlement are scheduled automatically; supervisor recycle is fallback-only when the targeted event cannot be published. `session-check` is status-only. Do not rerun `finalize`, run `write --commit`, repair, or force-disk the response.{}",
        file.display(),
        agent_doc_hash::content_hash(authority_content),
        agent_doc_hash::content_hash(disk_content),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            authority_content,
            disk_content,
        ),
        recovery_status,
        unmerged_editor_steering_note(
            authority_content,
            disk_content,
            &binary_authored_texts(file)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
    );
}

fn terminal_phase_allows_non_capture_projection_settlement(phase: Option<CyclePhase>) -> bool {
    phase.is_none_or(|phase| phase == CyclePhase::PreflightStarted)
}

fn self_heal_transiently_stale_committed_projection(
    file: &Path,
    authority_content: &str,
    effects: &impl SessionCheckEffects,
) -> Result<bool> {
    let Some(committed_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    if authority_content == committed_content {
        return Ok(false);
    }
    if agent_doc_turn::document_drift::authority_is_committed_with_only_head_markers(
        authority_content,
        &committed_content,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            "session_check_committed_projection_current_head_marker action=accept_without_write",
        );
        return Ok(false);
    }
    let normalized_authority =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            authority_content,
        );
    let normalized_committed =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            &committed_content,
        );
    if normalized_authority != normalized_committed {
        return Ok(false);
    }

    eprintln!(
        "[session-check] committed_projection_stale: self-healing transient-only authority/disk drift for {}",
        file.display(),
    );
    effects.settle_committed_projection(file, &committed_content, authority_content)?;
    let settled_authority = crate::resolve_current_document_content(
        file,
        "session_check_committed_projection_settled",
    )?;
    let settled_disk =
        crate::resolve_disk_document_content(file, "session_check_committed_projection_settled")?;
    anyhow::ensure!(
        settled_authority == committed_content && settled_disk == committed_content,
        "[session-check] committed projection settlement for {} returned without exact HEAD/authority/disk convergence (head_hash={}, authority_hash={}, disk_hash={}, component_divergence={})",
        file.display(),
        agent_doc_hash::content_hash(&committed_content),
        agent_doc_hash::content_hash(&settled_authority),
        agent_doc_hash::content_hash(&settled_disk),
        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
            &settled_authority,
            &settled_disk,
        ),
    );
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &committed_content,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(true)
}

/// `session-check` with the optional Codex final-gate.
///
/// Default (`codex_final_gate = false`): keeps exit 0 for a clean document and
/// prints `queue_continuation_required=...` as an informational typed detail.
/// Strict (`codex_final_gate = true`): exits `2` when a clean document still
/// owes an active `agent:queue auto` continuation, so Codex direct-exec closeout
/// paths cannot send a final answer past a stalled queue.
/// (#codex-auto-queue-stalled-final-gate)
pub fn run_with_options(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    exit_on_interrupted(file, try_run_with_options(file, codex_final_gate, effects))
}

/// A `session-check` INTERRUPTED outcome, carried as an error so a caller can
/// inspect it before the process exits (`#scsettleexit`).
///
/// The Interrupted branch used to `println!` and exited the process from
/// inside the check, so `run_read_only_settling` never received an `Err` to
/// classify: the `#scsettlewindow` (GH #60) re-sample was dead code on exactly
/// the `write_applied` deferral it was written for. Observed 2026-09-30 on
/// `sampleorders.md`: `respond` deferred, the one permitted `session-check`
/// reported INTERRUPTED after 2.7s, the Codex loop stopped, and the retained
/// intent committed 14s later.
#[derive(Debug)]
pub struct SessionCheckInterrupted(pub String);

impl std::fmt::Display for SessionCheckInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SessionCheckInterrupted {}

/// Print an INTERRUPTED outcome to stdout and exit `1`, as the CLI always has;
/// pass every other result through.
fn exit_on_interrupted(file: &Path, outcome: Result<()>) -> Result<()> {
    match outcome {
        Err(error) => match error.downcast::<SessionCheckInterrupted>() {
            Ok(SessionCheckInterrupted(message)) => {
                println!("{}", message);
                crate::profile::report_now(file);
                std::process::exit(1);
            }
            Err(other) => Err(other),
        },
        ok => ok,
    }
}

/// [`run_with_options`] that returns an INTERRUPTED outcome as a
/// [`SessionCheckInterrupted`] error instead of exiting.
pub fn try_run_with_options(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    // `#sccurrentpass`: one document version per sweep. See
    // `with_current_document_pass`.
    // Reset and report at the OUTERMOST boundary. Doing it around `inspect_core`
    // instead cleared the samples that the self-heal phases above it had already
    // recorded, which is how a profile can under-report the very work it was
    // added to find (`#sessioncheckprofile`).
    crate::profile::reset();
    let started = std::time::Instant::now();
    let out = crate::with_current_document_pass(|| {
        run_with_options_inner(file, codex_final_gate, effects)
    });
    crate::profile::report(file, started.elapsed());
    out
}

/// Default bound for [`run_read_only_settling`] (`#scsettlewindow`, GH #60).
/// 45s, not 15s (`#scsettleexit`): the 2026-09-30 `sampleorders.md`
/// deferral committed ~17s after its single `session-check` began.
pub const SESSION_CHECK_SETTLE_WINDOW_SECS: u64 = 45;
/// Override for the settle window, in seconds; `0` samples once.
pub const SESSION_CHECK_SETTLE_WINDOW_ENV: &str = "AGENT_DOC_SESSION_CHECK_SETTLE_SECS";
const SESSION_CHECK_SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Interruptions the binary resolves on its own, with no agent action: every
/// one of these tells the harness to do nothing but re-run `session-check`.
/// An interruption that asks the agent to act (finalize, write --commit,
/// answer a prompt, dedupe) is deliberately absent — waiting cannot fix it.
const SELF_CONVERGING_INTERRUPTIONS: &[&str] = &[
    "response write landed but no terminal commit followed",
    "will resume automatically",
    "remains unsettled",
    "projection settlement are scheduled automatically",
    "binary-owned captured closeout",
];

/// Describe the read-only projection transition while preserving its owner.
/// The two controller-owned states deliberately carry the existing
/// `remains unsettled` settle-window token; terminal observations do not.
fn read_only_terminal_projection_detail(
    decision: ReadOnlyTerminalProjectionDecision,
) -> &'static str {
    match decision {
        ReadOnlyTerminalProjectionDecision::AwaitEditorDelivery => {
            "the exact editor delivery acknowledgement is pending; retained delivery remains unsettled while controller reconciliation remains scheduled"
        }
        ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave => {
            "the one-shot editor-native save receipt is pending; retained delivery remains unsettled while controller reconciliation remains scheduled"
        }
        ReadOnlyTerminalProjectionDecision::ObserveOnly => {
            "session-check is observing a non-recoverable projection state"
        }
        ReadOnlyTerminalProjectionDecision::Converged => {
            "the projection changed after its converged observation"
        }
    }
}

/// Whether `message` reports a state the binary converges without the agent.
pub fn is_self_converging_interruption(message: &str) -> bool {
    message.contains("[session-check] INTERRUPTED")
        && SELF_CONVERGING_INTERRUPTIONS
            .iter()
            .any(|needle| message.contains(needle))
}

pub fn session_check_settle_window() -> std::time::Duration {
    let secs = std::env::var(SESSION_CHECK_SETTLE_WINDOW_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(SESSION_CHECK_SETTLE_WINDOW_SECS);
    std::time::Duration::from_secs(secs)
}

/// Read-only `session-check` that waits out a self-converging interruption
/// (`#scsettlewindow`, GH #60).
///
/// A zero-replica deferral makes `respond` exit nonzero while the retained
/// intent commits on its own seconds later, and the harness contract permits
/// exactly one `session-check` afterwards. Sampling once at entry made the
/// operator's answer depend on how fast the harness typed: ~1s after `respond`
/// it said `write_applied` / INTERRUPTED, ~4s after it said `ok / committed`,
/// for the same cycle. This re-samples a self-converging interruption until it
/// resolves or `window` elapses, then reports the LAST state — a genuine stall
/// is still reported, only later. Any other outcome returns at once.
pub fn run_read_only_settling(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
    window: std::time::Duration,
) -> Result<()> {
    let outcome = settle_until_converged(window, SESSION_CHECK_SETTLE_POLL, || {
        let read_only = ReadOnlySessionCheckEffects { inner: effects };
        try_run_with_options(file, codex_final_gate, &read_only)
    });
    exit_on_interrupted(file, outcome)
}

fn settle_until_converged(
    window: std::time::Duration,
    poll: std::time::Duration,
    mut check: impl FnMut() -> Result<()>,
) -> Result<()> {
    let deadline = std::time::Instant::now() + window;
    loop {
        let outcome = check();
        let converging = matches!(
            &outcome,
            Err(error) if is_self_converging_interruption(&format!("{error:#}"))
        );
        if !converging || std::time::Instant::now() + poll > deadline {
            return outcome;
        }
        std::thread::sleep(poll);
    }
}

struct ReadOnlySessionCheckEffects<'a, E> {
    inner: &'a E,
}

impl<E: SessionCheckEffects> SessionCheckEffects for ReadOnlySessionCheckEffects<'_, E> {
    fn allows_recovery(&self) -> bool {
        false
    }

    fn allows_response_replay_canonicalization(&self) -> bool {
        true
    }

    fn settle_response_replay_canonicalization(&self, file: &Path) -> Result<()> {
        self.inner.settle_response_replay_canonicalization(file)
    }

    fn closeout_recovery_hint(&self, file: &Path) -> String {
        self.inner.closeout_recovery_hint(file)
    }

    fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
        anyhow::bail!("status-only session-check refused an atomic document write")
    }

    fn atomic_repair_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String> {
        anyhow::ensure!(
            source == "session_check_response_replay_dedup",
            "status-only session-check refused non-canonicalization repair `{source}`"
        );
        self.inner
            .atomic_repair_write_if_current(file, content, expected_current, source)
    }

    fn settle_committed_projection(
        &self,
        _file: &Path,
        _committed_content: &str,
        _expected_current: &str,
    ) -> Result<()> {
        anyhow::bail!("status-only session-check refused projection settlement")
    }

    fn settle_retained_committed_projection(
        &self,
        _file: &Path,
        _committed_content: &str,
        _expected_disk: &str,
    ) -> Result<bool> {
        Ok(false)
    }

    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &Path,
    ) -> Result<Option<&'static str>> {
        // The one established status-check self-heal is metadata-only: when
        // HEAD already proves the bytes, repair its stale snapshot baseline.
        self.inner.repair_committed_historical_snapshot_drift(file)
    }

    fn recover_missing_commit_boundary(
        &self,
        _file: &Path,
        _event: &str,
    ) -> Result<Option<&'static str>> {
        Ok(None)
    }

    fn resume_captured_finalize(&self, _file: &Path) -> Result<CapturedFinalizeResumeOutcome> {
        Ok(CapturedFinalizeResumeOutcome::NotApplicable)
    }

    fn resume_retained_closeout_after_native_save(
        &self,
        file: &Path,
    ) -> Result<CapturedFinalizeResumeOutcome> {
        self.inner.resume_retained_closeout_after_native_save(file)
    }

    fn recover_retained_document_write(&self, _file: &Path) -> Result<bool> {
        Ok(false)
    }

    fn retained_document_write_blocks(&self, file: &Path) -> bool {
        self.inner.retained_document_write_blocks(file)
    }

    fn resume_retained_pending_only_commit(&self, file: &Path) -> Result<bool> {
        self.inner.resume_retained_pending_only_commit(file)
    }
}

/// Operator-facing session check. It may read live authority, write
/// diagnostics/metadata logs, request one editor-owned native save for a
/// retained `write_applied` closeout, and resume that exact already-captured
/// cycle after the save is proven exact. It may also apply and commit the one
/// lossless response-replay canonicalization proven from `HEAD`, authority,
/// and disk. It cannot replace, repair, replay, or commit unrelated document
/// content.
pub fn run_read_only_with_options(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    let read_only = ReadOnlySessionCheckEffects { inner: effects };
    run_with_options(file, codex_final_gate, &read_only)
}

/// The supervisor-scope queue head that is still unowned. A head claimed by a
/// subagent (or held by the queue subagents attribute) is in flight
/// elsewhere, so neither the supervisor nor the in-session fallback
/// (`#supdrainlive`) may drain it. Ignoring claims here once told a session to
/// drain a head a subagent was already working on (agent-doc-bugs.md,
/// 2026-10-03).
fn supervisor_scope_unclaimed_head(file: &Path, content: &str) -> Option<String> {
    agent_doc_queue::queue_continuation::live_drainable_continuation_head_excluding_claimed(
        content,
        agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        &agent_doc_queue_io::queue_claim::claimed_items_for_content(file, content),
    )
}

fn run_with_options_inner(
    file: &Path,
    codex_final_gate: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    let allows_recovery = effects.allows_recovery();
    if allows_recovery
        && let Some(message) =
            agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
                file,
                "session_check_start",
            )
    {
        eprintln!("[session-check] WARNING: {message}");
    }
    // A retained response replay can duplicate only semantic response cells or
    // protocol boundary markers. Collapse that narrow, lossless transient
    // before the generic gate so integrity does not block its own recovery.
    // `#sccurrentpass`: invalidate the pass memo only when the self-heal
    // ACTUALLY rewrote the document. It already reports that -- it returns
    // `Ok(false)` the moment it finds nothing to dedupe -- and the call site was
    // throwing the answer away and invalidating anyway. Every discarded `false`
    // cost the next reader a fresh ~491ms authority resolve to observe a
    // document that had not changed. This is `#idlerevisionreactive` at the call
    // site: "might have changed" is not "changed".
    let replay_healed = if effects.allows_response_replay_canonicalization() {
        crate::profile::timed("self_heal_response_replay_duplication", || {
            self_heal_response_replay_duplication(file, effects)
        })?
    } else {
        false
    };
    // Re-read only if it actually rewrote the document.
    if replay_healed {
        crate::invalidate_current_document_pass(file);
    }
    // A retained delivery can be validly reconstructable even when a stale
    // post-replacement IPC delta duplicated the exchange. Repair that known,
    // evidence-backed overapplication before the generic integrity gate;
    // otherwise the duplicate boundary marker prevents the recovery routine
    // that knows how to remove it from ever running.
    let late_ipc_healed = if allows_recovery {
        crate::profile::timed("self_heal_late_ipc_overapplication", || {
            self_heal_late_ipc_overapplication(file, effects)
        })?
    } else {
        false
    };
    if late_ipc_healed {
        crate::invalidate_current_document_pass(file);
    }
    // `session-check` is the final proof boundary. It must not report a clean
    // cycle for a document whose component tree cannot be parsed, regardless
    // of lifecycle sidecar state.
    let integrity_content =
        crate::resolve_current_document_content(file, "session_check_integrity_gate")?;
    let integrity = agent_doc_lint_io::validate_integrity_on_content_with_logger(
        file,
        &integrity_content,
        agent_doc_ops_log_io::log_op,
    );
    if let Err(error) = integrity {
        // A stale cross-lineage delta can corrupt the visible projection after
        // the response was durably captured but before ACK. The retained
        // capture owns the lossless reconstruction recipe, so give it one
        // idempotent resume before returning the generic integrity error. This
        // breaks the former cycle: integrity blocked the only recovery capable
        // of restoring integrity.
        let resumed = allows_recovery
            && resume_captured_finalize_for_recovery(
                file,
                effects,
                "session_check_integrity_recovered_from_retained_capture",
            )?;
        // A resume rewrites the document; the pass must re-read (`#sccurrentpass`).
        crate::invalidate_current_document_pass(file);
        if !resumed {
            return Err(error);
        }
        validate_integrity_after_captured_resume(
            file,
            "session_check_integrity_gate_after_captured_resume",
            effects,
        )?;
    }
    let mut authority_content =
        crate::resolve_current_document_content(file, "session_check_terminal_convergence")?;
    let mut disk_content =
        crate::resolve_disk_document_content(file, "session_check_terminal_convergence")?;
    // A replacement/re-register may remove structural duplication while leaving
    // a clean retained response visible only in canonical authority. Do not let
    // the generic authority/disk divergence guard block the exact captured
    // response that owns the lossless replay recipe.
    let resumed_before_convergence =
        allows_recovery && resume_captured_finalize_before_terminal_convergence(file, effects)?;
    if resumed_before_convergence {
        crate::invalidate_current_document_pass(file);
    }
    if resumed_before_convergence {
        authority_content = validate_integrity_after_captured_resume(
            file,
            "session_check_terminal_convergence_after_captured_resume",
            effects,
        )?;
        disk_content = crate::resolve_disk_document_content(
            file,
            "session_check_terminal_convergence_after_captured_resume",
        )?;
    }
    if allows_recovery {
        ensure_terminal_authority_disk_convergence(
            file,
            &authority_content,
            &disk_content,
            effects,
        )?;
    } else {
        let cycle_phase =
            agent_doc_cycle_state_io::load_with_closeout_projection(file)?.map(|state| state.phase);
        let retained_document_write_blocks = effects.retained_document_write_blocks(file);
        let terminal_projection_converged =
            terminal_projection_matches_required_scope(file, &authority_content, &disk_content)?;
        let editor_delivery_converged = authority_content == disk_content
            || agent_doc_document_realtime_io::live_editor_projection_ready_for_native_save(
                file,
                &authority_content,
                "session_check_read_only_retained_delivery_gate",
            )?;
        let terminal_projection_decision = decide_read_only_terminal_projection(
            terminal_projection_converged,
            cycle_phase,
            retained_document_write_blocks,
            editor_delivery_converged,
        );
        let retained_closeout_resume = ReadOnlyRetainedCloseoutResumeProjection::new(
            authority_content == disk_content,
            cycle_phase,
            retained_document_write_blocks,
        );
        if terminal_projection_decision
            == ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave
            && agent_doc_document_realtime_io::settle_live_editor_projection_through_authority(
                file,
                "session_check_read_only_retained_native_save",
            )?
        {
            crate::invalidate_current_document_pass(file);
            authority_content = crate::resolve_current_document_content(
                file,
                "session_check_read_only_retained_native_save",
            )?;
            disk_content = crate::resolve_disk_document_content(
                file,
                "session_check_read_only_retained_native_save",
            )?;
            retained_closeout_resume.observe_native_save(true, authority_content == disk_content);
        }
        // `#divergenceowner`: this branch used to promise unconditionally that
        // "the controller owns the next closeout attempt and will wake on that
        // state edge", and to forbid every recovery. That is only true while
        // something durable actually holds the write. Observed 2026-08-03 on
        // tasks/software/lazily.md, twice: the newest cycle was `committed`
        // over an hour earlier and no response capture was retained, so nothing
        // owned the divergence and no state edge could ever fire — yet the
        // message told the session to stand down, and two batches of response
        // and backlog edits sat uncommitted until a human intervened. A session
        // that follows this instruction faithfully loses the work, so the claim
        // has to be earned rather than assumed.
        //
        // `#percellconverge`: the predicate now lives in
        // `agent_doc_turn::write_ownership` and the write path calls the same
        // one. Fixing this site alone left an agent obeying whichever of the
        // other two refusals it happened to reach first.
        // `#ownershipverdictdiverges`: carry the phase. A `write_applied` cycle
        // is neither a self-completing deferral nor a stranded write, and this
        // site is the one that already reported it correctly as INTERRUPTED —
        // its note must name the same recovery the write path now names.
        let retained_capture = agent_doc_capture_io::load_active(file)
            .ok()
            .flatten()
            .is_some();
        // `#strandedremedydeadlock`: ownership alone cannot tell "the agent's
        // write is stranded" from "the operator typed the next prompt into the
        // live buffer" — both have no cycle and no capture. This site reported
        // the second as the first and handed out the `Stranded` remedy, which
        // names `agent-doc commit <FILE>`; that command then refused, correctly,
        // because committing a fresh operator edit swallows the next turn's
        // prompt. Observed 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`
        // with `component_divergence=queue:<a>-><b>`, HEAD holding the whole
        // committed response and the git tree clean.
        //
        // `#retainedmutdrop`: the original distinguishing fact was WHICH
        // components diverge — "`exchange` is where a response lives, so an
        // exchange-clean divergence in queue/backlog/status is by construction
        // not this turn's write". That premise is FALSE. A closeout's write
        // produces `queue`, `backlog`, `status`, `review`, and `done` too (they
        // are in the turn's own write_set), so a response that committed while
        // its tracked-work half did not produces exactly this shape. Calling it
        // an operator edit sent recovery to sweep it, losing the mutations while
        // the response stayed committed — the delivery half of `#prmergeguardpr`.
        //
        // The real distinguishing fact is whether THIS cycle's own recorded
        // mutations are visible on disk yet. When they are, a queue/backlog
        // divergence really is a fresh edit and the refusal stays correct, which
        // is what keeps `commit` from swallowing the operator's next prompt.
        let recorded = agent_doc_cycle_state_io::load(file).ok().flatten();
        let unlanded_own_mutations = recorded.as_ref().is_some_and(|state| {
            agent_doc_turn::write_ownership::recorded_tracked_work_is_unlanded(
                agent_doc_turn::write_ownership::RecordedTrackedWork {
                    done_ids: &state.pending_done_ids,
                    added_ids: &state.pending_added_ids,
                    // `#mutprovenancepreresponse`: intent recorded BEFORE the
                    // response write, so a write that fails once the response
                    // has landed still proves the divergence is ours.
                    requested_done_ids: &state.requested_done_ids,
                    requested_added_ids: &state.requested_added_ids,
                    // `#mutplanwitness`: a gate/edit/reorder/status-only
                    // closeout has no id witness; ask the cycle whether its
                    // mutation envelope ever published.
                    requested_mutations: state.requested_tracked_work_mutations,
                    mutations_applied: state.tracked_work_mutations_applied,
                },
                &disk_content,
            )
        });
        let ownership = terminal_divergence_ownership(
            agent_doc_document_realtime_io::observed_retained_write_ownership(file),
            cycle_phase,
            retained_capture,
            retained_document_write_blocks,
            agent_doc_git::has_blocking_non_exchange_component_drift(
                &disk_content,
                &authority_content,
                None,
            ) && !unlanded_own_mutations,
        );
        let divergence_owner_note = match ownership.verdict() {
            agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred => {
                // `#refusedreceiptveto`: this used to assert the controller
                // "has requested the generation-fenced native editor save".
                // That is not something this site knows. The save is gated on
                // the visible-delivery receipt, and when the gate is closed no
                // request is sent at all — `request_sent=false`,
                // `save_diagnosis=native_save_gate_not_ready`. Observed
                // 2026-09-28 on `tasks/agent-doc/agent-doc-bugs.md`: the
                // operator was told a request was in flight and to stand down,
                // for 38 minutes, while no request had ever been made. Name the
                // owner, which is true, and not the request, which may not have
                // happened.
                "The controller owns the next closeout attempt; it retries the native editor save as the \
                 delivery receipt allows. The editor buffer remains authoritative; Git, disk, and snapshots \
                 are evidence only. \
                 Do not patch or restore the file, ask the operator to save, rerun session-check, or resubmit finalize"
                    .to_string()
            }
            agent_doc_turn::write_ownership::RetainedWriteVerdict::Stranded
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::AwaitingTerminalCommit
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::CaptureResumeUnowned
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::UnansweredEditPending
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::DeliveryRejected
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::ReplicaUnserved => {
                agent_doc_turn::write_ownership::retained_write_remedy(
                    ownership,
                    &file.display().to_string(),
                )
            }
        };
        let projection_matches =
            terminal_projection_matches_required_scope(file, &authority_content, &disk_content)?;
        let operator_edit_lead = !projection_matches
            && operator_edit_lead_over_disk(
                file,
                &authority_content,
                &disk_content,
                cycle_phase,
                retained_document_write_blocks,
            );
        if operator_edit_lead {
            report_operator_edit_pending_save(
                file,
                &authority_content,
                &disk_content,
                "read_only_terminal_convergence",
            );
        }
        anyhow::ensure!(
            projection_matches || operator_edit_lead,
            "[session-check] INTERRUPTED: canonical editor authority and disk projection diverge for {} (authority_hash={}, disk_hash={}, component_divergence={}); {}. {}{}",
            file.display(),
            agent_doc_hash::content_hash(&authority_content),
            agent_doc_hash::content_hash(&disk_content),
            agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
                &authority_content,
                &disk_content,
            ),
            read_only_terminal_projection_detail(terminal_projection_decision),
            divergence_owner_note,
            unmerged_editor_steering_note(
                &authority_content,
                &disk_content,
                &binary_authored_texts(file)
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ),
        );
        if retained_closeout_resume.should_resume() {
            match crate::profile::timed("resume_retained_closeout_after_native_save", || {
                effects.resume_retained_closeout_after_native_save(file)
            })? {
                CapturedFinalizeResumeOutcome::Committed
                | CapturedFinalizeResumeOutcome::Superseded => {
                    crate::invalidate_current_document_pass(file);
                    authority_content = crate::resolve_current_document_content(
                        file,
                        "session_check_read_only_retained_closeout_resumed",
                    )?;
                    disk_content = crate::resolve_disk_document_content(
                        file,
                        "session_check_read_only_retained_closeout_resumed",
                    )?;
                    anyhow::ensure!(
                        authority_content == disk_content,
                        "[session-check] INTERRUPTED: retained closeout resumed after editor-native save, but canonical authority and disk diverged again for {} (authority_hash={}, disk_hash={}, component_divergence={})",
                        file.display(),
                        agent_doc_hash::content_hash(&authority_content),
                        agent_doc_hash::content_hash(&disk_content),
                        agent_doc_document::authority_hashes::format_authority_disk_component_divergence(
                            &authority_content,
                            &disk_content,
                        ),
                    );
                }
                CapturedFinalizeResumeOutcome::Retained { reason }
                | CapturedFinalizeResumeOutcome::RetryAt { reason, .. } => {
                    anyhow::bail!(
                        "[session-check] INTERRUPTED: editor-native save converged for {}, but the same retained closeout could not finish: {}",
                        file.display(),
                        reason,
                    );
                }
                CapturedFinalizeResumeOutcome::NotApplicable => {}
            }
        }
    }
    let stale_projection_healed = allows_recovery
        && crate::profile::timed("self_heal_transiently_stale_committed_projection", || {
            self_heal_transiently_stale_committed_projection(file, &authority_content, effects)
        })?;
    // Re-read only if a new document image was actually projected.
    if stale_projection_healed {
        crate::invalidate_current_document_pass(file);
    }
    // Terminal convergence can turn an `AwaitConvergence` retained write into
    // a causally replayable `ReplayStranded` transition. Session-check owns the
    // same recovery as preflight so it cannot report "ok" while the very next
    // preflight refuses the unchanged retained effect.
    let retained_write_recovered = allows_recovery
        && crate::profile::timed("recover_retained_document_write", || {
            effects.recover_retained_document_write(file)
        })?;
    if retained_write_recovered {
        crate::invalidate_current_document_pass(file);
        let recovered_authority = crate::resolve_current_document_content(
            file,
            "session_check_retained_write_recovered",
        )?;
        let recovered_disk =
            crate::resolve_disk_document_content(file, "session_check_retained_write_recovered")?;
        ensure_terminal_authority_disk_convergence(
            file,
            &recovered_authority,
            &recovered_disk,
            effects,
        )?;
    }
    // A retained tracked-work-only write may have converged after its original
    // `--commit` invocation returned at the editor boundary. The requested git
    // effect is durable cycle state, so session-check resumes that exact
    // continuation from the state edge instead of asking for another write or
    // preflight cycle.
    if crate::profile::timed("resume_retained_pending_only_commit", || {
        effects.resume_retained_pending_only_commit(file)
    })? {
        crate::invalidate_current_document_pass(file);
    }
    // The lossless replay canonicalization is itself a binary-owned document
    // mutation. A terminal session-check may not report `OK` until the same
    // exact bytes cross the git/snapshot boundary. Re-prove from HEAD as well
    // as remembering this pass so a process restart between write and commit
    // remains recoverable.
    let replay_canonicalization_requires_settlement = effects
        .allows_response_replay_canonicalization()
        && (replay_healed || response_replay_canonicalization_pending_commit(file)?);
    // Phase E rung 2 (`#adstatechart2`): advisory read-only observability of the
    // local-process four-region state, logged alongside the existing ops.log
    // markers. Never gates closeout — emitted regardless of the check outcome.
    agent_doc_state_observer_io::log_advisory_snapshot(file);
    let report = crate::profile::timed("inspect_with_warnings", || {
        inspect_with_warnings(file, effects)
    })?;
    for warning in &report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(message) => {
            if replay_canonicalization_requires_settlement {
                effects
                    .settle_response_replay_canonicalization(file)
                    .with_context(|| {
                        format!(
                            "[session-check] INTERRUPTED: response-replay canonicalization for {} reached the document but not its binary-owned commit boundary",
                            file.display()
                        )
                    })?;
                crate::invalidate_current_document_pass(file);
            }
            println!("{}", message);
            warn_unrecorded_chat_prompts(file);
            // `#wd40` / `#staleloop-recycle-restart`: a stale route-owned supervisor
            // that can never reach its own recycle boundary during a continuously
            // self-draining session asks the in-session loop to yield one boundary.
            // Surface that as a distinct, intentional yield (NOT a drained queue or
            // a stall) so the loop ends its turn cleanly; the idle boundary lets the
            // `execve` recycle fire and the drain resumes on the fresh binary. Never
            // force the Codex final-gate here — yielding is the desired outcome.
            if agent_doc_controller_io::project_controller::supervisor_recycle_yield_pending_for_file(file) {
                clear_queue_drain_stall_after_valid_closeout(file, "supervisor_recycle_yield");
                let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                    agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
                )
                .expect("static no-drainable-work outcome is valid")
                .log_fields();
                println!(
                    "queue_continuation_required=false queue_recycle_yield=true {outcome_fields}"
                );
                eprintln!(
                    "[session-check] {}",
                    agent_doc_queue::queue_continuation::RECYCLE_YIELD_GUIDANCE
                );
                return Ok(());
            }
            let continuation_content =
                crate::resolve_current_document_content(file, "session_check_queue_continuation")?;
            if let Some(continuation) = agent_doc_queue_io::queue_continuation::detect_for_content(
                file,
                &continuation_content,
            )? {
                // #prompt-preempts-auto-queue: a live unresolved exchange prompt
                // must run before queue continuation, even when it was already
                // baselined into the snapshot. Defer the queue (do not force the
                // Codex final-gate) while such a prompt exists so the next cycle
                // answers it instead of skipping to the queue head.
                if let Some(unresolved) = crate::unresolved_exchange_prompt(file)? {
                    clear_queue_drain_stall_after_valid_closeout(
                        file,
                        "clean_closeout_user_prompt_preempts",
                    );
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForOperatorProof,
                    )
                    .expect("static deferred operator-proof outcome is valid")
                    .log_fields();
                    println!(
                        "queue_continuation_required=false queue_deferred_for_unresolved_exchange_prompt={:?} next_queue_prompt={:?} {}",
                        unresolved, continuation.head_prompt, outcome_fields
                    );
                    eprintln!(
                        "[session-check] queue continuation deferred for {}: unresolved exchange prompt {:?} must run before the queue head {:?} (#prompt-preempts-auto-queue). {}",
                        file.display(),
                        unresolved,
                        continuation.head_prompt,
                        outcome_fields
                    );
                    return Ok(());
                }
                if let Some(command) =
                    agent_doc_queue::queue_command::slash_command_text(&continuation.head_prompt)
                {
                    println!(
                        "queue_continuation_required=true next_queue_command={:?}",
                        command
                    );
                } else {
                    println!(
                        "queue_continuation_required=true next_queue_prompt={:?}",
                        continuation.head_prompt
                    );
                }
                // #degraded-ipc-no-stall: binary-authoritative "keep draining"
                // guidance so the loop is not stalled on a degraded transport /
                // stale supervisor / accretion / semantic-completion warning.
                //
                // #qpausemix: emit the pause-aware guidance, NOT the bare
                // `CONTINUATION_NO_STALL_GUIDANCE` constant, so a controller-paused
                // queue (`queue_paused: true` alongside this
                // `queue_continuation_required: true`) prints the "queue_paused is
                // NOT a contradiction" preamble + recorded pause reason here too.
                eprintln!("[session-check] {}", continuation_guidance_for(file));
                // #qpausemix-verify / #j9ja: the pause-aware guidance above only
                // reaches session-check stderr. When the queue is controller-paused,
                // also drop a distinctive SUCCESS marker into ops.log so a live
                // operator test of the "queue_paused is NOT a contradiction" preamble
                // is provable/disprovable from the log (auto-verify resolves the gate
                // via `--pending-set-verify
                // verify=ops_log:queue_paused_continuation_guidance_emitted`).
                if let Some(reason) =
                    agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(
                        file,
                    )
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_paused_continuation_guidance_emitted pause_reason={reason:?} #qpausemix-verify"
                        ),
                    );
                }
                // #qstallguard Layer B: this is a clean closeout that STILL requires
                // continuation. Record a one-shot continuation-pending projection so
                // the next preflight can emit `queue_stall_detected` if the loop neither
                // continued nor recorded a valid stop reason (the prose no-stall
                // guidance is advisory; this makes the violation a hard signal).
                let stall_cycle_id = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                    .ok()
                    .flatten()
                    .map(|s| s.cycle_id)
                    .unwrap_or_default();
                let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(
                    file,
                    &continuation_content,
                );
                let recorded_drainable_head_count =
                    agent_doc_queue::queue_continuation::drainable_head_count_excluding_claimed(
                        &continuation_content,
                        &claimed,
                    );
                if let Err(err) = agent_doc_controller_io::project_controller::record_queue_drain_stall_continuation_pending_for_file(
                    file,
                    &stall_cycle_id,
                    &continuation.head_prompt,
                    recorded_drainable_head_count,
                ) {
                    eprintln!(
                        "[session-check] warning: failed to record continuation projection: {err}"
                    );
                }
                if codex_final_gate {
                    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(
                        &continuation.head_prompt,
                    ) {
                        eprintln!(
                            "[session-check] codex-final-gate: active `agent:queue auto` slash command required for {} — submit {} after the current turn reaches an idle prompt before sending any final answer.",
                            file.display(),
                            command
                        );
                    } else {
                        eprintln!(
                            "[session-check] codex-final-gate: active `agent:queue auto` continuation required for {} — continue with `agent-doc {}` before sending any final answer.",
                            file.display(),
                            file.display()
                        );
                    }
                    crate::profile::report_now(file);
                    std::process::exit(2);
                }
            } else {
                // #goqueuestall: when continuation is not required because every
                // remaining queue head is undrainable in the current session type
                // (`[clean-session]` under live IPC, or `[operator-verify]`),
                // surface a one-line deferred-heads note so the idle queue reads
                // as deferred work, not a silent stall.
                let content =
                    crate::resolve_current_document_content(file, "deferred_queue_head_count").ok();
                let deferred = content
                    .as_deref()
                    .map(agent_doc_queue::queue_continuation::deferred_head_count)
                    .unwrap_or(0);
                // #goqstall2/#freshqueueauth: pre-materialized queue lines that
                // match the exact non-drainable noise predicate (pasted console
                // evidence / agent fragments, never ordinary prose reports) are
                // counted so the idle queue reads as "deferred + N predicate-proven
                // lines to clear", not a silent stall. Fresh drainable operator
                // prompts remain authoritative and are never classified through
                // this path.
                let noise = content
                    .as_deref()
                    .map(agent_doc_queue::queue_continuation::queue_stale_noise_lines)
                    .unwrap_or(0);
                // #qfocsup: the in-session loop has no drainable head, but a
                // supervisor-scope head may still remain for the SUPERVISOR
                // clear-and-continue path. This includes tagged focused/clean heads
                // and untagged subagent or preset heads. The queue is NOT
                // operator-stalled; the agent yields and the supervisor
                // force-`/clear`s + re-dispatches it.
                let supervisor_head = content
                    .as_deref()
                    .and_then(|content| supervisor_scope_unclaimed_head(file, content));
                // #supdrainlive (GH #73): a hand-off is only valid if the receiver
                // can receive. Gate the yield on supervisor liveness; when it cannot
                // drain, admit a human is needed instead of asserting "NOT a stall".
                let supervisor_drain = supervisor_head.map(|head| {
                    let readiness =
                        agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc(
                            file,
                        );
                    (head, readiness)
                });
                if no_in_session_continuation_resolves_prior_stall(
                    supervisor_drain.as_ref().map(|(_, readiness)| *readiness),
                ) {
                    clear_queue_drain_stall_after_valid_closeout(
                        file,
                        "clean_closeout_no_in_session_continuation",
                    );
                }
                if let Some((supervisor_head, readiness)) = supervisor_drain {
                    let head_description = supervisor_drain_head_description(
                        content.as_deref().unwrap_or_default(),
                        &supervisor_head,
                    );
                    let kind = supervisor_drain_outcome_kind(readiness);
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(kind)
                        .expect("static supervisor-drain outcome is valid")
                        .log_fields();
                    log_supervisor_drain_handoff(
                        file,
                        &supervisor_head,
                        readiness,
                        &outcome_fields,
                    );
                    if readiness.is_ready() {
                        println!(
                            "queue_continuation_required=false queue_deferred_heads={} queue_stale_noise_lines={} supervisor_drain_readiness={} {}",
                            deferred,
                            noise,
                            readiness.reason(),
                            outcome_fields
                        );
                        // #supdrainyieldfalsifiable: make the yield falsifiable —
                        // the next preflight reports a head that outlived it.
                        agent_doc_controller_io::project_controller::record_supervisor_drain_handoff(
                            file,
                            &supervisor_head,
                        );
                        eprintln!(
                            "[session-check] queue continues via supervisor: {head_description} remains for the CP/supervisor clear-and-continue path (force /clear + re-dispatch to a fresh session). End this turn so the supervisor takes over — NOT an operator stall ({}; #qfocsup). {}",
                            file.display(),
                            outcome_fields
                        );
                    } else {
                        // #focusedfallback: the supervisor cannot take the
                        // head, so the in-session loop does. Handing it to the
                        // operator was a stall.
                        println!(
                            "queue_continuation_required=true next_queue_prompt={:?} supervisor_drain_readiness={}",
                            agent_doc_queue::queue_continuation::supervisor_only_head_prompt_text(
                                content.as_deref().unwrap_or_default()
                            )
                            .unwrap_or_else(|| supervisor_head.clone()),
                            readiness.reason(),
                        );
                        eprintln!(
                            "{} {}",
                            supervisor_drain_unavailable_message_for_description(
                                file,
                                readiness,
                                head_description,
                            ),
                            outcome_fields
                        );
                        let stall_cycle_id =
                            agent_doc_cycle_state_io::load_with_closeout_projection(file)
                                .ok()
                                .flatten()
                                .map(|s| s.cycle_id)
                                .unwrap_or_default();
                        if let Err(err) = agent_doc_controller_io::project_controller::record_queue_drain_stall_continuation_pending_for_file(
                            file,
                            &stall_cycle_id,
                            &supervisor_head,
                            1,
                        ) {
                            eprintln!(
                                "[session-check] warning: failed to record continuation projection: {err}"
                            );
                        }
                    }
                } else if deferred > 0 || noise > 0 {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForOperatorProof,
                    )
                    .expect("static deferred operator-proof outcome is valid")
                    .log_fields();
                    println!(
                        "queue_continuation_required=false queue_deferred_heads={} queue_stale_noise_lines={} {}",
                        deferred, noise, outcome_fields
                    );
                    eprintln!(
                        "[session-check] queue idle: {} head(s) deferred (operator-verify), {} predicate-proven noise line(s) — operator-gated heads need a human / predicate-proven noise can be cleared with `agent-doc queue prune-noise` ({}; #goqueuestall/#goqstall2/#freshqueueauth). {}",
                        deferred,
                        noise,
                        file.display(),
                        outcome_fields
                    );
                } else {
                    let outcome_fields = agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
                    )
                    .expect("static no-drainable-work outcome is valid")
                    .log_fields();
                    println!("queue_continuation_required=false {outcome_fields}");
                }
            }
            // #finalize-owned-pane-response-patchback: proactive final-gate
            // block. When a Codex same-pane recursive invocation was refused
            // (abandoned cycle with last_event starting
            // "recursive_direct_invocation_blocked") but no response body was
            // captured, the agent may still produce a final chat answer that
            // bypasses `agent-doc write` / `finalize`. Block the final answer
            // so the operator must pipe the response through binary-owned
            // closeout.
            //
            // Recovery adoption: if the response was already patched into
            // agent:exchange (no unresolved prompt), the abandoned cycle is
            // recoverable — adopt the visible response idempotently instead of
            // blocking.
            if codex_final_gate
                && let Some(cycle) = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                    .ok()
                    .flatten()
                && matches!(cycle.phase, agent_doc_turn::CyclePhase::Abandoned)
                && OpsLogEvent::RecursiveDirectInvocationBlocked.is_line(&cycle.last_event)
                && cycle.capture_id.is_none()
                && cycle.response_sha256.is_none()
            {
                let has_visible_response = crate::unresolved_exchange_prompt(file)?.is_none()
                    && crate::exchange_tail_has_response_heading(file)?;
                if has_visible_response {
                    eprintln!(
                        "[session-check] codex-final-gate: recursive direct invocation was blocked for {} but the response is already visible in agent:exchange — adopting the manual patchback idempotently.",
                        file.display()
                    );
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "codex_final_gate_manual_patchback_adopted file={} cycle_id={} last_event={}",
                            file.display(),
                            cycle.cycle_id,
                            cycle.last_event
                        ),
                    );
                } else {
                    eprintln!(
                        "[session-check] codex-final-gate: recursive direct invocation was blocked for {} with no captured response body — pipe the response through `agent-doc write --commit {}` before sending any final answer.",
                        file.display(),
                        file.display()
                    );
                    crate::profile::report_now(file);
                    std::process::exit(2);
                }
            }
            Ok(())
        }
        SessionCheckStatus::SteeringPending(message) => {
            report_steering_pending(file, &message, codex_final_gate);
            Ok(())
        }
        SessionCheckStatus::Interrupted(message) => Err(SessionCheckInterrupted(message).into()),
    }
}

/// `#chatprompt` (GH #125 gap 4): the committed cycle carried chat prompts the
/// document has no `> **Chat prompt (#chatprompt):**` record of. A warning with
/// the repair, never a failure: the work may be done and committed; only its
/// record in the session document is missing.
pub fn unrecorded_chat_prompt_warning(file: &Path) -> Option<String> {
    let cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()?;
    if cycle.phase != CyclePhase::Committed || cycle.chat_prompts.is_empty() {
        return None;
    }
    let content =
        crate::resolve_current_document_content(file, "session_check_chat_prompt_record").ok()?;
    let missing = agent_doc_prompt_contract::chat_prompt::chat_prompts_missing_record(
        &content,
        &cycle.chat_prompts,
    );
    let warning = agent_doc_prompt_contract::chat_prompt::unrecorded_chat_prompt_closeout_warning(
        &file.display().to_string(),
        &missing,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_chat_prompt_unrecorded file={} cycle_id={} count={} (#chatprompt)",
            file.display(),
            cycle.cycle_id,
            missing.len()
        ),
    );
    Some(warning)
}

fn warn_unrecorded_chat_prompts(file: &Path) {
    if let Some(warning) = unrecorded_chat_prompt_warning(file) {
        eprintln!("{warning}");
    }
}

/// `#steerinterruptexit`: print a pending-steering status as a successful
/// check. The queue continuation is deferred behind the steering, exactly like
/// `#prompt-preempts-auto-queue`: the next `agent-doc <FILE>` admits the
/// steering as its prompt, and queue work resumes in operator order after it.
///
/// Under `--codex-final-gate` the check exits `2`, the gate's "work is still
/// owed" code that an active queue continuation already uses: pending steering
/// must be handled before a final answer, but it is not a failed closeout (`1`).
fn report_steering_pending(file: &Path, message: &str, codex_final_gate: bool) {
    println!("{message}");
    println!("queue_continuation_required=false steering_pending=true");
    eprintln!(
        "[session-check] queue continuation deferred for {}: pending operator steering runs first; the next `agent-doc {}` admits it as its prompt (#steerinterruptexit).",
        file.display(),
        file.display(),
    );
    if codex_final_gate {
        eprintln!(
            "[session-check] codex-final-gate: operator steering is pending for {} — handle each item by its dispatch (continue with `agent-doc {}`) before sending any final answer.",
            file.display(),
            file.display(),
        );
        crate::profile::report_now(file);
        std::process::exit(2);
    }
}

fn resume_captured_finalize_for_recovery(
    file: &Path,
    effects: &impl SessionCheckEffects,
    recovered_log_event: &str,
) -> Result<bool> {
    let cycle_state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let resumable = cycle_state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            CyclePhase::ResponseCaptured
                | CyclePhase::WriteApplied
                | CyclePhase::Committed
                | CyclePhase::Abandoned
        )
    });
    if !resumable {
        return Ok(false);
    }
    match effects.resume_captured_finalize(file)? {
        CapturedFinalizeResumeOutcome::Committed | CapturedFinalizeResumeOutcome::Superseded => {
            agent_doc_ops_log_io::log_op(file, recovered_log_event);
            Ok(true)
        }
        CapturedFinalizeResumeOutcome::NotApplicable
        | CapturedFinalizeResumeOutcome::Retained { .. }
        | CapturedFinalizeResumeOutcome::RetryAt { .. } => Ok(false),
    }
}

fn resume_captured_finalize_before_terminal_convergence(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<bool> {
    resume_captured_finalize_for_recovery(
        file,
        effects,
        "session_check_divergence_recovered_from_retained_capture",
    )
}

pub fn inspect(file: &Path, effects: &impl SessionCheckEffects) -> Result<SessionCheckStatus> {
    Ok(inspect_with_warnings(file, effects)?.status)
}

pub fn inspect_read_only(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<SessionCheckStatus> {
    let read_only = ReadOnlySessionCheckEffects { inner: effects };
    inspect(file, &read_only)
}

pub fn inspect_with_warnings(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<SessionCheckReport> {
    // `#sccurrentpass`: callers outside `run_with_options` (compact/write
    // closeout) get the same one-version-per-sweep memo. Nested inside a pass
    // this reuses the outer one.
    crate::with_current_document_pass(|| inspect_with_warnings_inner(file, effects))
}

fn inspect_with_warnings_inner(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<SessionCheckReport> {
    let mut steering = None;
    let report = inspect_with_warnings_sweep(file, effects, &mut steering)?;
    Ok(steering_survives_steering_carried_drift(report, steering))
}

/// `#steerinterruptexit`: the guard sweep runs over pending steering so a
/// genuine integrity finding still fails the check. A finding whose own
/// recovery is the pending steering turn (`Recovery [pending_operator_steering]`:
/// "that turn's commit carries the remaining drift") is not a second failure,
/// though: it is the same steering, seen as drift. It stays visible as a
/// warning and the status stays `steering pending`.
fn steering_survives_steering_carried_drift(
    mut report: SessionCheckReport,
    steering: Option<String>,
) -> SessionCheckReport {
    let Some(steering) = steering else {
        return report;
    };
    if let SessionCheckStatus::Interrupted(message) = &report.status
        && message.contains(&format!(
            "Recovery [{}]",
            agent_doc_turn::turn_admission::PENDING_OPERATOR_STEERING_RECOVERY
        ))
    {
        report.warnings.push(message.replacen(
            "INTERRUPTED:",
            "warn (carried by the steering turn):",
            1,
        ));
        report.status = SessionCheckStatus::SteeringPending(steering);
    }
    report
}

fn inspect_with_warnings_sweep(
    file: &Path,
    effects: &impl SessionCheckEffects,
    steering_message: &mut Option<String>,
) -> Result<SessionCheckReport> {
    let mut report = SessionCheckReport {
        status: inspect_core(file, effects)?,
        warnings: Vec::new(),
    };
    report.status = retained_write_gate_status(
        report.status,
        effects.retained_document_write_blocks(file),
        file,
    );
    // `#steerinterruptexit`: pending steering is not a failure, so the integrity
    // guard sweep still runs over it and a genuine finding still wins.
    let steering_pending = matches!(report.status, SessionCheckStatus::SteeringPending(_));
    if let SessionCheckStatus::SteeringPending(message) = &report.status {
        *steering_message = Some(message.clone());
    }
    if matches!(report.status, SessionCheckStatus::Ok(_)) || steering_pending {
        // Build one CycleContext for the guard sweep and seed it with the resolved
        // CurrentDocument. Guards that need content, frontmatter, or components
        // read from that lazily graph instead of independently resolving and
        // parsing the current document.
        let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
        // #rtwwire (rung 3): seed the guard-sweep cache from the authoritative
        // current document. Active editors resolve through the CRDT relay; disk
        // is consulted only when no editor is attached.
        let current_document = crate::resolve_current_document(file, "session_check_guard_sweep")?;
        // Phase 1 lossless-tree shadow projection (#lzlosstree): audit the tree
        // round-trip against the same authoritative text the guard sweep uses.
        // Logged, never load-bearing — the flat CRDT stays the closeout authority.
        crate::profile::timed("shadow_audit_lossless_roundtrip", || {
            agent_doc_ops_log_io::log_op(
                file,
                &agent_doc_markdown_lossless::shadow_audit_ops_log_line(
                    current_document.content(),
                    "session_check_guard_sweep",
                ),
            );
        });
        rc.set_current_document(current_document);
        match crate::profile::timed("guard_dropped_exchange_prompt", || {
            crate::check_dropped_exchange_prompt_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::profile::timed("guard_dropped_queue_prompt", || {
            crate::check_dropped_queue_prompt_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::profile::timed("guard_queue_response_contamination", || {
            crate::check_queue_response_contamination_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        if let Some(message) = crate::profile::timed("guard_completed_pending_reap", || {
            crate::check_completed_pending_reap_guard(file, &rc)
        })? {
            report.status = SessionCheckStatus::Interrupted(message);
            return Ok(report);
        }
        match crate::profile::timed("guard_shadow_backlog", || {
            crate::check_shadow_backlog_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::profile::timed("guard_malformed_tracked_item", || {
            crate::check_malformed_tracked_item_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::profile::timed("guard_backlog_replay", || {
            crate::check_backlog_replay_guard(file, &rc)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_snapshot_committed_guard(file, &rc, |file| {
            effects.closeout_recovery_hint(file)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_parent_submodule_pointer_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_committed_without_response_body_guard(file, |file| {
            effects.closeout_recovery_hint(file)
        })? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_no_response_active_queue_head(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match crate::check_reaped_queue_head_without_response(file, &rc)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        // A prompt-only exchange tail IS the pending steering already reported;
        // it is not a second, failing finding about the committed cycle.
        match if steering_pending {
            GuardResult::None
        } else {
            crate::profile::timed("guard_prompt_only_exchange_tail", || {
                crate::check_prompt_only_exchange_tail_guard(file, &rc)
            })?
        } {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        for guard in [
            crate::profile::timed("guard_pending_capture", || {
                crate::check_pending_capture_guard(file, &rc)
            })?,
            crate::profile::timed("guard_coined_ids", || {
                crate::check_coined_ids_guard(file, &rc)
            })?,
            crate::profile::timed("guard_pending_done", || {
                crate::check_pending_done_guard(file, &rc)
            })?,
            crate::profile::timed("guard_expect_done_or_gate", || {
                crate::check_expect_done_or_gate_guard(file, &rc)
            })?,
            crate::profile::timed("guard_partial_closeout_state", || {
                crate::check_partial_closeout_state_guard(file)
            })?,
            crate::profile::timed("guard_partial_staging_closeout", || {
                crate::check_partial_staging_closeout_guard(file)
            })?,
            crate::profile::timed("guard_blocked_closeout_followup", || {
                crate::check_blocked_closeout_followup_guard(file, &rc)
            })?,
            crate::profile::timed("guard_gated_phase_split", || {
                crate::check_gated_phase_split_guard(file, &rc)
            })?,
            crate::profile::timed("guard_queue_audit_partial_completion", || {
                crate::check_queue_audit_partial_completion_guard(file)
            })?,
            crate::profile::timed("guard_queue_head_removal", || {
                crate::check_queue_head_removal_guard(file, &rc)
            })?,
            crate::profile::timed("guard_free_text_queue_head_provenance", || {
                crate::check_free_text_queue_head_provenance(file, &rc)
            })?,
        ] {
            match guard {
                GuardResult::None => {}
                GuardResult::Warn(lines) => report.warnings.extend(lines),
                GuardResult::Error(message) => {
                    report.status = SessionCheckStatus::Interrupted(message);
                    break;
                }
            }
        }
        if let Ok(Some(miss)) = agent_doc_supervisor_io::startup_miss::load_startup_miss(file) {
            if let Some(supersession) =
                agent_doc_supervisor_io::startup_miss::superseded_by_newer_registered_start(
                    agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
                    file,
                    &miss,
                )?
            {
                agent_doc_supervisor_io::startup_miss::clear_startup_miss(file)?;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "session_check_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} latest_start_timestamp={}",
                        file.display(),
                        miss.pane_id,
                        supersession.registered_pane,
                        supersession.latest_start_timestamp
                    ),
                );
            } else {
                let detail = agent_doc_supervisor_io::startup_miss::session_log_diagnostic(
                    file,
                    &miss.session_id,
                )
                .ok()
                .flatten()
                .map(|detail| format!("; {detail}"))
                .unwrap_or_default();
                report.warnings.push(format!(
                    "[session-check] WARNING: startup-miss marker exists for pane {} ({:?}) — the last {} start never acknowledged a document cycle{}",
                    miss.pane_id, miss.origin, miss.harness, detail
                ));
            }
        }
    }
    // #fccsupwarn: surface a stale hosting controller/supervisor at closeout too, so a
    // drifted post-commit cycle points the operator at a recycle instead of re-filing a
    // File-Cache-Conflict dialog. Fail-open — any status/stat error yields no warning.
    if let Some(message) =
        agent_doc_controller_io::project_controller::stale_supervisor_warning_for_doc(file)
    {
        report
            .warnings
            .push(format!("[session-check] WARNING: {message}"));
    }
    Ok(report)
}

fn retained_write_gate_status(
    status: SessionCheckStatus,
    retained_write_blocks: bool,
    file: &Path,
) -> SessionCheckStatus {
    // A retained write blocks pending steering too: the steering is the next
    // cycle's input, and preflight refuses that cycle until the write settles.
    if retained_write_blocks
        && matches!(
            status,
            SessionCheckStatus::Ok(_) | SessionCheckStatus::SteeringPending(_)
        )
    {
        SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: retained document-write delivery remains unsettled for {}; automatic controller reconciliation remains scheduled. Refusing a false clean closeout because preflight would block the same effect. Run only `agent-doc session-check {}` after recovery settles; do not resubmit finalize/write, force disk, or replace the queued edit.",
            file.display(),
            file.display(),
        ))
    } else {
        status
    }
}

///
/// Kept out of the read-only `inspect*` path on purpose: only the mutating
/// command entrypoints (`enforce_clean_closeout` on the finalize boundary,
/// `run_with_options` for direct-exec `agent-doc session-check`) repair in place.
fn self_heal_late_ipc_overapplication(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<bool> {
    let Some(overapplication) = detect_late_ipc_response_overapplication(file)? else {
        return Ok(false);
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "late_ipc_response_overapplication_self_healed file={}",
            file.display()
        ),
    );
    eprintln!(
        "[session-check] late_ipc_overapplication: self-healing — restoring committed HEAD over the re-added duplicate response for {}",
        file.display()
    );
    effects.atomic_write(file, &overapplication.remediated_content)?;
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &overapplication.remediated_content,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(true)
}

fn self_heal_response_replay_duplication(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<bool> {
    let current = crate::resolve_current_document_content(
        file,
        "session_check_response_replay_dedup_current",
    )?;
    let Some(normalized) =
        agent_doc_document_realtime_io::normalize_recoverable_response_replay_duplication_for_file(
            file,
            &current,
            "session_check_response_replay_dedup",
        )?
    else {
        return Ok(false);
    };
    // `#dedupnoopretain`: a "repair" byte-identical to current content is not a
    // repair. Publishing it routes a write whose target already equals the
    // authority, so there is no visible delta for an editor to acknowledge and
    // the intent is retained forever — every later `session-check` re-reports a
    // terminal failure and the document can never open a new cycle.
    //
    // Observed 2026-08-08 on `tasks/agent-doc/agent-doc-bugs2.md`: the normalizer
    // logged `session_check_response_replay_dedup_crdt_relay_exact_noop`, the
    // caller routed the write anyway (`write_authority ... len=25480
    // hash=7d5b76e…`), an IDE restart registered mid-flight
    // (`repair_projection_late_editor_retained`), and the disk content hash was
    // *already* exactly the retained target. HEAD was correct throughout; only
    // this self-heal was wedged, and it wedged the document with it.
    //
    // The normalizer returning `Some` means "a duplication shape was
    // recognized", NOT "a change is required" — an unrepairable shape (two
    // full-bodied copies, where only empty replay shells are removable)
    // legitimately normalizes to itself. Nothing to publish, so say so.
    if normalized == current {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "session_check_response_replay_dedup_noop_skipped file={} content_hash={} #dedupnoopretain note=normalized cut is byte-identical to current authority; skipping the write that would retain forever",
                file.display(),
                agent_doc_hash::content_hash(&current),
            ),
        );
        return Ok(false);
    }
    // Supersede the stale replay intent before publishing its normalized cut.
    // Otherwise a failed/late delivery can restore the duplicate immediately
    // after this repair and make every session-check repeat the same CRDT write.
    agent_doc_document_realtime_io::reconcile_deferred_write_to_canonical_cut_if_needed(
        file,
        &normalized,
        "session_check_response_replay_dedup",
    )?;
    let repaired = effects.atomic_repair_write_if_current(
        file,
        &normalized,
        &current,
        "session_check_response_replay_dedup",
    )?;
    let repaired_settlement =
        agent_doc_document_realtime_io::classify_response_replay_repair_settlement_for_file(
            file,
            &normalized,
            &repaired,
            "session_check_response_replay_repaired_settlement",
        )?;
    anyhow::ensure!(
        matches!(
            repaired_settlement,
            agent_doc_document_realtime::write_policy::SemanticRepairSettlement::ExactTarget
                | agent_doc_document_realtime::write_policy::SemanticRepairSettlement::AdvancedCanonicalAuthority
        ),
        "[session-check] response replay deduplication for {} returned an authority projection that still contains the replay",
        file.display(),
    );
    let settled = crate::resolve_current_document_content(
        file,
        "session_check_response_replay_dedup_settled",
    )?;
    let settled_decision =
        agent_doc_document_realtime_io::classify_response_replay_repair_settlement_for_file(
            file,
            &normalized,
            &settled,
            "session_check_response_replay_terminal_settlement",
        )?;
    anyhow::ensure!(
        matches!(
            settled_decision,
            agent_doc_document_realtime::write_policy::SemanticRepairSettlement::ExactTarget
                | agent_doc_document_realtime::write_policy::SemanticRepairSettlement::AdvancedCanonicalAuthority
        ),
        "[session-check] response replay deduplication for {} returned while the replay remained in authority",
        file.display(),
    );
    if settled_decision
        == agent_doc_document_realtime::write_policy::SemanticRepairSettlement::AdvancedCanonicalAuthority
    {
        agent_doc_document_realtime_io::reconcile_deferred_write_to_canonical_cut_if_needed(
            file,
            &settled,
            "session_check_response_replay_advanced_authority",
        )?;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_check_response_replay_duplication_self_healed file={} content_hash={} settlement={settled_decision:?}",
            file.display(),
            agent_doc_hash::content_hash(&settled),
        ),
    );
    Ok(true)
}

fn response_replay_canonicalization_pending_commit(file: &Path) -> Result<bool> {
    let current = crate::resolve_current_document_content(
        file,
        "session_check_response_replay_commit_proof_current",
    )?;
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    if head == current {
        return Ok(false);
    }
    let normalized =
        agent_doc_document_realtime_io::normalize_recoverable_response_replay_duplication_for_file(
            file,
            &head,
            "session_check_response_replay_commit_proof_head",
        )?;
    // The commit boundary stages only this independently proven HEAD repair.
    // Current authority may also contain a concurrent operator cut; requiring
    // byte equality here would strand the safe repair after a process restart
    // even though committing it cannot absorb or overwrite that operator work.
    Ok(normalized.is_some_and(|candidate| candidate != head))
}

fn validate_integrity_after_captured_resume(
    file: &Path,
    source: &str,
    effects: &impl SessionCheckEffects,
) -> Result<String> {
    self_heal_response_replay_duplication(file, effects)?;
    let recovered = crate::resolve_current_document_content(file, source)?;
    agent_doc_lint_io::validate_integrity_on_content_with_logger(
        file,
        &recovered,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(recovered)
}

pub fn enforce_clean_closeout(file: &Path, effects: &impl SessionCheckEffects) -> Result<()> {
    enforce_clean_closeout_with_force_disk(file, false, effects)
}

pub fn enforce_clean_closeout_with_force_disk(
    file: &Path,
    force_disk: bool,
    effects: &impl SessionCheckEffects,
) -> Result<()> {
    // `#preflightprojpass`: open the same projection pass `run_with_options`
    // opens. This is the EMBEDDED closeout path — what `respond` /
    // `write --commit` run — and it went through the identical guard suite with
    // no pass installed, so every guard paid a full document resolve.
    //
    // Measured 2026-08-09 on 0.35.204, once `pass=` made the outcome readable:
    // 190 of 226 resolves were `uninstalled` — no pass open at all — against
    // only 12 genuine `miss`. The redundancy was never invalidation tuning; the
    // pass simply was not there. Note what that means for the ~11
    // `invalidate_current_document_pass` calls in this file: they are inert
    // while no pass is installed, so they have been correct-but-unused here,
    // and opening the pass is what activates the design they were written for.
    //
    // Safe in the order this item recorded: the one mutating call on this path
    // (`recover_retained_document_write`) already self-invalidates before any
    // later read, so a post-mutation read observes the new text.
    crate::with_current_document_pass(|| {
        crate::with_force_disk_resolution(force_disk, || {
            enforce_clean_closeout_inner(file, effects)
        })
    })
}

fn enforce_clean_closeout_inner(file: &Path, effects: &impl SessionCheckEffects) -> Result<()> {
    if let Some(message) =
        agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
            file,
            "closeout_proof_start",
        )
    {
        eprintln!("[session-check] WARNING: {message}");
    }
    self_heal_late_ipc_overapplication(file, effects)?;
    if effects.recover_retained_document_write(file)? {
        crate::invalidate_current_document_pass(file);
    }
    let report = inspect_with_warnings(file, effects)?;
    for warning in report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(_) => Ok(()),
        // `#steerinterruptexit`: the closeout succeeded; the steering is the
        // next cycle's input. The terminal report names it here, and the
        // `#closeout-steering` block after it delivers the items once.
        SessionCheckStatus::SteeringPending(message) => {
            eprintln!("{message}");
            Ok(())
        }
        SessionCheckStatus::Interrupted(message) => anyhow::bail!(message),
    }
}

pub fn detect_uncommitted_closeout_drift(
    file: &Path,
    effects: &impl SessionCheckEffects,
) -> Result<Option<String>> {
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    detect_uncommitted_closeout_drift_with_context(file, &rc, effects)
}

pub fn detect_uncommitted_closeout_drift_with_context(
    file: &Path,
    rc: &agent_doc_run_context_io::CycleContext,
    effects: &impl SessionCheckEffects,
) -> Result<Option<String>> {
    if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file) {
        let closeout_committed = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
            .is_some_and(|state| state.phase == CyclePhase::Committed);
        if !closeout_committed {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_check_drift_deferred_to_pending_write file={} intent_id={} target_hash={} reason={}",
                    file.display(),
                    pending.intent_id,
                    pending.target_hash,
                    pending.reason.token(),
                ),
            );
            return Ok(None);
        }
    }
    if effects
        .repair_committed_historical_snapshot_drift(file)?
        .is_some()
    {
        return Ok(None);
    }
    if let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? {
        return Ok(Some(agent_doc_git::parent_submodule_pointer_message(
            &drift.relative_path,
            drift.parent_head.as_deref(),
            &drift.submodule_head,
            &file.display().to_string(),
        )));
    }
    // Phase 3 (#jbccc3): jb_cache_conflict_cancel is auto-recoverable through
    // `git::commit`. Skip the lower-precision `detect_bypassed_response_write`
    // and `SnapshotDiffersFromHead` paths below so neither this caller nor
    // standalone `session-check` accuses the user of a direct patchback when
    // the binary-owned write path actually applied the response but the commit
    // boundary never landed. Preflight's `enforce_no_uncommitted_closeout_drift`
    // separately runs `git::commit` to close the cycle.
    if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
        return Ok(None);
    }
    if let Some(marker) = crate::detect_bypassed_response_write(file)? {
        return Ok(Some(format!(
            "found likely direct response patchback without agent-doc cycle: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            effects.closeout_recovery_hint(file)
        )));
    }
    if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
        // `#admissionsteeringagree`: the same predicate `session-check` answers
        // the committed-cycle verdict with. Steering admits; drift waits for
        // that turn's commit.
        if crate::turn_admission(file, false)?.continues_with_steering() {
            return Ok(None);
        }
        return Ok(Some(format!(
            "document has uncommitted exchange changes beyond the committed snapshot: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            effects.closeout_recovery_hint(file)
        )));
    }
    match rc.snapshot_commit_status() {
        agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            // `#admissionsteeringagree` (GH #118): one predicate with
            // `session-check`. A fresh operator prompt after a closed cycle is the
            // next turn's input, never a reason to refuse admission.
            if crate::turn_admission(file, false)?.continues_with_steering() {
                return Ok(None);
            }
            let identity = rc
                .snapshot_content()
                .zip(rc.head_content())
                .map(|(snapshot, head)| snapshot_head_drift_identity(&snapshot, &head))
                .unwrap_or_else(|| "snapshot_sha256=unavailable, head_sha256=unavailable, first_differing_line=unavailable".to_string());
            Ok(Some(format!(
                "snapshot differs from HEAD without an open or recoverable agent-doc cycle (snapshot_len={}, head_len={}, {}; {}; recovery_rejected=no_open_cycle_or_recognized_safe_binary_owned_drift; inspect=`agent-doc doctor {} --json`){} {}",
                snapshot_len,
                head_len,
                identity,
                closed_cycle_admission_note(file),
                file.display(),
                agent_doc_git_io::status::tracked_side_effect_note(file)?,
                effects.closeout_recovery_hint(file)
            )))
        }
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoSnapshot
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoHead
        | agent_doc_snapshot_io::SnapshotCommitStatus::NotInGitRepo => Ok(None),
    }
}

/// GH #118 ask 3: "without an open or recoverable agent-doc cycle" is true of a
/// committed cycle and reads as "nothing is in flight". Say which closed cycle
/// this is and that the admission predicate observed no unanswered operator
/// steering, so a refusal can never be mistaken for "there is a pending prompt,
/// but stop anyway".
fn closed_cycle_admission_note(file: &Path) -> String {
    let cycle = match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(Some(state)) => format!(
            "last_cycle=`{}` phase={} last_event={}",
            state.cycle_id,
            state.phase.as_str(),
            state.last_event
        ),
        Ok(None) => "last_cycle=none".to_string(),
        Err(err) => format!(
            "last_cycle=unavailable ({})",
            format!("{err:#}").replace('\n', " ")
        ),
    };
    format!(
        "{cycle}; turn_admission={} (no unanswered operator steering observed)",
        agent_doc_turn::turn_admission::TurnAdmission::RequireCleanCloseout.as_str()
    )
}

fn snapshot_head_drift_identity(snapshot: &str, head: &str) -> String {
    let snapshot_lines = snapshot.lines().collect::<Vec<_>>();
    let head_lines = head.lines().collect::<Vec<_>>();
    let first_differing_line = snapshot_lines
        .iter()
        .zip(&head_lines)
        .position(|(snapshot_line, head_line)| snapshot_line != head_line)
        .map(|index| index + 1)
        .or_else(|| {
            (snapshot_lines.len() != head_lines.len())
                .then(|| snapshot_lines.len().min(head_lines.len()) + 1)
        });
    format!(
        "snapshot_sha256={}, head_sha256={}, first_differing_line={}",
        agent_doc_hash::content_hash(snapshot),
        agent_doc_hash::content_hash(head),
        first_differing_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "none".to_string())
    )
}

fn projected_open_closeout_message(
    file: &Path,
    projection: &agent_doc_cycle_state_io::ProjectedCloseoutState,
    phase: CyclePhase,
) -> String {
    let cycle_id = projection.cycle_id.as_deref().unwrap_or("unknown");
    let detail = match phase {
        CyclePhase::PreflightStarted => "cycle started but no write/commit followed",
        CyclePhase::ResponseCaptured => "response captured but no write/commit followed",
        CyclePhase::WriteApplied => "response write landed but no commit followed",
        CyclePhase::Committed | CyclePhase::Abandoned => "cycle is terminal",
    };
    format!(
        "[session-check] INTERRUPTED: state-backbone closeout projection cycle `{}` is `{}` — {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
        cycle_id,
        phase.as_str(),
        detail,
        file.display(),
        file.display()
    )
}

pub const RETAINED_PENDING_NEEDS_OPERATOR_TOKEN: &str = "retained_pending_needs_operator=true";

struct RetainedPendingWriteMessage<'a> {
    intent_id: &'a str,
    reason: &'a str,
    source: &'a str,
    target_hash: &'a str,
    resume_reason: Option<&'a str>,
    captured_closeout: bool,
}

fn retained_pending_write_message(
    file: &Path,
    pending: RetainedPendingWriteMessage<'_>,
    ownership: agent_doc_turn::write_ownership::RetainedWriteOwnership,
) -> String {
    let RetainedPendingWriteMessage {
        intent_id,
        reason,
        source,
        target_hash,
        resume_reason,
        captured_closeout,
    } = pending;
    let resume_detail = resume_reason
        .map(|reason| {
            format!(
                " Same-capture recovery remains pending: {}.",
                reason.replace('\n', " ")
            )
        })
        .unwrap_or_default();

    let verdict = ownership.with_retained_projection(true).verdict();
    if matches!(
        verdict,
        agent_doc_turn::write_ownership::RetainedWriteVerdict::DeliveryRejected
            | agent_doc_turn::write_ownership::RetainedWriteVerdict::ReplicaUnserved
    ) {
        let kind = if captured_closeout {
            "response delivery"
        } else {
            "document projection"
        };
        return format!(
            "[session-check] INTERRUPTED: binary-owned {kind} `{intent_id}` is retained for `{}` (reason={reason}, source={source}, target_hash={target_hash}); {RETAINED_PENDING_NEEDS_OPERATOR_TOKEN}.{} The exact intent remains durable and must not be recaptured or forced to disk. {}",
            file.display(),
            resume_detail,
            agent_doc_turn::write_ownership::retained_write_remedy(
                ownership.with_retained_projection(true),
                &file.display().to_string(),
            ),
        );
    }

    // `#retainednoeditor`: "resumes automatically after editor/controller delivery
    // converges" is only true while an editor replica can still project. When the
    // editor is GONE (IDE closed or restarted), there is nothing to converge
    // with, and "retry only `agent-doc session-check`" becomes an instruction
    // that can never succeed — the operator retries forever against a precondition
    // that will not arrive on its own.
    //
    // Two independent sessions hit exactly this on 2026-07-18: retained
    // `editor_projection_pending` with zero live editor replicas, escalating
    // through session-check retries, `admin recycle`, and `repair-projection`
    // before stopping. The capture is genuinely durable and force-disk is
    // genuinely wrong, so the fix is not to weaken the guard — it is to name the
    // precondition that is actually missing.
    let editor_live = agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file);
    // The same failure mode with the opposite precondition: the planes have
    // ALREADY converged and the intent still cannot settle, so there is no
    // pending delivery for a retry to observe. "Retry only session-check" then
    // loops forever against a hash the document moved past — measured on
    // 2026-07-26 through session-check retries, `admin reload-lib`, `repair`,
    // `autofix`, and the `write --commit` that `doctor` recommends (which
    // answers "empty response — nothing to write", because the body is already
    // in the working tree). Name the command that actually resolves it.
    let converged_but_unsettleable =
        agent_doc_document_realtime_io::retained_write_is_stranded(file, "session_check_guidance");
    let convergence_detail =
        retained_pending_guidance(editor_live, converged_but_unsettleable, captured_closeout);
    if captured_closeout {
        format!(
            "[session-check] INTERRUPTED: binary-owned response delivery `{}` is retained for `{}` (reason={}, source={}, target_hash={}); the same capture will resume automatically after editor/controller delivery converges.{}{} Do not issue another closeout payload and do not force disk; retry only `agent-doc session-check {}`.",
            intent_id,
            file.display(),
            reason,
            source,
            target_hash,
            resume_detail,
            convergence_detail,
            file.display(),
        )
    } else {
        format!(
            "[session-check] INTERRUPTED: binary-owned document projection `{}` is retained for `{}` (reason={}, source={}, target_hash={}); the same intent will resume automatically after editor/controller delivery converges.{}{} Do not issue a closeout payload and do not force disk; retry only `agent-doc session-check {}`.",
            intent_id,
            file.display(),
            reason,
            source,
            target_hash,
            resume_detail,
            convergence_detail,
            file.display(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedPendingGuidance {
    ReopenEditor,
    AwaitCapturedReplayProjection,
    CommitConvergedProjection,
    AwaitDelivery,
}

fn retained_pending_guidance(
    editor_live: bool,
    converged_but_unsettleable: bool,
    captured_closeout: bool,
) -> &'static str {
    let guidance = if !editor_live {
        RetainedPendingGuidance::ReopenEditor
    } else if converged_but_unsettleable && captured_closeout {
        RetainedPendingGuidance::AwaitCapturedReplayProjection
    } else if converged_but_unsettleable {
        RetainedPendingGuidance::CommitConvergedProjection
    } else {
        RetainedPendingGuidance::AwaitDelivery
    };
    match guidance {
        RetainedPendingGuidance::ReopenEditor => {
            " NOTE: zero live editor replicas are registered for this document, so \
             there is currently nothing for the delivery to converge WITH — it will \
             stay retained rather than drain on its own. Reopen the document in the \
             editor so its replica re-registers; the retained capture then drains \
             automatically. The capture is durable in CRDT/Lazily state, so it is not \
             at risk while you do that."
        }
        RetainedPendingGuidance::AwaitCapturedReplayProjection => {
            " NOTE: authority and disk currently agree on bytes that do NOT contain \
             the captured response, while the exact captured replay remains retained \
             for editor delivery. `agent-doc commit` cannot succeed from this state \
             because there is no response body in its staged snapshot. Do not run \
             `commit`, submit another replay/closeout payload, or force disk; the \
             existing replay must become visible in the editor projection first."
        }
        RetainedPendingGuidance::CommitConvergedProjection => {
            " NOTE: authority and disk have ALREADY converged and still do not \
             satisfy this non-capture intent, so no delivery is in flight and \
             retrying will not change the answer. Run `agent-doc commit <FILE>` to \
             adopt the content that is already present; do NOT force disk during an \
             editor reconnect."
        }
        RetainedPendingGuidance::AwaitDelivery => "",
    }
}

fn inspect_core(file: &Path, effects: &impl SessionCheckEffects) -> Result<SessionCheckStatus> {
    inspect_core_with_captured_resume(file, effects, true)
}

fn log_slow_session_check_phase(file: &Path, phase: &str, started: &mut std::time::Instant) {
    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(250) {
        eprintln!(
            "[perf] session_check.{} file={} elapsed_ms={}",
            phase,
            file.display(),
            elapsed.as_millis()
        );
    }
    *started = std::time::Instant::now();
}

/// Time the whole run and print the per-operation breakdown when it was slow
/// (`#sessioncheckprofile`).
///
/// A wrapper rather than instrumentation at each exit: the inner function
/// returns from dozens of branches, and a profiler that only reports on some of
/// them would understate exactly the runs worth looking at.
fn inspect_core_with_captured_resume(
    file: &Path,
    effects: &impl SessionCheckEffects,
    allow_captured_resume: bool,
) -> Result<SessionCheckStatus> {
    crate::profile::timed("inspect_core", || {
        inspect_core_profiled(file, effects, allow_captured_resume)
    })
}

fn inspect_core_profiled(
    file: &Path,
    effects: &impl SessionCheckEffects,
    allow_captured_resume: bool,
) -> Result<SessionCheckStatus> {
    let mut phase_started = std::time::Instant::now();
    let initial_last_ops_event = agent_doc_ops_log_io::last_ops_event(file)?;

    if let Some(replay) = detect_jb_cache_conflict_accept_duplicate_replay(file)? {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found JetBrains File Cache Conflict accept replay duplicate at `{}`; `dedupe(current)` matches committed HEAD. Run `agent-doc preflight {}` to auto-repair, or run `agent-doc dedupe {}` followed by `agent-doc write --commit {}`.",
            replay.heading,
            file.display(),
            file.display(),
            file.display()
        )));
    }

    if let Some(heading) = detect_duplicate_response_patchback(file)? {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found consecutive duplicate response patchback at `{}`. Run `agent-doc dedupe {}` or rerun closeout so the write path can repair it before commit.",
            heading,
            file.display()
        )));
    }

    // Late-IPC reposition / stale-patch replay re-inserted the committed
    // response (possibly boundary-wrapped and non-adjacent) into the working
    // tree after it already reached HEAD. Recognize it as an over-application
    // before the generic `detect_bypassed_response_write` guard accuses the
    // operator of a manual patchback. The mutating entrypoints
    // (`enforce_clean_closeout`, `run_with_options`) self-heal this in place via
    // `self_heal_late_ipc_overapplication` before reaching here, and `preflight`
    // auto-repairs by restoring HEAD; this Interrupted return is the fallback for
    // read-only inspectors (#late-ipc-patch-response-uncommitted).
    if detect_late_ipc_response_overapplication(file)?.is_some() {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: found late-IPC committed-response over-application at `{}`; the working tree re-adds a response already in HEAD. Run `agent-doc preflight {}` to auto-repair (restores the committed HEAD), or `agent-doc write --commit {}` to recover through the normal closeout boundary.",
            file.display(),
            file.display(),
            file.display()
        )));
    }
    log_slow_session_check_phase(file, "initial_integrity_detection", &mut phase_started);

    let closeout_projection = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    let cycle_state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    log_slow_session_check_phase(file, "cycle_state_projection", &mut phase_started);
    let closeout_committed = cycle_state
        .as_ref()
        .is_some_and(|state| state.phase == CyclePhase::Committed);

    let mut captured_resume_reason = None;
    if !closeout_committed
        && allow_captured_resume
        && cycle_state.as_ref().is_some_and(|state| {
            matches!(
                state.phase,
                CyclePhase::ResponseCaptured | CyclePhase::WriteApplied | CyclePhase::Abandoned
            )
        })
    {
        // GH 90: a capture whose bytes no validator will ever accept is not
        // "pending" — resuming it is guaranteed to fail identically, and the
        // resume path would report whatever unrelated effect failed first.
        // Report the structural reason and the one command that clears it.
        if let Some(unlandable) = crate::unlandable_capture::current_unlandable_capture(file)? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_check_{} file={} cycle_id={} capture_id={} response_sha256={} action=stop_retrying recovery=\"{}\"",
                    crate::unlandable_capture::UNLANDABLE_CAPTURE_TOKEN,
                    file.display(),
                    unlandable.cycle_id,
                    unlandable.capture_id,
                    unlandable.response_sha256,
                    crate::unlandable_capture::UnlandableCapture::recovery_command(file),
                ),
            );
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: {}",
                unlandable.operator_report(file).replace('\n', " ")
            )));
        }
        match effects.resume_captured_finalize(file)? {
            CapturedFinalizeResumeOutcome::Committed
            | CapturedFinalizeResumeOutcome::Superseded => {
                return inspect_core_with_captured_resume(file, effects, false);
            }
            CapturedFinalizeResumeOutcome::NotApplicable => {}
            CapturedFinalizeResumeOutcome::Retained { reason }
            | CapturedFinalizeResumeOutcome::RetryAt { reason, .. } => {
                captured_resume_reason = Some(reason);
            }
        }
    }

    let captured_closeout = cycle_state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            CyclePhase::ResponseCaptured | CyclePhase::WriteApplied | CyclePhase::Abandoned
        )
    });
    if !closeout_committed && !captured_closeout {
        match agent_doc_document_realtime_io::settle_retained_non_capture_projection_through_authority(
            file,
            "session_check_retained_non_capture_projection_settlement",
        ) {
            Ok(true) | Ok(false) => {}
            Err(err) => {
                captured_resume_reason = Some(format!(
                    "non-capture projection is not yet safe to settle: {err:#}"
                ));
            }
        }
    }
    log_slow_session_check_phase(file, "retained_projection_settlement", &mut phase_started);
    if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file)
        && !closeout_committed
    {
        return Ok(SessionCheckStatus::Interrupted(
            retained_pending_write_message(
                file,
                RetainedPendingWriteMessage {
                    intent_id: &pending.intent_id,
                    reason: pending.reason.token(),
                    source: pending.source.token(),
                    target_hash: &pending.target_hash,
                    resume_reason: captured_resume_reason.as_deref(),
                    captured_closeout,
                },
                agent_doc_document_realtime_io::observed_retained_write_ownership(file),
            ),
        ));
    }

    if let Some(reason) = captured_resume_reason {
        return Ok(SessionCheckStatus::Interrupted(format!(
            "[session-check] INTERRUPTED: binary-owned captured closeout for `{}` remains pending: {}. The same capture is durable; do not issue another closeout payload and do not force disk. Retry only `agent-doc session-check {}`.",
            file.display(),
            reason.replace('\n', " "),
            file.display(),
        )));
    }

    if let Some(state) = cycle_state {
        if state.is_open() {
            if let Some(blocked) = state.blocked_closeout.as_ref() {
                return Ok(SessionCheckStatus::Interrupted(blocked_closeout_message(
                    file, &state, blocked,
                )));
            }
            let recovered_boundary = effects.recover_missing_commit_boundary(
                file,
                OpsLogEvent::SessionCheckCommitBoundaryRecovered.as_str(),
            )?;
            log_slow_session_check_phase(
                file,
                "recover_missing_commit_boundary",
                &mut phase_started,
            );
            if let Some(reason) = recovered_boundary {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` was `{}` ({}), recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        state.phase.as_str(),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` was `{}` ({}); recovered the missing commit boundary from {}",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    reason
                )));
            }
            if let Some(message) = crate::open_cycle_manual_patchback_message(file, &state)? {
                return Ok(SessionCheckStatus::Interrupted(message));
            }
            return Ok(SessionCheckStatus::Interrupted(crate::open_cycle_message(
                file, &state,
            )?));
        }
        // #codex-owned-pane-prompt-miss: a recursive same-pane direct invocation
        // that abandoned its empty cycle is terminal, but that abandon is NOT
        // sufficient closeout if an unresolved exchange prompt still remains with
        // no later response — the user prompt was never answered. Report a
        // missed-prompt recovery instead of accepting the abandoned cycle as OK.
        // (Defense in depth: the run-side early guard now bails before opening a
        // cycle in this shape, but older abandoned cycles or alternate paths must
        // still be caught here.)
        if matches!(state.phase, agent_doc_turn::CyclePhase::Abandoned)
            && OpsLogEvent::RecursiveDirectInvocationBlocked.is_line(&state.last_event)
            && let Some(unresolved) = crate::unresolved_exchange_prompt(file)?
        {
            let excerpt: String = unresolved
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(&unresolved)
                .trim()
                .chars()
                .take(200)
                .collect();
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` was abandoned by the recursive same-pane guard ({}), but an unresolved exchange prompt is still unanswered: \"{}\". Answer it in this owner pane's current turn and persist with `agent-doc finalize {}` (or `agent-doc write --commit {}`); do not re-run `agent-doc {}` from this same pane.",
                state.cycle_id,
                state.last_event,
                excerpt,
                file.display(),
                file.display(),
                file.display()
            )));
        }
        if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
            if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(committed_cycle_steering_status(
                    file,
                    &state,
                    Some(reason),
                    &prompt_marker,
                ));
            }
            return Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                state.cycle_id,
                state.phase.as_str(),
                state.last_event,
                reason
            )));
        }
        if let Some(marker) = crate::detect_bypassed_response_write(file)? {
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(committed_cycle_steering_status(
                        file,
                        &state,
                        Some(reason),
                        &prompt_marker,
                    ));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    reason
                )));
            }
            // #patchback-head-tolerant: the patchback heuristic compares the
            // snapshot against the working tree, never against HEAD — so a response
            // that WAS committed through `finalize` (it reached HEAD) but whose
            // snapshot sidecar is stale, or whose working tree was re-drifted by the
            // post-commit IPC listener (#postcommit-ipc-worktree-corruption), trips a
            // FALSE "direct response patchback without agent-doc cycle". If the flagged
            // `### Re:` heading is present in HEAD, the binary's write/commit path DID
            // run for it — it is not a bypassed patchback. Do not interrupt; fall
            // through to the remaining guards (post-commit drift etc.) which catch a
            // genuine working-tree problem without the false closeout-violation.
            let marker_committed_in_head = agent_doc_git_io::revision::show_head(file)?
                .is_some_and(|head| {
                    agent_doc_document::write_normalization::response_marker_present_in_content(
                        &head, &marker,
                    )
                });
            if marker_committed_in_head {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "session_check_patchback_tolerated_head_committed file={} marker={:?} (#patchback-head-tolerant)",
                        file.display(),
                        marker.chars().take(80).collect::<String>(),
                    ),
                );
            } else {
                return Ok(SessionCheckStatus::Interrupted(
                    agent_doc_workflow::session_check::likely_direct_response_patchback_message(
                        &marker,
                        &agent_doc_git_io::status::tracked_side_effect_note(file)?,
                        &effects.closeout_recovery_hint(file),
                    ),
                ));
            }
        }
        let mut latest_head_response_visible_in_live_buffer = false;
        let latest_head_response_missing = match agent_doc_git_io::revision::show_head(file)? {
            Some(head) => {
                match crate::resolve_current_document_content(file, "latest_head_response_missing")
                {
                    Ok(working) => {
                        let heading =
                        agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
                            &head, &working,
                        );
                        match heading {
                            Some(heading)
                                if operator_live_buffer_contains_heading(file, &heading) =>
                            {
                                latest_head_response_visible_in_live_buffer = true;
                                None
                            }
                            other => other,
                        }
                    }
                    Err(_) => None,
                }
            }
            None => None,
        };
        if let Some(heading) = latest_head_response_missing {
            return Ok(SessionCheckStatus::Interrupted(
                agent_doc_workflow::session_check::committed_head_response_missing_message(
                    &file.display().to_string(),
                    &state.cycle_id,
                    state.phase,
                    &state.last_event,
                    &heading,
                ),
            ));
        }
        match crate::classify_active_session_post_commit_drift(file)? {
            // `#codexsteerinterrupt`: an editor-op-proven operator prompt added
            // after the commit is the same pending steering the non-harness
            // path reports (`#steerinterruptexit`), not harness drift.
            Some(crate::ActiveSessionPostCommitDrift::OperatorSteering { marker, .. }) => {
                return Ok(committed_cycle_steering_status(file, &state, None, &marker));
            }
            Some(crate::ActiveSessionPostCommitDrift::Unproven { detail }) => {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event,
                    detail
                )));
            }
            None => {}
        }
        if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
            if latest_head_response_visible_in_live_buffer {
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({})",
                    state.cycle_id,
                    state.phase.as_str(),
                    state.last_event
                )));
            }
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the document has uncommitted exchange changes beyond the committed snapshot: {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle before reporting success.",
                state.cycle_id,
                state.phase.as_str(),
                state.last_event,
                marker,
                file.display(),
                file.display()
            )));
        }
        // `#admissionsteeringagree` (GH #118): the verdict preflight admission
        // derives from too. Continue-with-steering here is admission there.
        if let crate::TurnAdmissionVerdict {
            steering: Some(marker),
            ..
        } = crate::turn_admission(file, state.is_open())?
        {
            return Ok(committed_cycle_steering_status(file, &state, None, &marker));
        }
        return Ok(SessionCheckStatus::Ok(format!(
            "[session-check] ok — cycle `{}` is `{}` ({})",
            state.cycle_id,
            state.phase.as_str(),
            state.last_event
        )));
    }

    if let Some(projection) = closeout_projection.as_ref()
        && let Some(phase) = projection.phase
        && phase.is_open()
    {
        return Ok(SessionCheckStatus::Interrupted(
            projected_open_closeout_message(file, projection, phase),
        ));
    }

    match initial_last_ops_event {
        None => {
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — repaired committed historical {} snapshot drift",
                    reason
                )));
            }
            if let Some(marker) = crate::detect_bypassed_response_write(file)? {
                if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — repaired committed historical {} snapshot drift",
                        reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    agent_doc_git_io::status::tracked_side_effect_note(file)?,
                    effects.closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = crate::detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: document has uncommitted exchange changes beyond the committed snapshot (no cycle state): {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
                    marker,
                    file.display(),
                    file.display()
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: document has unresolved prompt-bearing user changes but no agent-doc cycle ever started: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(
                "[session-check] no cycle state or ops.log — ok".to_string(),
            ))
        }
        Some(event) if event.starts_with(PREFLIGHT_START_EVENT) => {
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — cycle started but no write/commit followed",
                PREFLIGHT_START_EVENT
            )))
        }
        Some(event) if is_write_completed_commit_missing_event(&event) => {
            if let Some(reason) = effects.recover_missing_commit_boundary(
                file,
                OpsLogEvent::SessionCheckCommitBoundaryRecovered.as_str(),
            )? {
                let repaired_cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log entry was `{}`, recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        event_name(&event),
                        reason,
                        prompt_marker
                    )));
                }
                if let Some(state) = repaired_cycle {
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; recovered the missing commit boundary from {} into cycle `{}`",
                        event, reason, state.cycle_id
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; recovered the missing commit boundary from {}",
                    event, reason
                )));
            }
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — response write landed but no commit followed",
                event_name(&event)
            )))
        }
        Some(event) => {
            if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                    event, reason
                )));
            }
            if let Some(marker) = crate::detect_bypassed_response_write(file)? {
                if let Some(reason) = effects.repair_committed_historical_snapshot_drift(file)? {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                        event, reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    agent_doc_git_io::status::tracked_side_effect_note(file)?,
                    effects.closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = crate::detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = crate::detect_uncommitted_exchange_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the document has uncommitted exchange changes beyond the committed snapshot: {}. Run `agent-doc finalize {}` or `agent-doc write --commit {}` to close the cycle.",
                    marker,
                    file.display(),
                    file.display()
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — last event: {}",
                event
            )))
        }
    }
}

fn blocked_closeout_message(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    blocked: &agent_doc_cycle_state_io::BlockedCloseout,
) -> String {
    let editor_authority = blocked_closeout_editor_authority_note(file, blocked);
    let file_display = file.display().to_string();
    agent_doc_workflow::session_check::blocked_closeout_message(BlockedCloseoutMessage {
        file: &file_display,
        kind: &blocked.kind,
        cycle_id: &state.cycle_id,
        phase: state.phase,
        last_event: &state.last_event,
        source: &blocked.source,
        reason: &blocked.reason,
        patch_id: blocked.patch_id.as_deref(),
        recovery: blocked.recovery.as_deref(),
        detail: blocked.detail.as_deref(),
        recovery_command: blocked.recovery_command.as_deref(),
        editor_authority_note: &editor_authority,
    })
}

fn blocked_closeout_editor_authority_note(
    file: &Path,
    blocked: &agent_doc_cycle_state_io::BlockedCloseout,
) -> String {
    if blocked.kind != "editor_convergence_required" {
        return String::new();
    }
    if !agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file) {
        return String::new();
    }
    let registration =
        agent_doc_controller_io::project_controller::live_editor_registration_for_file(file)
            .ok()
            .flatten();
    if registration.as_ref().is_some_and(|registration| {
        registration
            .capabilities
            .iter()
            .any(|capability| capability == agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
    }) {
        return String::new();
    }
    let editor_id = registration
        .as_ref()
        .map(|registration| registration.editor_id.as_str())
        .unwrap_or("unknown");
    format!(
        " Live editor `{editor_id}` lacks required capability `{}`; reload or restart the editor plugin before retrying so delivery can preserve operator text.",
        agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY
    )
}

fn detect_duplicate_response_patchback(file: &Path) -> Result<Option<String>> {
    let content = crate::resolve_current_document_content(file, "duplicate_response_patchback")?;
    Ok(agent_doc_turn::response_replay::first_duplicate_response_heading(&content))
}

#[cfg(test)]
mod terminal_convergence_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn clean_closeout_resolves_prior_stall_unless_supervisor_fallback_stays_in_session() {
        use agent_doc_controller::status::SupervisorDrainReadiness;

        assert!(no_in_session_continuation_resolves_prior_stall(None));
        assert!(no_in_session_continuation_resolves_prior_stall(Some(
            SupervisorDrainReadiness::Ready { supervisor_pid: 7 }
        )));
        assert!(!no_in_session_continuation_resolves_prior_stall(Some(
            SupervisorDrainReadiness::HeartbeatStale { supervisor_pid: 7 }
        )));
    }

    #[test]
    fn snapshot_head_drift_identity_reports_hashes_and_first_line() {
        let identity = snapshot_head_drift_identity("one\ntwo\n", "one\nchanged\n");
        assert!(identity.contains("snapshot_sha256="));
        assert!(identity.contains("head_sha256="));
        assert!(identity.contains("first_differing_line=2"));
    }

    #[test]
    fn retained_write_gate_never_preserves_false_ok_status() {
        let file = Path::new("/tmp/retained-session.md");
        let gated = retained_write_gate_status(
            SessionCheckStatus::Ok("[session-check] OK".to_string()),
            true,
            file,
        );
        match gated {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("retained document-write delivery remains unsettled"));
                assert!(message.contains("preflight would block the same effect"));
                assert!(!message.contains("reload"));
            }
            SessionCheckStatus::Ok(message) | SessionCheckStatus::SteeringPending(message) => {
                panic!("retained write incorrectly preserved clean status: {message}")
            }
        }
    }

    /// `#steerinterruptexit`: a guard finding whose recovery IS the pending
    /// steering turn keeps the steering status; any other finding still fails.
    #[test]
    fn steering_carried_drift_keeps_steering_but_genuine_findings_fail() {
        let steering = "[session-check] steering pending: prompt_target: x".to_string();
        let carried = SessionCheckReport {
            status: SessionCheckStatus::Interrupted(
                "[session-check] INTERRUPTED: snapshot drift. Recovery [pending_operator_steering]: `agent-doc d.md`"
                    .to_string(),
            ),
            warnings: Vec::new(),
        };
        let kept = steering_survives_steering_carried_drift(carried, Some(steering.clone()));
        assert_eq!(
            kept.status,
            SessionCheckStatus::SteeringPending(steering.clone())
        );
        assert_eq!(kept.warnings.len(), 1, "{:?}", kept.warnings);

        let genuine = SessionCheckReport {
            status: SessionCheckStatus::Interrupted(
                "[session-check] INTERRUPTED: user-authored agent:queue edit(s) were dropped"
                    .to_string(),
            ),
            warnings: Vec::new(),
        };
        let failed = steering_survives_steering_carried_drift(genuine, Some(steering));
        assert!(failed.status.is_failure(), "{:?}", failed.status);
    }

    /// `#steerinterruptexit`: pending steering is a success status, so a
    /// retained write must gate it exactly like `Ok` — preflight would refuse
    /// the cycle that answers the steering until the write settles.
    #[test]
    fn retained_write_gate_blocks_pending_steering_like_ok() {
        let file = Path::new("/tmp/retained-session.md");
        let gated = retained_write_gate_status(
            SessionCheckStatus::SteeringPending(
                "[session-check] steering pending: prompt_target: x".to_string(),
            ),
            true,
            file,
        );
        assert!(
            matches!(&gated, SessionCheckStatus::Interrupted(message)
                if message.contains("retained document-write delivery remains unsettled")),
            "{gated:?}"
        );
    }

    #[test]
    fn retained_write_gate_preserves_existing_interruption() {
        let existing = SessionCheckStatus::Interrupted("existing interruption".to_string());
        assert_eq!(
            retained_write_gate_status(existing.clone(), true, Path::new("/tmp/doc.md")),
            existing,
        );
    }

    /// `#admissionsplitmerge`: the inverse of the old `#steeringremedydeadlock`
    /// assertion. Admission now MERGES a three-way split instead of refusing it, so
    /// `run agent-doc <FILE> to continue` is correct advice in every state — and
    /// substituting an operator-convergence remedy for it would stall a document the
    /// binary reconciles on its own. The split predicate and the notice are shared
    /// with preflight, so assert against the shared source rather than restating
    /// either string here.
    #[test]
    fn steering_guidance_keeps_the_trigger_because_admission_merges_the_split() {
        use agent_doc_document::admission_divergence::{
            AdmissionDivergence, classify, three_way_merge_notice,
        };

        assert_eq!(
            classify(Some("base"), Some("live"), "disk"),
            AdmissionDivergence::ThreeWaySplitMerged,
            "three distinct planes are the state admission merges"
        );

        let notice = three_way_merge_notice("tasks/api.md");
        assert!(
            notice.contains("three-way merge") && notice.contains("no writer is dropped"),
            "the notice must report the split as reconciled: {notice}"
        );
        assert!(
            !notice.contains("save or close this document's editor tab"),
            "it must not prescribe hand-convergence the binary performs: {notice}"
        );

        // The guidance itself is now unconditional: nothing substitutes for the
        // `run agent-doc <FILE> to continue` form, and the split observation is
        // log-only.
        let guidance = realtime_steering_closeout_guidance(std::path::Path::new("tasks/api.md"));
        assert!(
            guidance.contains("to continue"),
            "steering guidance must keep the trigger: {guidance}"
        );
        assert!(
            !guidance.contains("NOT yet answerable"),
            "no plane observation may make a steering prompt unanswerable: {guidance}"
        );
    }

    #[test]
    fn retained_projection_prevents_partial_compact_from_becoming_unanswered_edit() {
        let ownership = terminal_divergence_ownership(
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED,
            None,
            false,
            true,
            true,
        );

        assert_eq!(
            ownership.verdict(),
            agent_doc_turn::write_ownership::RetainedWriteVerdict::Deferred,
        );
        let remedy =
            agent_doc_turn::write_ownership::retained_write_remedy(ownership, "tasks/contracts.md");
        assert!(remedy.contains("retained capture or projection"));
        assert!(!remedy.contains("UNANSWERED DOCUMENT EDIT"));
        assert!(!remedy.contains("agent-doc tasks/contracts.md` to open the next cycle"));
    }

    #[test]
    fn terminal_divergence_preserves_observed_unregistered_zero_replica_route() {
        let observed = agent_doc_turn::write_ownership::RetainedWriteOwnership::new(true, true)
            .with_retained_projection(true)
            .with_delivery_rejected(true)
            .with_editor_route_unowned(true);
        let ownership = terminal_divergence_ownership(
            observed,
            Some(CyclePhase::ResponseCaptured),
            true,
            true,
            false,
        );

        assert_eq!(
            ownership.verdict(),
            agent_doc_turn::write_ownership::RetainedWriteVerdict::Stranded,
            "an unregistered route with zero live replicas has no holder"
        );
        let remedy =
            agent_doc_turn::write_ownership::retained_write_remedy(ownership, "tasks/doc.md");
        assert!(remedy.contains("UNREGISTERED"), "{remedy}");
        assert!(remedy.contains("ZERO live editor replicas"), "{remedy}");
        assert!(remedy.contains("agent-doc commit tasks/doc.md"), "{remedy}");
        assert!(!remedy.contains("controller owns the next closeout attempt"));
    }

    #[test]
    fn read_only_native_save_resumes_only_the_same_exact_write_applied_closeout() {
        let projection = ReadOnlyRetainedCloseoutResumeProjection::new(
            false,
            Some(CyclePhase::WriteApplied),
            true,
        );
        assert!(!projection.should_resume());

        projection.observe_native_save(true, true);
        assert!(projection.should_resume());

        projection.observe_native_save(true, false);
        assert!(!projection.should_resume());

        projection.observe_native_save(true, true);
        projection.observe_cycle_phase(Some(CyclePhase::ResponseCaptured));
        assert!(!projection.should_resume());

        projection.observe_cycle_phase(Some(CyclePhase::WriteApplied));
        projection.observe_retained_document_write_blocks(false);
        assert!(!projection.should_resume());
    }

    #[test]
    fn read_only_preconverged_capture_resumes_the_exact_write_applied_closeout() {
        let projection = ReadOnlyRetainedCloseoutResumeProjection::new(
            true,
            Some(CyclePhase::WriteApplied),
            true,
        );
        assert!(projection.should_resume());

        projection.observe_cycle_phase(Some(CyclePhase::ResponseCaptured));
        assert!(!projection.should_resume());

        projection.observe_cycle_phase(Some(CyclePhase::WriteApplied));
        projection.observe_retained_document_write_blocks(false);
        assert!(!projection.should_resume());
    }

    struct TestEffects;

    impl SessionCheckEffects for TestEffects {
        fn closeout_recovery_hint(&self, _file: &Path) -> String {
            String::new()
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
        }

        fn atomic_repair_write_if_current(
            &self,
            file: &Path,
            content: &str,
            expected_current: &str,
            source: &str,
        ) -> Result<String> {
            agent_doc_document_realtime_io::atomic_repair_write_if_current_through_authority(
                file,
                content,
                expected_current,
                source,
            )
        }

        fn settle_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_current: &str,
        ) -> Result<()> {
            agent_doc_document_realtime_io::settle_committed_projection_if_current_through_authority(
                file,
                committed_content,
                expected_current,
                "session_check_test_settlement",
            )
        }

        fn settle_retained_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_disk: &str,
        ) -> Result<bool> {
            agent_doc_document_realtime_io::settle_retained_committed_projection_through_authority(
                file,
                committed_content,
                expected_disk,
                "session_check_test_retained_settlement",
            )
        }

        fn repair_committed_historical_snapshot_drift(
            &self,
            _file: &Path,
        ) -> Result<Option<&'static str>> {
            Ok(None)
        }

        fn recover_missing_commit_boundary(
            &self,
            _file: &Path,
            _event: &str,
        ) -> Result<Option<&'static str>> {
            Ok(None)
        }

        fn resume_captured_finalize(&self, _file: &Path) -> Result<CapturedFinalizeResumeOutcome> {
            Ok(CapturedFinalizeResumeOutcome::NotApplicable)
        }
    }

    #[test]
    fn operator_session_check_effects_refuse_every_semantic_document_mutation() {
        let effects = ReadOnlySessionCheckEffects {
            inner: &TestEffects,
        };
        let file = Path::new("/tmp/status-only-session-check.md");

        assert!(!effects.allows_recovery());
        assert!(effects.atomic_write(file, "replacement").is_err());
        assert!(
            effects
                .atomic_repair_write_if_current(file, "replacement", "current", "test")
                .is_err()
        );
        assert!(
            effects
                .settle_committed_projection(file, "head", "current")
                .is_err()
        );
        assert!(
            !effects
                .settle_retained_committed_projection(file, "head", "disk")
                .unwrap()
        );
        assert_eq!(
            effects.resume_captured_finalize(file).unwrap(),
            CapturedFinalizeResumeOutcome::NotApplicable
        );
        assert!(!effects.recover_retained_document_write(file).unwrap());
    }

    #[test]
    fn retained_write_applied_session_check_waits_for_delivery_before_editor_owned_save() {
        assert_eq!(
            decide_read_only_terminal_projection(
                false,
                Some(CyclePhase::WriteApplied),
                true,
                false,
            ),
            ReadOnlyTerminalProjectionDecision::AwaitEditorDelivery,
        );
        assert_eq!(
            decide_read_only_terminal_projection(false, Some(CyclePhase::WriteApplied), true, true,),
            ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave,
        );
        assert_eq!(
            decide_read_only_terminal_projection(
                false,
                Some(CyclePhase::WriteApplied),
                false,
                false,
            ),
            ReadOnlyTerminalProjectionDecision::ObserveOnly,
        );
        assert_eq!(
            decide_read_only_terminal_projection(
                false,
                Some(CyclePhase::ResponseCaptured),
                true,
                true,
            ),
            ReadOnlyTerminalProjectionDecision::ObserveOnly,
        );
        assert_eq!(
            decide_read_only_terminal_projection(true, Some(CyclePhase::WriteApplied), true, false,),
            ReadOnlyTerminalProjectionDecision::Converged,
        );
    }

    /// `#unowneddivergence`: a divergence whose cycle is already terminal and
    /// which no retained write blocks has NO owner — no controller state edge
    /// can fire for it. Classifying that as `ObserveOnly` made session-check
    /// INTERRUPT forever, which stopped the queue drain and stranded the
    /// visible edits; it happened twice on tasks/software/lazily.md on
    /// 2026-08-03, both times with the newest cycle `committed` over an hour
    /// earlier and zero retained captures. Ask the editor to persist its own
    /// buffer instead, which is the only action that converges this state
    /// without the agent writing the document.
    #[test]
    fn unowned_divergence_requests_editor_save_instead_of_stalling_forever() {
        for phase in [
            None,
            Some(CyclePhase::Committed),
            Some(CyclePhase::Abandoned),
        ] {
            assert_eq!(
                decide_read_only_terminal_projection(false, phase, false, true),
                ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave,
                "terminal phase {phase:?} with no retained block must try to converge"
            );
        }
        // A retained write still has an owner, so it keeps waiting rather than
        // racing the controller with an editor save.
        assert_eq!(
            decide_read_only_terminal_projection(false, Some(CyclePhase::Committed), true, true),
            ReadOnlyTerminalProjectionDecision::ObserveOnly,
        );
        // An OPEN cycle is owned by definition — unchanged.
        assert_eq!(
            decide_read_only_terminal_projection(
                false,
                Some(CyclePhase::PreflightStarted),
                false,
                true
            ),
            ReadOnlyTerminalProjectionDecision::ObserveOnly,
        );
    }

    struct RepairOnlyEffects;

    impl SessionCheckEffects for RepairOnlyEffects {
        fn closeout_recovery_hint(&self, file: &Path) -> String {
            TestEffects.closeout_recovery_hint(file)
        }

        fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
            anyhow::bail!("proof-bearing replay repair used generic semantic write")
        }

        fn atomic_repair_write_if_current(
            &self,
            file: &Path,
            content: &str,
            expected_current: &str,
            source: &str,
        ) -> Result<String> {
            TestEffects.atomic_repair_write_if_current(file, content, expected_current, source)
        }

        fn settle_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_current: &str,
        ) -> Result<()> {
            TestEffects.settle_committed_projection(file, committed_content, expected_current)
        }

        fn settle_retained_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_disk: &str,
        ) -> Result<bool> {
            TestEffects.settle_retained_committed_projection(file, committed_content, expected_disk)
        }

        fn repair_committed_historical_snapshot_drift(
            &self,
            file: &Path,
        ) -> Result<Option<&'static str>> {
            TestEffects.repair_committed_historical_snapshot_drift(file)
        }

        fn recover_missing_commit_boundary(
            &self,
            file: &Path,
            event: &str,
        ) -> Result<Option<&'static str>> {
            TestEffects.recover_missing_commit_boundary(file, event)
        }

        fn resume_captured_finalize(&self, file: &Path) -> Result<CapturedFinalizeResumeOutcome> {
            TestEffects.resume_captured_finalize(file)
        }
    }

    struct ResumeEffects;

    impl SessionCheckEffects for ResumeEffects {
        fn closeout_recovery_hint(&self, file: &Path) -> String {
            TestEffects.closeout_recovery_hint(file)
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            TestEffects.atomic_write(file, content)
        }

        fn atomic_repair_write_if_current(
            &self,
            file: &Path,
            content: &str,
            expected_current: &str,
            source: &str,
        ) -> Result<String> {
            TestEffects.atomic_repair_write_if_current(file, content, expected_current, source)
        }

        fn settle_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_current: &str,
        ) -> Result<()> {
            TestEffects.settle_committed_projection(file, committed_content, expected_current)
        }

        fn settle_retained_committed_projection(
            &self,
            file: &Path,
            committed_content: &str,
            expected_disk: &str,
        ) -> Result<bool> {
            TestEffects.settle_retained_committed_projection(file, committed_content, expected_disk)
        }

        fn repair_committed_historical_snapshot_drift(
            &self,
            file: &Path,
        ) -> Result<Option<&'static str>> {
            TestEffects.repair_committed_historical_snapshot_drift(file)
        }

        fn recover_missing_commit_boundary(
            &self,
            file: &Path,
            event: &str,
        ) -> Result<Option<&'static str>> {
            TestEffects.recover_missing_commit_boundary(file, event)
        }

        fn resume_captured_finalize(&self, _file: &Path) -> Result<CapturedFinalizeResumeOutcome> {
            Ok(CapturedFinalizeResumeOutcome::Committed)
        }
    }

    #[test]
    fn session_check_resumes_retained_capture_even_after_target_reaches_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ reproduce retained response\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, baseline).unwrap();
        agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline)).unwrap();
        agent_doc_capture_io::capture_response_with_current_content(
            &file,
            "### Re: retained response — gpt-5\n\nRecovered body.\n",
            baseline,
        )
        .unwrap();

        assert!(
            resume_captured_finalize_before_terminal_convergence(&file, &ResumeEffects).unwrap()
        );
        assert!(
            resume_captured_finalize_before_terminal_convergence(&file, &ResumeEffects).unwrap(),
            "exact authority/disk convergence must still terminalize retained capture state"
        );
    }

    #[test]
    fn session_check_resumes_retained_effect_after_cycle_is_committed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ reproduce retained response\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, baseline).unwrap();
        agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline)).unwrap();
        agent_doc_capture_io::capture_response_with_current_content(
            &file,
            "### Re: retained response — gpt-5\n\nRecovered body.\n",
            baseline,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &file,
            "commit_success",
            Some(baseline),
            Some(baseline),
        )
        .unwrap();

        assert!(
            resume_captured_finalize_before_terminal_convergence(&file, &ResumeEffects).unwrap(),
            "Committed records can still own a deferred durable projection effect"
        );
    }

    #[test]
    fn terminal_convergence_only_settles_non_capture_or_preflight_projection() {
        assert!(terminal_phase_allows_non_capture_projection_settlement(
            None
        ));
        assert!(terminal_phase_allows_non_capture_projection_settlement(
            Some(CyclePhase::PreflightStarted,)
        ));
        for phase in [
            CyclePhase::ResponseCaptured,
            CyclePhase::WriteApplied,
            CyclePhase::Committed,
            CyclePhase::Abandoned,
        ] {
            assert!(
                !terminal_phase_allows_non_capture_projection_settlement(Some(phase)),
                "captured or terminal phase {phase:?} must stay on its identity-checked closeout path",
            );
        }
    }

    #[test]
    fn session_check_rejects_crdt_disk_divergence_after_committed_closeout() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(
            &file,
            "canonical response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n",
        )
        .unwrap();

        let err = ensure_terminal_authority_disk_convergence(
            &file,
            "canonical response\n<!-- agent:boundary:new -->\n",
            "canonical response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n",
            &TestEffects,
        )
        .expect_err("session-check must fail when authority and disk differ");
        let message = format!("{err:#}");
        assert!(message.contains("refusing a false successful closeout"));
        assert!(message.contains("scheduled automatically"));
        assert!(message.contains("`session-check` is status-only"));
        assert!(message.contains("Do not rerun `finalize`"));
        assert!(
            message.contains("supervisor recycle is fallback-only"),
            "{message}"
        );
        assert!(
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .is_none(),
            "session-check divergence must prefer targeted editor recovery",
        );
    }

    fn commit_session_doc(dir: &Path, content: &str) -> std::path::PathBuf {
        let file = dir.join("session.md");
        std::fs::write(&file, content).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "session.md"],
            vec!["commit", "-m", "committed closeout", "--no-verify"],
        ] {
            let out = Command::new("git")
                .current_dir(dir)
                .args(&args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        }
        file
    }

    const LEAD_COMMITTED: &str = "# Session\n\n<!-- agent:queue -->\n- shipped item\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\nPrior answer.\n<!-- /agent:exchange -->\n";

    /// `#operatorleadpendingsave` regression (tasks/sdk.md, 2026-10-03): the
    /// operator's queue + exchange edit lives in the editor authority while
    /// disk still holds the committed revision and the controller's save is
    /// pending. That is a pending save / pending steering, not INTERRUPTED.
    #[test]
    fn session_check_reports_operator_edits_ahead_of_disk_as_a_pending_save() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = commit_session_doc(dir.path(), LEAD_COMMITTED);
        let authority = LEAD_COMMITTED
            .replace(
                "- shipped item\n",
                "- shipped item\n- operator added this\n",
            )
            .replace("Prior answer.\n", "Prior answer.\n\nalso check CI please\n");

        ensure_terminal_authority_disk_convergence(&file, &authority, LEAD_COMMITTED, &TestEffects)
            .expect("operator edits ahead of a lagging disk are not a turn failure");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            LEAD_COMMITTED,
            "session-check must never write the operator's edit to disk itself",
        );
    }

    /// The boundary: a disk that carries content the authority lacks (written
    /// behind the editor) has no operator provenance and stays INTERRUPTED,
    /// even with a committed plane to compare against.
    #[test]
    fn session_check_still_interrupts_a_disk_that_conflicts_with_the_authority() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = commit_session_doc(dir.path(), LEAD_COMMITTED);
        let authority = LEAD_COMMITTED.replace(
            "- shipped item\n",
            "- shipped item\n- operator added this\n",
        );
        let disk = LEAD_COMMITTED.replace("- shipped item\n", "- written behind the editor\n");
        std::fs::write(&file, &disk).unwrap();

        let err =
            ensure_terminal_authority_disk_convergence(&file, &authority, &disk, &TestEffects)
                .expect_err("a conflicting disk projection must stay fail-closed");
        assert!(
            format!("{err:#}").contains("refusing a false successful closeout"),
            "{err:#}"
        );
    }

    /// An open cycle owns its divergence: the binary's write may be the lead, so
    /// the operator-lead classification must not apply.
    #[test]
    fn session_check_operator_lead_does_not_apply_while_a_cycle_is_open() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = commit_session_doc(dir.path(), LEAD_COMMITTED);
        let authority = LEAD_COMMITTED.replace(
            "- shipped item\n",
            "- shipped item\n- operator added this\n",
        );
        assert!(operator_edit_lead_over_disk(
            &file,
            &authority,
            LEAD_COMMITTED,
            None,
            false,
        ));
        for phase in [
            CyclePhase::PreflightStarted,
            CyclePhase::ResponseCaptured,
            CyclePhase::WriteApplied,
        ] {
            assert!(
                !operator_edit_lead_over_disk(
                    &file,
                    &authority,
                    LEAD_COMMITTED,
                    Some(phase),
                    false
                ),
                "{phase:?} owns its divergence"
            );
        }
        assert!(
            !operator_edit_lead_over_disk(&file, &authority, LEAD_COMMITTED, None, true),
            "a retained document write owns its divergence"
        );
    }

    #[test]
    fn session_check_self_heals_transiently_stale_committed_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let committed = "# Session\n\ncomplete response\n<!-- agent:boundary:new -->\n";
        let stale = "# Session\n\ncomplete response\n<!-- no-pending-capture -->\n<!-- agent:boundary:old -->\n";
        std::fs::write(&file, committed).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        let commit = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "clean closeout", "--no-verify"])
            .output()
            .unwrap();
        assert!(commit.status.success());
        std::fs::write(&file, stale).unwrap();

        let healed =
            self_heal_transiently_stale_committed_projection(&file, stale, &TestEffects).unwrap();

        assert!(healed);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), committed);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .as_deref(),
            Some(committed),
        );
    }

    #[test]
    fn session_check_does_not_rewrite_current_head_marker_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let committed = "# Session\n\n### Re: recovery — gpt-5\n\nDone.\n";
        let authority = "# Session\n\n### Re: recovery — gpt-5 (HEAD)\n\nDone.\n";
        std::fs::write(&file, committed).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        let commit = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "clean closeout", "--no-verify"])
            .output()
            .unwrap();
        assert!(commit.status.success());
        std::fs::write(&file, authority).unwrap();

        let healed =
            self_heal_transiently_stale_committed_projection(&file, authority, &TestEffects)
                .unwrap();

        assert!(!healed);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), authority);
    }

    #[test]
    fn session_check_self_heals_duplicate_response_replay_boundary_before_integrity() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:stale -->\n",
            "### Re: retained — gpt-5\n\nRetained response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, duplicated).unwrap();

        let read_only = ReadOnlySessionCheckEffects {
            inner: &RepairOnlyEffects,
        };
        assert!(read_only.allows_response_replay_canonicalization());
        assert!(!read_only.allows_recovery());
        assert!(self_heal_response_replay_duplication(&file, &read_only).unwrap());

        let healed = std::fs::read_to_string(&file).unwrap();
        assert_eq!(healed.matches("agent:boundary:").count(), 1);
        assert!(healed.contains("agent:boundary:latest"));
        assert!(healed.contains("❯ operator prompt"));
        assert!(healed.contains("Retained response."));
        agent_doc_lint_io::validate_structure_on_content(&file, &healed).unwrap();
    }

    #[test]
    fn session_check_accepts_operator_edit_after_response_replay_repair() {
        struct AdvancedRepairEffects;

        impl SessionCheckEffects for AdvancedRepairEffects {
            fn closeout_recovery_hint(&self, file: &Path) -> String {
                TestEffects.closeout_recovery_hint(file)
            }
            fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
                anyhow::bail!("proof-bearing replay repair used generic semantic write")
            }
            fn atomic_repair_write_if_current(
                &self,
                file: &Path,
                content: &str,
                _expected_current: &str,
                _source: &str,
            ) -> Result<String> {
                let advanced = content.replacen("operator prompt", "operator promt", 1);
                std::fs::write(file, &advanced)?;
                Ok(advanced)
            }
            fn settle_committed_projection(
                &self,
                file: &Path,
                committed_content: &str,
                expected_current: &str,
            ) -> Result<()> {
                TestEffects.settle_committed_projection(
                    file,
                    committed_content,
                    expected_current,
                )
            }
            fn settle_retained_committed_projection(
                &self,
                file: &Path,
                committed_content: &str,
                expected_disk: &str,
            ) -> Result<bool> {
                TestEffects.settle_retained_committed_projection(
                    file,
                    committed_content,
                    expected_disk,
                )
            }
            fn repair_committed_historical_snapshot_drift(
                &self,
                file: &Path,
            ) -> Result<Option<&'static str>> {
                TestEffects.repair_committed_historical_snapshot_drift(file)
            }
            fn recover_missing_commit_boundary(
                &self,
                file: &Path,
                event: &str,
            ) -> Result<Option<&'static str>> {
                TestEffects.recover_missing_commit_boundary(file, event)
            }
            fn resume_captured_finalize(
                &self,
                file: &Path,
            ) -> Result<CapturedFinalizeResumeOutcome> {
                TestEffects.resume_captured_finalize(file)
            }
        }

        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:stale -->\n",
            "### Re: retained — gpt-5\n\nRetained response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, duplicated).unwrap();

        assert!(
            self_heal_response_replay_duplication(&file, &AdvancedRepairEffects).unwrap(),
            "the newer editor revision must settle the semantic replay repair",
        );
        let healed = std::fs::read_to_string(&file).unwrap();
        assert!(healed.contains("operator promt"));
        assert_eq!(healed.matches("agent:boundary:").count(), 1);
        agent_doc_lint_io::validate_structure_on_content(&file, &healed).unwrap();
    }

    /// `#dedupnoopretain`: a dedup cut identical to current content must not be
    /// published.
    ///
    /// Publishing it routes a write whose target already equals the authority, so
    /// there is no visible delta for an editor to acknowledge; the intent is
    /// retained forever and every later `session-check` re-reports a terminal
    /// failure. Observed 2026-08-08 on `tasks/agent-doc/agent-doc-bugs2.md`: the
    /// normalizer logged
    /// `session_check_response_replay_dedup_crdt_relay_exact_noop`, the caller
    /// routed the write anyway, an IDE restart registered mid-flight
    /// (`repair_projection_late_editor_retained`), and the disk hash was already
    /// exactly the retained target (`len=25480 hash=7d5b76e…`). HEAD was correct
    /// throughout; only the self-heal was wedged, and it wedged the document.
    ///
    /// HONEST LIMIT: this is a WIRING guard, not a behavioural one. The branch
    /// that returns an identical cut is the CRDT-relay path, which needs a live
    /// controller and editor registration — unreachable from a temp-file unit
    /// test. A behavioural fixture was written first and **mutation-checked**:
    /// it passed with the guard disabled, because two full-bodied copies make the
    /// normalizer return `None` and never reach the guard at all. Rather than
    /// ship a green test that proves nothing, the reachable half is asserted
    /// behaviourally (nothing repairable => no write, no drift) and the guard
    /// itself is pinned structurally.
    #[test]
    fn no_op_replay_dedup_is_guarded_before_any_write_is_published() {
        let source = include_str!("command.rs");
        let guard = source
            .split_once("fn self_heal_response_replay_duplication(")
            .map(|(_, tail)| tail)
            .and_then(|tail| tail.split_once("reconcile_deferred_write_to_canonical_cut_if_needed"))
            .map(|(head, _)| head)
            .expect("the replay-dedup self-heal must remain discoverable");
        assert!(
            guard.contains("if normalized == current {"),
            "the no-op guard must run BEFORE the reconcile/write pair, or an              identical cut is routed and retained forever (#dedupnoopretain)"
        );
        assert!(
            guard.contains("session_check_response_replay_dedup_noop_skipped"),
            "a skipped no-op must stay observable in ops.log"
        );

        // The reachable half, asserted for real: when there is nothing to
        // repair, no write path may be entered and the document must not drift.
        struct WriteIsAWedgeEffects;

        impl SessionCheckEffects for WriteIsAWedgeEffects {
            fn closeout_recovery_hint(&self, file: &Path) -> String {
                TestEffects.closeout_recovery_hint(file)
            }

            fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
                panic!("#dedupnoopretain: a no-op dedup must not publish a write");
            }

            fn atomic_repair_write_if_current(
                &self,
                _file: &Path,
                _content: &str,
                _expected_current: &str,
                _source: &str,
            ) -> Result<String> {
                panic!(
                    "#dedupnoopretain: a repair identical to current content must never be \
                     routed — that is the write that gets retained forever"
                );
            }

            fn settle_committed_projection(
                &self,
                file: &Path,
                committed_content: &str,
                expected_current: &str,
            ) -> Result<()> {
                TestEffects.settle_committed_projection(file, committed_content, expected_current)
            }

            fn settle_retained_committed_projection(
                &self,
                file: &Path,
                committed_content: &str,
                expected_disk: &str,
            ) -> Result<bool> {
                TestEffects.settle_retained_committed_projection(
                    file,
                    committed_content,
                    expected_disk,
                )
            }

            fn repair_committed_historical_snapshot_drift(
                &self,
                file: &Path,
            ) -> Result<Option<&'static str>> {
                TestEffects.repair_committed_historical_snapshot_drift(file)
            }

            fn recover_missing_commit_boundary(
                &self,
                file: &Path,
                event: &str,
            ) -> Result<Option<&'static str>> {
                TestEffects.recover_missing_commit_boundary(file, event)
            }

            fn resume_captured_finalize(
                &self,
                file: &Path,
            ) -> Result<CapturedFinalizeResumeOutcome> {
                TestEffects.resume_captured_finalize(file)
            }
        }

        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        // Two full-bodied copies: only proven-empty replay shells are removable,
        // so there is nothing this self-heal can fix.
        let unrepairable = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "\u{276f} operator prompt\n\n",
            "### Re: duplicated topic — gpt-5\n\nA complete body.\n\n",
            "### Re: duplicated topic — gpt-5\n\nA complete body.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, unrepairable).unwrap();

        let healed = self_heal_response_replay_duplication(&file, &WriteIsAWedgeEffects).unwrap();

        assert!(
            !healed,
            "nothing was repaired, so the self-heal must report false rather than \
             claiming a repair it did not make"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            unrepairable,
            "the document must be byte-identical: no write, no drift"
        );
    }

    #[test]
    fn session_check_self_heals_stranded_duplicate_response_heading_before_integrity() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let interrupted = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "### Re: retained topic — gpt-5\n\nRetained response.\n\n",
            "### Re: intervening topic — gpt-5\n\nIntervening response.\n\n",
            "### Re: retained topic — gpt-5\n\n",
            "### Re: latest topic — gpt-5\n\nLatest response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, interrupted).unwrap();

        assert!(self_heal_response_replay_duplication(&file, &RepairOnlyEffects).unwrap());

        let healed = std::fs::read_to_string(&file).unwrap();
        assert_eq!(healed.matches("### Re: retained topic — gpt-5").count(), 1);
        assert!(healed.contains("Retained response."));
        assert!(healed.contains("Intervening response."));
        assert!(healed.contains("Latest response."));
        agent_doc_lint_io::validate_structure_on_content(&file, &healed).unwrap();
    }

    #[test]
    fn session_check_recovers_replay_commit_boundary_after_process_restart() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "### Re: retained topic — gpt-5\n\nRetained response.\n\n",
            "### Re: intervening topic — gpt-5\n\nIntervening response.\n\n",
            "### Re: retained topic — gpt-5\n\n",
            "### Re: latest topic — gpt-5\n\nLatest response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let repaired =
            agent_doc_document_realtime_io::normalize_recoverable_response_replay_duplication(
                duplicated,
            )
            .unwrap();
        std::fs::write(&file, duplicated).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        let initial_commit = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "replayed closeout", "--no-verify"])
            .output()
            .unwrap();
        assert!(initial_commit.status.success());
        std::fs::write(&file, &repaired).unwrap();

        assert!(
            response_replay_canonicalization_pending_commit(&file).unwrap(),
            "a restarted session-check must recover the write-without-commit boundary from HEAD"
        );

        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        let repaired_commit = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "repair replay", "--no-verify"])
            .output()
            .unwrap();
        assert!(repaired_commit.status.success());
        assert!(!response_replay_canonicalization_pending_commit(&file).unwrap());
    }

    #[test]
    fn replay_commit_boundary_survives_a_concurrent_operator_cut() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "### Re: retained topic — gpt-5\n\nRetained response.\n\n",
            "### Re: intervening topic — gpt-5\n\nIntervening response.\n\n",
            "### Re: retained topic — gpt-5\n\n",
            "### Re: latest topic — gpt-5\n\nLatest response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
            "\n<!-- agent:queue preset=\"#build\" priority go -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        let repaired =
            agent_doc_document_realtime_io::normalize_recoverable_response_replay_duplication(
                duplicated,
            )
            .unwrap();
        std::fs::write(&file, duplicated).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        let initial_commit = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "replayed closeout", "--no-verify"])
            .output()
            .unwrap();
        assert!(initial_commit.status.success());
        let operator_cut = repaired.replace("preset=\"#build\"", "preset=\"#verify\"");
        std::fs::write(&file, operator_cut).unwrap();

        assert!(
            response_replay_canonicalization_pending_commit(&file).unwrap(),
            "canonical HEAD repair remains pending even when current also has operator work"
        );
    }

    #[test]
    fn captured_resume_normalizes_replay_boundary_before_revalidation() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let replayed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:stale -->\n",
            "### Re: retained — gpt-5\n\nRetained response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, replayed).unwrap();

        let recovered = validate_integrity_after_captured_resume(
            &file,
            "test_post_resume_integrity",
            &TestEffects,
        )
        .expect("post-resume validation should first normalize replay artifacts");

        assert_eq!(recovered.matches("agent:boundary:").count(), 1);
        assert!(recovered.contains("agent:boundary:latest"));
        assert!(recovered.contains("❯ operator prompt"));
        assert!(recovered.contains("Retained response."));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), recovered);
    }

    #[test]
    fn retained_pending_write_guidance_preserves_same_capture() {
        let file = Path::new("session.md");
        let message = retained_pending_write_message(
            file,
            RetainedPendingWriteMessage {
                intent_id: "intent-1",
                reason: "editor_projection_pending",
                source: "write_stream",
                target_hash: "new",
                resume_reason: None,
                captured_closeout: true,
            },
            agent_doc_turn::write_ownership::RetainedWriteOwnership::new(true, true)
                .with_retained_projection(true),
        );
        assert!(message.contains("same capture will resume"));
        assert!(message.contains("Do not issue another closeout payload"));
        assert!(message.contains("do not force disk"));
        assert!(message.contains("retry only `agent-doc session-check session.md`"));
        assert!(!message.contains("agent-doc finalize"));
        assert!(!message.contains("write --commit"));
    }

    #[test]
    fn retained_pending_write_guidance_distinguishes_non_capture_projection() {
        let file = Path::new("session.md");
        let message = retained_pending_write_message(
            file,
            RetainedPendingWriteMessage {
                intent_id: "intent-2",
                reason: "merge_unsaved_editor_cut_with_deferred_target",
                source: "editor_reconnect",
                target_hash: "normalized",
                resume_reason: None,
                captured_closeout: false,
            },
            agent_doc_turn::write_ownership::RetainedWriteOwnership::UNOWNED
                .with_retained_projection(true),
        );
        assert!(message.contains("binary-owned document projection"));
        assert!(message.contains("same intent will resume"));
        assert!(message.contains("Do not issue a closeout payload"));
        assert!(!message.contains("binary-owned response delivery"));
        assert!(!message.contains("same capture will resume"));
    }

    #[test]
    fn retained_pending_rejection_reports_operator_gate_instead_of_automatic_retry() {
        let file = Path::new("session.md");
        let message = retained_pending_write_message(
            file,
            RetainedPendingWriteMessage {
                intent_id: "intent-refused",
                reason: "editor_projection_pending",
                source: "serialized_atomic_write",
                target_hash: "target",
                resume_reason: Some(
                    "retained target has not reached exact canonical/disk convergence",
                ),
                captured_closeout: true,
            },
            agent_doc_turn::write_ownership::RetainedWriteOwnership::new(true, true)
                .with_retained_projection(true)
                .with_delivery_rejected(true),
        );

        assert!(
            message.contains(RETAINED_PENDING_NEEDS_OPERATOR_TOKEN),
            "{message}"
        );
        assert!(
            message.contains("REJECTED the delivery receipt"),
            "{message}"
        );
        assert!(
            message.contains("restart or reload the editor"),
            "{message}"
        );
        assert!(
            message.contains("exact intent remains durable"),
            "{message}"
        );
        assert!(
            !message.contains("retry only `agent-doc session-check"),
            "{message}"
        );
        assert!(
            !message.contains("same capture will resume automatically"),
            "{message}"
        );
    }

    #[test]
    fn captured_stranded_guidance_never_prescribes_an_impossible_commit() {
        let message = retained_pending_guidance(true, true, true);
        assert!(message.contains("do NOT contain the captured response"));
        assert!(message.contains("`agent-doc commit` cannot succeed"));
        assert!(message.contains("existing replay"));
        assert!(!message.contains("Run `agent-doc commit <FILE>`"));
    }

    #[test]
    fn non_capture_stranded_guidance_can_adopt_converged_projection() {
        let message = retained_pending_guidance(true, true, false);
        assert!(message.contains("non-capture intent"));
        assert!(message.contains("Run `agent-doc commit <FILE>`"));
    }
}

#[cfg(test)]
mod settle_window_tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    const WRITE_APPLIED: &str = "[session-check] INTERRUPTED: cycle `c` is still `write_applied` (e) — response write landed but no terminal commit followed.";

    #[test]
    fn a_self_converging_interruption_is_resampled_until_it_commits() {
        let calls = Cell::new(0);
        let out = settle_until_converged(Duration::from_secs(5), Duration::from_millis(1), || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                anyhow::bail!(WRITE_APPLIED)
            }
            Ok(())
        });
        assert!(out.is_ok());
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn an_agent_actionable_interruption_is_reported_at_once() {
        let calls = Cell::new(0);
        let out = settle_until_converged(Duration::from_secs(5), Duration::from_millis(1), || {
            calls.set(calls.get() + 1);
            anyhow::bail!(
                "[session-check] INTERRUPTED: cycle `c` is `committed` (e), but the document has uncommitted exchange changes beyond the committed snapshot: x. Run `agent-doc finalize f`"
            )
        });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_stall_is_still_reported_when_the_window_closes() {
        let calls = Cell::new(0);
        let out =
            settle_until_converged(Duration::from_millis(30), Duration::from_millis(5), || {
                calls.set(calls.get() + 1);
                anyhow::bail!(WRITE_APPLIED)
            });
        assert!(format!("{:#}", out.unwrap_err()).contains("write_applied"));
        assert!(calls.get() > 1);
        let once = Cell::new(0);
        let _ = settle_until_converged(Duration::ZERO, Duration::from_millis(5), || {
            once.set(once.get() + 1);
            anyhow::bail!(WRITE_APPLIED)
        });
        assert_eq!(once.get(), 1, "a zero window samples exactly once");
    }

    /// `#scsettleexit`: the real check returns its INTERRUPTED outcome as a
    /// typed error, and the settle loop must classify that error — not only a
    /// hand-built `bail!` string — or the window is dead code again.
    #[test]
    fn the_typed_interrupted_outcome_is_resampled_until_it_commits() {
        let calls = Cell::new(0);
        let out = settle_until_converged(Duration::from_secs(5), Duration::from_millis(1), || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                return Err(SessionCheckInterrupted(WRITE_APPLIED.to_string()).into());
            }
            Ok(())
        });
        assert!(out.is_ok());
        assert_eq!(calls.get(), 3);
    }

    /// `#scsettleexit`: an INTERRUPTED outcome must never exit the process from
    /// inside the check. The only `process::exit(1)` allowed in production code
    /// is the entry-point one in `exit_on_interrupted`, which runs after any
    /// settling. The window must also cover the observed 17s deferral.
    #[test]
    fn interrupted_exits_only_at_the_entry_point() {
        let source = include_str!("command.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("test modules")];
        let exits: Vec<usize> = production
            .match_indices("std::process::exit(1)")
            .map(|(at, _)| at)
            .collect();
        assert_eq!(exits.len(), 1, "exactly one exit(1): {exits:?}");
        let entry = production
            .find("fn exit_on_interrupted(")
            .expect("exit_on_interrupted");
        let entry_end = entry + production[entry..].find("\n}\n").expect("fn end");
        assert!(
            (entry..entry_end).contains(&exits[0]),
            "the exit(1) must live in exit_on_interrupted"
        );
        const { assert!(SESSION_CHECK_SETTLE_WINDOW_SECS >= 20) };
    }

    #[test]
    fn every_retained_resume_message_is_self_converging() {
        for message in [
            WRITE_APPLIED,
            "[session-check] INTERRUPTED: binary-owned response delivery `i` is retained for `f` (reason=r, source=s, target_hash=h); the same capture will resume automatically after editor/controller delivery converges.",
            "[session-check] INTERRUPTED: retained document-write delivery remains unsettled for f; automatic controller reconciliation remains scheduled.",
            "[session-check] INTERRUPTED: canonical editor authority and disk projection diverge for f (authority_hash=a, disk_hash=d, component_divergence=c); refusing a false successful closeout. Automatic editor recovery status: s. Replica re-registration and projection settlement are scheduled automatically;",
            "[session-check] INTERRUPTED: canonical editor authority and disk projection diverge for f (authority_hash=a, disk_hash=d, component_divergence=c); the one-shot editor-native save receipt is pending; retained delivery remains unsettled while controller reconciliation remains scheduled. The controller owns the next closeout attempt; it retries the native editor save as the delivery receipt allows.",
        ] {
            assert!(is_self_converging_interruption(message), "{message}");
        }
        assert!(!is_self_converging_interruption(
            "will resume automatically"
        ));
    }

    #[test]
    fn read_only_editor_authority_recovery_waits_until_rejection_is_actionable() {
        let calls = Cell::new(0);
        let pending = format!(
            "[session-check] INTERRUPTED: canonical editor authority and disk projection diverge for f; {}. The controller owns the next closeout attempt",
            read_only_terminal_projection_detail(
                ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave,
            ),
        );
        let rejected = "[session-check] INTERRUPTED: binary-owned response delivery is retained; the registered editor endpoint rejected the delivery receipt; restart or reload the editor";

        let outcome = settle_until_converged(
            Duration::from_secs(5),
            Duration::from_millis(1),
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    anyhow::bail!(pending.clone());
                }
                anyhow::bail!(rejected);
            },
        );

        assert_eq!(calls.get(), 2, "the pending native-save state must be resampled");
        assert!(format!("{:#}", outcome.unwrap_err()).contains("rejected"));
    }

    #[test]
    fn every_needle_names_a_real_interruption() {
        // Pin the needles to the emitting sources: a reworded message must fail
        // here, not silently turn the settle window off.
        assert_eq!(
            agent_doc_workflow::session_check::open_cycle_detail(
                agent_doc_turn::CyclePhase::WriteApplied
            ),
            SELF_CONVERGING_INTERRUPTIONS[0]
        );
        let source = include_str!("command.rs");
        // Production code only (every test module sits below the first
        // `#[cfg(test)]`), minus the needle table itself.
        let production = &source[..source.find("\n#[cfg(test)]").expect("test modules")];
        let table = production
            .find("const SELF_CONVERGING_INTERRUPTIONS")
            .expect("needle table");
        let after = table + production[table..].find("];").expect("needle table ends");
        let emitters = format!("{}{}", &production[..table], &production[after..]);
        for needle in &SELF_CONVERGING_INTERRUPTIONS[1..] {
            assert!(
                emitters
                    .split("\"[session-check] INTERRUPTED:")
                    .skip(1)
                    .any(|message| message.split("\",").next().unwrap_or("").contains(needle)),
                "no INTERRUPTED message emits {needle:?}"
            );
        }
    }
}

#[cfg(test)]
mod supervisor_scope_claim_tests {
    use super::*;

    /// Every head is claimed by a subagent (or held by the queue subagents
    /// attribute): the supervisor-scope fallback must report no head, so
    /// session-check does not tell the session to drain in-flight work.
    #[test]
    fn claimed_and_held_heads_are_not_supervisor_drainable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue subagents=1 go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] first\n",
            "- [ ] [#b] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&file, content).unwrap();
        assert!(
            agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            )
            .is_some(),
            "precondition: without claims a head is supervisor-drainable"
        );

        // `a` is claimed (the one slot), so `b` is held by the cap.
        agent_doc_queue_io::queue_claim::claim(&file, "#a", "subagent:a", 600).unwrap();
        assert_eq!(supervisor_scope_unclaimed_head(&file, content), None);
    }
}
