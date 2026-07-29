//! # Module: write
//!
//! All write paths for agent responses: inline append, template patch, stream
//! flush, editor delivery, and recovery helpers. Each response is one immutable
//! intent in the state ledger. The pipeline advances monotonically through
//! canonical apply, editor acceptance/visibility, disk projection, and commit;
//! retries and reconnects resume that same intent. Lazily is the sole attached
//! editor authority. Snapshot/CRDT files are detached recovery projections only.
//!
//! ## Write dedup (v0.28.2)
//!
//! All four write paths (`run`, `run_template`, `run_stream` disk, `run_stream`
//! IPC) skip the actual write when the merged/patched content is identical to
//! the current file on disk. Dedup events are logged to stderr and appended
//! (with backtrace) to `/tmp/agent-doc-write-dedup.log` for diagnosis.
//!
//! ## Pane ownership verification (v0.28.2)
//!
//! `verify_pane_ownership()` is called at the top of `run`, `run_template`, and
//! `run_stream`. It checks that the current tmux pane matches the session
//! registry entry for the document's `session` frontmatter field. If a
//! *different* pane definitively owns the session, the write is rejected with an
//! error suggesting `agent-doc claim`. The check is lenient: it passes silently
//! when not in tmux, when there is no session ID, or when the pane is
//! indeterminate.
//!
//! ## Spec
//!
//! - `run`: inline (User/Assistant) mode. Reads response from stdin, strips any
//!   leading `## Assistant` / trailing `## User` headings the agent may have
//!   echoed, then appends `## Assistant\n\n<response>\n\n## User\n\n` to the
//!   document. Saves a undo checkpoint for undo. If the file changed
//!   since `baseline`, performs a 3-way git merge before writing.
//!
//! - `run_template`: template-component mode. Parses `patch:NAME` fence blocks
//!   from stdin, sanitizes any `<!-- agent:NAME -->` markers in patch content
//!   (prevents parser corruption), applies patches to the baseline via
//!   `agent_doc_template_io::apply_patches`, then performs the same lock/merge/atomic-write
//!   cycle as `run`.
//!
//! - `run_stream`: template stream-flush mode. Like `run_template` but resolves
//!   concurrent changes through the document-model component/semantic merge
//!   policy. Each flush checkpoints the intent in `state.db`, applies a canonical
//!   CRDT operation, and delivers a typed intent to the registered editor
//!   endpoint. A timeout retains the same intent and proof frontier; it never
//!   elects a file transport or recaptures the response.
//!
//! - `run_ipc`: explicit editor-delivery mode. Sends one typed message to the
//!   selected PID-scoped endpoint and records the causal receipt in Lazily state.
//! - `run_command(options, commit_mode)`: private shared command entrypoint for
//!   `write` and `finalize`. `finalize` is always strict. `write --commit` stays
//!   best-effort for non-session documents and `--pending-only`, but upgrades to
//!   the same strict commit-boundary contract as `finalize` when the target file
//!   is a real session document (`agent_doc_session` / legacy `session`) and the
//!   command is writing a response. A bare session-document `write` may still
//!   preserve the response body/capture for recovery, but it must fail closed
//!   instead of returning success while the cycle remains open at
//!   `response_captured` / `write_applied`.
//!
//! - `try_ipc`: low-level socket helper used by `run_stream`. Delivers component
//!   operations plus an optional boundary reposition intent. It returns only
//!   after a causal editor receipt, or retains the intent for later endpoint
//!   recovery when proof is unavailable.
//!
//! - `try_ipc_full_content`: narrowly scoped canonical recovery delivery. It is
//!   allowed only with an exact expected editor baseline and uses the same
//!   targeted endpoint; it has no file transport or implicit disk fallback.
//!
//! - `try_ipc_reposition_boundary`: fire-and-forget IPC signal with the exact
//!   committed exchange `boundary_id`. Normalizes the editor buffer back to the
//!   committed boundary marker without touching the working tree (preserves
//!   cursor/undo in the IDE). Non-fatal on timeout.
//!
//! - `apply_append_from_string`: recovery variant of `run` — takes response
//!   text directly instead of reading stdin. Used by `repair` to replay
//!   orphaned inline responses.
//!
//! - `apply_template_from_string`: recovery variant of `run_template`.
//!
//! - `apply_stream_from_string`: recovery variant of `run_stream`.
//!
//! - `agent_doc_turn::heuristics::future_work_signal`: detects deferred-work
//!   phrases in responses while `run_stream` owns only the warning side effect
//!   when the caller did not provide `--pending-add`.
//!
//! - `enforce_imperative_response_contract(file, baseline, current, response)`:
//!   when the current document diff contains imperative user directives
//!   (`do #id`, `run tests`, `build + install`, `commit + push`, or approval
//!   words like `go`), rejects status-only/meta responses using
//!   `agent_doc_turn::response_text::response_satisfies_imperative_contract`.
//!   The write path keeps diff inspection, ops-log emission, and rejection
//!   formatting as the binary-side backstop for the executable-directive
//!   contract.
//!
//! - `agent_doc_template::sanitize`: escapes `<!-- agent:NAME -->` and
//!   `<!-- /agent:NAME -->` markers appearing in patch content before the
//!   component parser can treat them as real delimiters.
//!
//! - `agent_doc_turn::response_text`: strips leading `## Assistant` and
//!   trailing `## User` headings from append responses before this module adds
//!   the canonical transcript headings.
//!
//! ## Agentic Contracts
//!
//! - Snapshot invariant: the snapshot saved after every write normally contains
//!   `final_content` (the actual post-merge disk state), not `content_ours`.
//!   This eliminates ghost diffs caused by stale baselines (e.g. streaming
//!   checkpoints with an outdated baseline). Narrow exception: when the caller
//!   supplied an explicit baseline and concurrent prompt-bearing or
//!   non-`agent:exchange` user edits changed the merged disk state, the snapshot
//!   stays at `content_ours` so those late user edits remain visible to the next
//!   diff cycle instead of being folded into the just-finished turn.
//! - Once a response survives strict pre-write closeout gates, it is saved to
//!   the pending store before any document mutation and cleared only after a
//!   successful write, so an interrupted write is recoverable.
//! - Pre-response snapshot is captured from the live document state while the
//!   advisory doc lock is held, so `undo` restores the exact on-disk content
//!   that existed immediately before the local write path applied the response.
//! - All writes are atomic (temp file + rename). Partial writes never corrupt
//!   the document.
//! - Advisory file lock (`flock`) serialises concurrent writes to the same
//!   document; the lock is dropped immediately after `atomic_write`.
//! - `try_ipc` targets only a PID-scoped registered editor endpoint. It never
//!   creates or polls a file patch inbox, and an attached document never falls
//!   back to disk around Lazily current authority.
//! - IPC writes include `reposition_boundary: true` so the plugin moves the
//!   boundary marker to end-of-exchange in the same Document API transaction as
//!   the patch, avoiding a second round-trip.
//! - CRDT snapshots are saved from the merged state (not from `content_ours`)
//!   so subsequent merges use the correct shared ancestor, preventing
//!   character-level duplication across cycles.
//! - `agent_doc_template::sanitize` is applied to every patch block before any
//!   write path applies it, preventing agent-generated examples of component
//!   syntax from corrupting future parses.
//!
//! ## Evals
//!
//! - `write_appends_response`: inline write appends `## Assistant\n\n<text>` +
//!   `\n## User\n\n` to a document → both headings and content present in file.
//! - `write_updates_snapshot`: after a write the snapshot path resolves to
//!   `.agent-doc/snapshots/` and a roundtrip read/write is lossless.
//! - `write_preserves_user_edits_via_merge`: 3-way merge when user appends to
//!   `## User` block concurrently → merged result contains both response and
//!   user addition.
//! - `write_no_merge_when_unchanged`: when file equals baseline at lock time,
//!   `content_ours` is used directly (no merge invoked).
//! - `atomic_write_correct_content`: temp-rename write produces the exact bytes
//!   supplied.
//! - `concurrent_writes_no_corruption`: 20 threads racing on atomic_write →
//!   final file is one complete writer's content (no corruption or partial
//!   writes).
//! - `snapshot_matches_disk_state`: snapshot saved as `final_content`;
//!   snapshot always matches the actual file on disk after a write.
//! - `try_ipc_refuses_incomplete_registration`: an attached document without a
//!   matching PID-scoped endpoint fails closed without a disk write.
//! - `try_ipc_succeeds_with_visible_receipt`: the matching endpoint accepts the
//!   intent and Lazily proves the resulting visible content.
//! - `try_ipc_full_content_returns_false`: full-content IPC is disabled and
//!   returns `false` without emitting an unfenced replacement payload.
//! - `sanitize_escapes_open_agent_tag`: `<!-- agent:exchange -->` inside patch
//!   content is escaped to `&lt;!-- agent:exchange --&gt;`.
//! - *(aspirational)* `run_stream_crdt_merge`: concurrent user keystroke during
//!   stream flush → CRDT merge produces text containing both agent response and
//!   user addition without character interleaving.
//! - *(aspirational)* `ipc_fallback_on_timeout`: `run_stream` with IPC timeout
//!   exits with code 75 and leaves a patch file for deferred plugin pickup.
//! - `detects_worth_revisiting`: response with "Worth revisiting" and no
//!   pending-add → returns `Some("worth revisiting")`.
//! - `future_work_signal`: response with "future work" and no pending-add state
//!   → warns from the write path.
//! - `detects_follow_up_needed`: response with "Follow-up needed" → returns signal.
//! - `suppressed_when_pending_add_present`: response with signal but
//!   `has_pending_add=true` → returns `None`.
//! - `no_false_positive_on_normal_text`: response without any signal phrases →
//!   returns `None`.
//! - `case_insensitive_detection`: "WORTH REVISITING" (uppercase) → detected.
//! - `imperative_contract_rejects_status_only_response`: directive diff +
//!   "In progress" response → error
//! - `imperative_contract_allows_concrete_blocker`: directive diff +
//!   blocked/error response → accepted
//! - `normalize_user_prompts_new_line_gets_prefix`: user adds "Hello" to exchange
//!   → normalized content has "❯ Hello".
//! - `normalize_user_prompts_agent_response_not_prefixed`: agent response lines in content_ours
//!   must NOT get `❯ ` prefix — only user-added lines (snapshot→baseline diff) are prefixed.
//! - `normalize_user_prompts_blank_line_skipped`: blank line added → no prefix.
//! - `normalize_user_prompts_heading_skipped`: line starting with `#` → no prefix.
//! - `normalize_user_prompts_already_prefixed_skipped`: line already starts with `❯` → unchanged.
//! - `normalize_user_prompts_existing_content_unchanged`: lines from snapshot → unchanged (no double-prefix).
//! - `normalize_user_prompts_restores_prefix_lost_in_file`: snapshot has `❯ do`, baseline (file) has `do` → restored to `❯ do`.
//! - `normalize_user_prompts_heading_replacement_does_not_swallow_next_prompt`: a synthetic heading replacement
//!   (for example ` (HEAD)` suffix churn) must not suppress `❯ ` prefixing for the next user line.
//! - `normalize_user_prompts_no_exchange_passthrough`: document without exchange → returned unchanged.
//! - `shrink_guard_blocks_truncation`: exchange shrinks from 500 to 5 bytes →
//!   `check_exchange_shrink_guard` returns error.
//! - `shrink_guard_allows_normal_write`: exchange shrinks by 50% → guard passes.
//! - `shrink_guard_skips_small_exchange`: exchange is 50 bytes → guard passes
//!   regardless of shrink ratio (below `SHRINK_GUARD_MIN_BYTES`).
//! - `splice_pending_replaces_content_when_both_have_pending`: on IPC timeout,
//!   pending-done mutations from disk are preserved in the written content.
//! - `splice_pending_noop_when_source_has_no_pending`: no source pending → target unchanged.
//! - `splice_pending_warns_when_target_missing_pending`: target has no pending component → target unchanged.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::io::Read;
use std::path::Path;

#[cfg(test)]
use agent_doc_document::write_normalization::{
    SplicePendingComponentWarning, count_code_fence_openings, splice_pending_component,
    strip_boundary_for_dedup,
};
use agent_doc_document_realtime::write_policy::{
    reconcile_visible_write, response_already_in_current,
};
pub(crate) use agent_doc_document_realtime_io::{
    VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS, guard_visible_write_expected_current_or_target,
    guard_visible_write_reconcile_with_target,
};
#[cfg(test)]
use agent_doc_element::element;
use agent_doc_element_backlog_io::backlog_cmd;
use agent_doc_element_exchange::{
    PromptGrowthProvenanceInput, exchange_has_live_user_edit, exchange_prompt_prefix_count,
    exchange_prompt_text_duplicated, response_precedes_prompt_in_exchange_with_prompt_growth,
    strip_prompt_prefix_from_response_body_first_lines,
};
use agent_doc_queue::queue_consume::{
    queue_consumption_allowed_for_response, queue_targeted_completion_id_for_current_head,
};
use agent_doc_queue_io::queue_consume::{self, QueueConsumeWriteEffects, QueueConsumptionOutcome};
use agent_doc_template_io::normalize_user_prompts_in_exchange_safe;
use agent_doc_workflow::session_cycle::{
    FinalizeRerunCommand, compact_command_hint, finalize_rerun_command_base,
    group_pending_add_targets, parse_id_order, parse_tracked_work_edits,
    pending_kept_open_ids_from_mutations,
};

use agent_doc_element_exchange_io::DuplicatePromptRepairOptions;
use agent_doc_frontmatter::frontmatter;
use agent_doc_template as template;
#[cfg(test)]
use agent_doc_template_io::normalize_template_structure_or_fail;
use agent_doc_template_io::{
    log_duplicate_prompt_residue_guard, normalize_template_structure_or_fail_preserving,
};

use agent_doc_template::stale_baseline::{
    exchange_append_patch_can_rebase_to_head, is_stale_baseline, patch_touches_exchange,
};
use agent_doc_turn::response_replay::response_materialized_in_content;
use agent_doc_write_command_io::{CommandOptions, CommitMode, TemplateApplyOptions};

thread_local! {
    static RESPONSE_STDIN_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn resolve_current_document(
    file: &Path,
    source: &'static str,
) -> Result<agent_doc_document_realtime::CurrentDocument> {
    agent_doc_document_realtime_io::try_resolve_current_document_with_source(file, source)
        .with_context(|| {
            format!(
                "{source}: failed to resolve current document {}",
                file.display()
            )
        })
}

fn resolve_force_disk_document(
    file: &Path,
    source: &'static str,
) -> Result<agent_doc_document_realtime::CurrentDocument> {
    agent_doc_document_realtime_io::resolve_disk_current_document(file, source)
}

struct ForceDiskQueueConsumeWritebackEffects;

static FORCE_DISK_QUEUE_CONSUME_WRITEBACK_EFFECTS: ForceDiskQueueConsumeWritebackEffects =
    ForceDiskQueueConsumeWritebackEffects;

impl QueueConsumeWriteEffects for ForceDiskQueueConsumeWritebackEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(file, content)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }
}

fn queue_consume_writeback_effects(force_disk: bool) -> &'static dyn QueueConsumeWriteEffects {
    if force_disk {
        &FORCE_DISK_QUEUE_CONSUME_WRITEBACK_EFFECTS
    } else {
        &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS
    }
}

fn queue_skip_diagnostic_for_current_document(file: &Path, force_disk: bool) -> Result<String> {
    let content = if force_disk {
        resolve_force_disk_document(file, "queue_skip_diagnostic")?.into_content()
    } else {
        resolve_current_document(file, "queue_skip_diagnostic")?.into_content()
    };
    agent_doc_queue::queue_heads::queue_skip_diagnostic_for_content(&content)
}

pub struct RuntimeRepairReplayWriteEffects;

pub static REPAIR_REPLAY_WRITE_EFFECTS: RuntimeRepairReplayWriteEffects =
    RuntimeRepairReplayWriteEffects;

impl agent_doc_repair_io::RepairStrictReplayWriteEffects for RuntimeRepairReplayWriteEffects {
    fn run_strict_write_replay(
        &self,
        file: &Path,
        response: &str,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> Result<()> {
        let commit_mode = if agent_doc_git_io::status::is_in_git_repo(file) {
            CommitMode::Required
        } else {
            CommitMode::None
        };
        run_command_with_response(
            CommandOptions::repair_replay(
                file,
                is_template,
                is_stream,
                force_disk,
                queue_completion_ids,
            ),
            commit_mode,
            response.to_string(),
        )
    }
}

impl agent_doc_repair_io::RepairFallbackWriteEffects for RuntimeRepairReplayWriteEffects {
    fn apply_template_from_string(
        &self,
        file: &Path,
        response: &str,
        force_disk: bool,
    ) -> Result<()> {
        run_entry::apply_template_from_string_with_options(
            file,
            response,
            TemplateApplyOptions { force_disk },
        )
    }

    fn apply_append_from_string(&self, file: &Path, response: &str) -> Result<()> {
        run_entry::apply_append_from_string(file, response)
    }
}

impl agent_doc_repair_io::RepairRecoveredQueueHeadEffects for RuntimeRepairReplayWriteEffects {
    fn strike_recovered_free_text_queue_head(
        &self,
        file: &Path,
        expected_head: &str,
    ) -> Result<()> {
        match agent_doc_queue_io::queue_consume::consume_queue_prompt_if_head_matches_with_outcome(
            file,
            expected_head,
            &[],
            true,
            &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
        ) {
            Ok(Some(outcome)) => {
                eprintln!(
                    "[repair] struck consumed free-text queue head (remaining: {})",
                    outcome.remaining
                );
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => Err(err),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WriteFlags {
    pub(crate) allow_replace_pending: bool,
    pub(crate) has_pending_add: bool,
    pub(crate) has_pending_done: bool,
    pub(crate) has_pending_mutation: bool,
    pub(crate) pending_done_ids: Vec<String>,
    pub(crate) queue_completion_ids: Vec<String>,
    pub(crate) pending_kept_open_ids: Vec<String>,
    pub(crate) strict_closeout: bool,
    pub(crate) force_disk: bool,
    pub(crate) no_pending_capture: bool,
    pub(crate) mutation_plan_json: Option<String>,
    pub(crate) empty_response_recovery: Option<EmptyResponseRecovery>,
    pub(crate) rerun_command_base: Option<String>,
}

fn pending_write_flags(flags: &WriteFlags) -> agent_doc_session_check_io::PendingWriteFlags {
    agent_doc_session_check_io::PendingWriteFlags {
        has_pending_add: flags.has_pending_add,
        has_pending_done: flags.has_pending_done,
        pending_done_ids: flags.pending_done_ids.clone(),
        pending_kept_open_ids: flags.pending_kept_open_ids.clone(),
        strict_closeout: flags.strict_closeout,
        force_disk: flags.force_disk,
        rerun_command_base: flags.rerun_command_base.clone(),
    }
}

pub(crate) const EMPTY_RESPONSE_ERROR: &str = "empty response — nothing to write";

pub(crate) type EmptyResponseRecovery = fn(&Path, bool, bool, bool) -> Result<bool>;

thread_local! {
    /// Capability tokens for strict-write recursion on this thread. A repair
    /// replay may re-enter the write runtime while its outer closeout guard is
    /// still live; that nested call is covered by the existing actor claim.
    static ACTIVE_FOREGROUND_CLOSEOUTS: std::cell::RefCell<Vec<std::path::PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct CloseoutOwnerGuard {
    file: std::path::PathBuf,
    owner_key: std::path::PathBuf,
    cycle_id: String,
    owner_id: String,
    stop: Option<std::sync::mpsc::Sender<()>>,
    heartbeat: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CloseoutOwnerGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take()
            && stop.send(()).is_err()
        {
            agent_doc_ops_log_io::log_op(
                &self.file,
                &format!(
                    "closeout_owner_heartbeat_stop_disconnected file={} cycle_id={} owner_id={}",
                    self.file.display(),
                    self.cycle_id,
                    self.owner_id
                ),
            );
        }
        if let Some(heartbeat) = self.heartbeat.take()
            && heartbeat.join().is_err()
        {
            agent_doc_ops_log_io::log_op(
                &self.file,
                &format!(
                    "closeout_owner_heartbeat_panicked file={} cycle_id={} owner_id={}",
                    self.file.display(),
                    self.cycle_id,
                    self.owner_id
                ),
            );
        }
        if let Err(err) =
            agent_doc_controller_io::project_controller::release_closeout_owner_for_file(
                &self.file,
                &self.cycle_id,
                &self.owner_id,
                "foreground_closeout_finished",
            )
        {
            agent_doc_ops_log_io::log_op(
                &self.file,
                &format!(
                    "closeout_owner_release_failed file={} cycle_id={} owner_id={} err={err}",
                    self.file.display(),
                    self.cycle_id,
                    self.owner_id,
                ),
            );
        }
        ACTIVE_FOREGROUND_CLOSEOUTS.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(index) = active.iter().rposition(|path| path == &self.owner_key) {
                active.remove(index);
            } else {
                agent_doc_ops_log_io::log_op(
                    &self.file,
                    &format!(
                        "closeout_owner_local_capability_missing file={} cycle_id={} owner_id={}",
                        self.file.display(),
                        self.cycle_id,
                        self.owner_id,
                    ),
                );
            }
        });
    }
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteCloseoutOwnerRole {
    ForegroundFinalize,
    CapturedFinalizeResume,
}

impl WriteCloseoutOwnerRole {
    fn from_origin(origin: Option<&str>) -> Self {
        if origin == Some("captured_finalize_resume") {
            Self::CapturedFinalizeResume
        } else {
            Self::ForegroundFinalize
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ForegroundFinalize => "foreground_finalize",
            Self::CapturedFinalizeResume => "captured_finalize_resume",
        }
    }

    fn owner_id_prefix(self) -> &'static str {
        match self {
            Self::ForegroundFinalize => "foreground-finalize",
            Self::CapturedFinalizeResume => "captured-finalize-resume",
        }
    }
}

fn claim_foreground_closeout_owner(
    file: &Path,
    role: WriteCloseoutOwnerRole,
) -> Result<Option<CloseoutOwnerGuard>> {
    use agent_doc_controller_io::project_controller as controller;

    // Ignored/untracked and standalone documents have no project actor. Their
    // existing write path decides whether to skip or use the explicit
    // actorless compatibility boundary.
    if agent_doc_project_root_io::project_root_containing(file).is_none() {
        return Ok(None);
    }
    let owner_key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if ACTIVE_FOREGROUND_CLOSEOUTS
        .with(|active| active.borrow().iter().any(|path| path == &owner_key))
    {
        return Ok(None);
    }

    let owner_id = controller::new_closeout_owner_id(role.owner_id_prefix());
    let owner_pid = std::process::id();
    let role_name = role.as_str();
    let cycle_id = match controller::claim_closeout_owner_for_file(
        file,
        controller::CloseoutOwnerClaimRequest {
            expected_cycle_id: None,
            owner_id: owner_id.clone(),
            owner_pid,
            role: role_name.to_string(),
            now_secs: current_epoch_secs(),
            lease_secs: controller::CLOSEOUT_OWNER_LEASE_SECS,
            allow_dead_owner_takeover: true,
        },
    )? {
        controller::CloseoutOwnerClaimOutcome::Acquired(owner) => owner.cycle_id,
        controller::CloseoutOwnerClaimOutcome::HeldByOther(owner) => {
            anyhow::bail!(
                "closeout operation is already in progress for cycle {} by {} pid={} role={} until={}",
                owner.cycle_id,
                owner.owner_id,
                owner.owner_pid,
                owner.role,
                owner.expires_secs
            );
        }
        controller::CloseoutOwnerClaimOutcome::CycleSuperseded => return Ok(None),
    };

    let (stop, stopped) = std::sync::mpsc::channel();
    let heartbeat_file = file.to_path_buf();
    let heartbeat_cycle = cycle_id.clone();
    let heartbeat_owner = owner_id.clone();
    let heartbeat = std::thread::spawn(move || {
        let interval =
            std::time::Duration::from_secs((controller::CLOSEOUT_OWNER_LEASE_SECS / 3).max(1));
        loop {
            match stopped.recv_timeout(interval) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            let refreshed = controller::claim_closeout_owner_for_file(
                &heartbeat_file,
                controller::CloseoutOwnerClaimRequest {
                    expected_cycle_id: Some(heartbeat_cycle.clone()),
                    owner_id: heartbeat_owner.clone(),
                    owner_pid,
                    role: role_name.to_string(),
                    now_secs: current_epoch_secs(),
                    lease_secs: controller::CLOSEOUT_OWNER_LEASE_SECS,
                    allow_dead_owner_takeover: false,
                },
            );
            if !matches!(
                refreshed,
                Ok(controller::CloseoutOwnerClaimOutcome::Acquired(_))
            ) {
                agent_doc_ops_log_io::log_op(
                    &heartbeat_file,
                    &format!(
                        "closeout_owner_heartbeat_stopped file={} cycle_id={} owner_id={} outcome={refreshed:?}",
                        heartbeat_file.display(),
                        heartbeat_cycle,
                        heartbeat_owner,
                    ),
                );
                break;
            }
        }
    });
    ACTIVE_FOREGROUND_CLOSEOUTS.with(|active| active.borrow_mut().push(owner_key.clone()));
    Ok(Some(CloseoutOwnerGuard {
        file: file.to_path_buf(),
        owner_key,
        cycle_id,
        owner_id,
        stop: Some(stop),
        heartbeat: Some(heartbeat),
    }))
}

pub fn run_command_with_response(
    options: CommandOptions,
    commit_mode: CommitMode,
    response: String,
) -> Result<()> {
    let previous = RESPONSE_STDIN_OVERRIDE.with(|slot| slot.replace(Some(response)));
    let result = run_command(options, commit_mode);
    RESPONSE_STDIN_OVERRIDE.with(|slot| {
        slot.replace(previous);
    });
    result
}

/// Persist a cumulative, semantically complete response into the live document
/// without applying closeout mutations or committing the cycle.
pub fn checkpoint_response(file: &Path, response: &str) -> Result<()> {
    run_entry::checkpoint_response(file, response)
}

pub fn run_command_with_empty_response_recovery(
    options: CommandOptions,
    commit_mode: CommitMode,
    empty_response_recovery: EmptyResponseRecovery,
) -> Result<()> {
    run_command_inner(options, commit_mode, Some(empty_response_recovery))
}

/// How long a NON-INTERACTIVE stdin may stay silent before it is treated as hung.
///
/// `0` disables the bound entirely. Override with `AGENT_DOC_STDIN_TIMEOUT_SECS`.
const STDIN_SILENT_TIMEOUT_SECS: u64 = 60;

/// How long to wait for stdin to reach EOF, or `None` to wait forever.
///
/// `#writestdinhang`: `read_to_string(stdin)` blocks until EOF, and an automation
/// caller that never closes fd 0 never delivers one. Observed 2026-07-20 — two
/// ~15-minute hangs where fd 0 was a still-open socket belonging to the calling
/// harness; `/proc/<pid>/task/*/wchan` showed `unix_stream_read_generic`. The
/// process cannot distinguish that from "the producer is about to write", so it
/// waits indefinitely and the operator sees a mystery hang.
///
/// A TTY is exempt and waits forever: a human composing a response at a terminal
/// is legitimately silent for minutes, and a deadline there would truncate real
/// input. Only a non-interactive stdin — pipe, socket, file — is bounded, because
/// a producer that has piped nothing after a full minute is not coming.
pub fn stdin_read_deadline(is_terminal: bool, timeout_secs: u64) -> Option<std::time::Duration> {
    if is_terminal || timeout_secs == 0 {
        return None;
    }
    Some(std::time::Duration::from_secs(timeout_secs))
}

fn configured_stdin_timeout_secs() -> u64 {
    std::env::var("AGENT_DOC_STDIN_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(STDIN_SILENT_TIMEOUT_SECS)
}

fn read_response_input() -> Result<String> {
    if let Some(response) = RESPONSE_STDIN_OVERRIDE.with(|slot| slot.borrow_mut().take()) {
        return Ok(response);
    }

    let deadline = stdin_read_deadline(
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
        configured_stdin_timeout_secs(),
    );
    let Some(deadline) = deadline else {
        let mut response = String::new();
        std::io::stdin()
            .read_to_string(&mut response)
            .context("failed to read response from stdin")?;
        return Ok(response);
    };

    // The blocked read cannot be cancelled, so it runs on a detached thread and
    // dies with the process. Bounding the WAIT (not the read) is what turns an
    // indefinite hang into an actionable error.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut response = String::new();
        let result = std::io::stdin()
            .read_to_string(&mut response)
            .map(|_| response);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(deadline) {
        Ok(result) => result.context("failed to read response from stdin"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => anyhow::bail!(
            "no response arrived on stdin after {}s and it never closed, so this would have waited forever.\n\
             `agent-doc write` reads the response body from stdin. Either pipe one in:\n\
             \n\
             \x20   cat <<'RESPONSE' | agent-doc write --commit <FILE>\n\
             \x20   <!-- patch:exchange --> ... <!-- /patch:exchange -->\n\
             \x20   RESPONSE\n\
             \n\
             or, for a flags-only invocation (for example `--backlog-add`), close stdin with `< /dev/null`.\n\
             Set AGENT_DOC_STDIN_TIMEOUT_SECS=0 to wait indefinitely instead.",
            deadline.as_secs()
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("stdin reader thread died before delivering a response")
        }
    }
}

fn read_response_input_for_closeout(strict_closeout: bool) -> Result<String> {
    let response = read_response_input()?;
    if !strict_closeout {
        return Ok(response);
    }
    Ok(
        agent_doc_template::response_materialization::canonicalize_strict_closeout_response_heading(
            &response,
        ),
    )
}

fn log_resolved_backlog_prompt_cleanup(file: &Path, removed_total: usize) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "cleanup_resolved_backlog_prompts file={} removed={}",
            file.display(),
            removed_total
        ),
    );
    eprintln!(
        "[write] removed {} resolved prompt target(s) from backlog component(s)",
        removed_total
    );
}

#[cfg(test)]
fn log_splice_pending_component_warning(warning: &SplicePendingComponentWarning) {
    match warning {
        SplicePendingComponentWarning::SourceParseFailed(err) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse source components: {}",
                err
            );
        }
        SplicePendingComponentWarning::TargetParseFailed(err) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse target components: {}",
                err
            );
        }
        SplicePendingComponentWarning::TargetMissingBacklogComponent => {
            eprintln!(
                "[write] WARNING: splice_pending: source has tracked backlog content but target does not - pending mutations may be lost on IPC fallback"
            );
        }
    }
}

/// Resolve the common ancestor handed to the write state machine from the
/// content-bearing state.db projection. There is no filesystem-baseline override.
fn read_document_baseline(file: &Path) -> Result<Option<String>> {
    agent_doc_snapshot_io::load_document_baseline(file)
}

/// Pre-write gate for a baseline-backed closeout against an already-`committed`
/// cycle (`#finalize-stale-baseline-reopen-friction`).
///
/// The cycle phase is `Committed` whenever the prior finalize already closed and
/// no fresh `preflight` reopened the cycle (for example a post-commit retry or a
/// second response in the same turn). Historically this always failed
/// closed with "run `agent-doc preflight` and retry", forcing a manual reopen even
/// for a legitimately new response.
///
/// Returns:
/// - `Ok(None)` — no gate applies (open cycle or non-finalize mode).
/// - `Ok(Some(fresh_baseline))` — a genuinely new response was supplied after the
///   commit, so the cycle is auto-reopened from `HEAD` and the caller must diff the
///   new response against this `HEAD` baseline (the stale explicit baseline is
///   discarded). This is exactly what a manual `preflight` reopen would do.
/// - `Err(..)` — fail closed. A true replay (the incoming response is already
///   materialized in `HEAD`) must not be re-applied (duplicate-block risk); an
///   empty/repair response or a non-git document cannot be safely auto-reopened.
fn guard_no_baseline_replay_after_committed_cycle(
    file: &Path,
    commit_mode: CommitMode,
    has_tracked_work_mutations: bool,
) -> Result<Option<String>> {
    if commit_mode != CommitMode::Required {
        return Ok(None);
    }

    let Some(cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file) else {
        return Ok(None);
    };

    // `#committedwedge`: a pending-only closeout (`--backlog-add`, `--done`,
    // `--review-add`, `--status`, …) carries no response body, so the
    // duplicate-block risk this gate exists to prevent cannot apply. Rejecting it
    // produced a loop with no operator escape — `write --commit` said "run
    // preflight", and preflight reported `no_changes` (or failed closed on a
    // snapshot/HEAD mismatch) and pointed back at `write --commit`, leaving
    // legitimate backlog bookkeeping impossible to record after a committed cycle.
    //
    // Reopen from HEAD so the mutation lands through the normal binary-owned
    // snapshot/commit boundary, exactly as a manual `preflight` reopen would.
    if has_tracked_work_mutations {
        let Some(head) = agent_doc_git_io::revision::show_head(file).ok().flatten() else {
            // No HEAD to re-baseline from; fall through to the normal gate so the
            // non-git / empty-repo case keeps its existing fail-closed behavior.
            return guard_no_baseline_replay_after_committed_cycle_inner(file, commit_mode);
        };
        agent_doc_cycle_state_io::start_preflight(file, Some(&head), Some(&head))?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "baseline_replay_pending_only_reopened file={} cycle_id={}",
                file.display(),
                cycle_id
            ),
        );
        eprintln!(
            "[finalize] cycle `{cycle_id}` was already committed; reopened a fresh cycle for a \
             pending-only tracked-work update (#committedwedge)"
        );
        return Ok(Some(head));
    }

    guard_no_baseline_replay_after_committed_cycle_inner(file, commit_mode)
}

fn guard_no_baseline_replay_after_committed_cycle_inner(
    file: &Path,
    commit_mode: CommitMode,
) -> Result<Option<String>> {
    if commit_mode != CommitMode::Required {
        return Ok(None);
    }

    let Some(cycle_id) = agent_doc_flow_io::closeout::cycle_already_committed(file) else {
        return Ok(None);
    };

    // Read the incoming response now and re-stash it so the downstream write path
    // (which calls `read_response_input` once for the resolved mode) still sees it.
    let response = read_response_input_for_closeout(commit_mode == CommitMode::Required)?;
    RESPONSE_STDIN_OVERRIDE.with(|slot| {
        slot.borrow_mut().replace(response.clone());
    });

    let head = agent_doc_git_io::revision::show_head(file).ok().flatten();

    let reject = |reason: &str| -> anyhow::Error {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "baseline_replay_rejected file={} cycle_id={} reason={reason}",
                file.display(),
                cycle_id
            ),
        );
        anyhow::anyhow!(
            "[finalize] pre-write gate: the latest agent-doc cycle `{}` for {} is already `committed`; refusing to replay the response without reopening the binary-owned write/commit path. Run `agent-doc preflight {}` and retry.",
            cycle_id,
            file.display(),
            file.display()
        )
    };

    // True replay: the incoming response is already committed in HEAD. Re-applying
    // it risks a duplicate block, and the work is already durable — fail closed.
    if let Some(head) = head.as_deref()
        && response_materialized_in_content(&response, head)
    {
        return Err(reject("response_already_in_head"));
    }

    // Empty/repair response (pending-only closeout) — keep failing closed; the
    // auto-reopen path is for genuinely new assistant responses only.
    if response.trim().is_empty() {
        return Err(reject("empty_response"));
    }

    // No HEAD (non-git or empty repo) — cannot mint a safe HEAD baseline.
    let Some(head) = head else {
        return Err(reject("no_head_baseline"));
    };

    // Genuinely new response after a committed cycle: auto-reopen a fresh
    // `preflight_started` cycle from HEAD instead of forcing a manual preflight,
    // and hand the caller the HEAD baseline so the new response diffs against the
    // actual committed state (the stale explicit baseline is discarded).
    agent_doc_cycle_state_io::start_preflight(file, Some(&head), Some(&head))?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "baseline_replay_auto_reopened file={} cycle_id={}",
            file.display(),
            cycle_id
        ),
    );
    eprintln!(
        "[finalize] cycle `{}` was already committed; auto-reopened a fresh cycle for the new response (baseline refreshed to HEAD) instead of requiring a manual preflight.",
        cycle_id
    );
    Ok(Some(head))
}

fn ensure_pending_add_target(target: &Path) -> Result<()> {
    if !target.exists() {
        anyhow::bail!(
            "--backlog-add-to target file not found (also --pending-add-to target file not found): {}",
            target.display()
        );
    }
    let target_doc = resolve_current_document(target, "validate_backlog_add_to_target")?;
    let content = target_doc.content();
    let components = agent_doc_element::element::parse(content).with_context(|| {
        format!(
            "failed to parse --backlog-add-to target {}",
            target.display()
        )
    })?;
    if !components
        .iter()
        .any(|component| agent_doc_element::element::is_backlog_component(&component.name))
    {
        anyhow::bail!(
            "--backlog-add-to target {} has no agent:backlog/agent:pending component",
            target.display()
        );
    }
    Ok(())
}

fn is_session_document(file: &Path) -> Result<bool> {
    let current = resolve_current_document(file, "is_session_document")?;
    is_session_document_content(current.content())
}

fn is_session_document_with_force_disk(file: &Path, force_disk: bool) -> Result<bool> {
    if !force_disk {
        return match is_session_document(file) {
            Ok(is_session) => Ok(is_session),
            Err(err) if error_requests_retry_without_disk(&err) => Ok(true),
            Err(err) => Err(err),
        };
    }
    let current = resolve_force_disk_document(file, "is_session_document")?;
    is_session_document_content(current.content())
}

fn is_session_document_content(content: &str) -> Result<bool> {
    let (fm, _) = frontmatter::parse(content)?;
    Ok(fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty()))
}

fn resolve_commit_mode(
    file: &Path,
    requested: CommitMode,
    pending_only: bool,
    force_disk: bool,
) -> Result<CommitMode> {
    if pending_only || requested != CommitMode::BestEffort {
        return Ok(requested);
    }
    if is_session_document_with_force_disk(file, force_disk)? {
        return Ok(CommitMode::Required);
    }
    Ok(CommitMode::BestEffort)
}

fn guard_no_exchange_compaction_request_for_diff(file: &Path, diff_text: &str) -> Result<()> {
    if agent_doc_diff::detect_exchange_compaction_request(diff_text) {
        anyhow::bail!(
            "bare `compact exchange` directive detected in the current diff; close this turn \
             through the binary compaction path instead: `{}` \
             (optionally add `--message ...` for a custom checkpoint summary)",
            compact_command_hint(file)
        );
    }
    Ok(())
}

fn guard_no_exchange_compaction_request_between(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
) -> Result<()> {
    let baseline_owned = baseline.map(ToOwned::to_owned).or_else(|| {
        agent_doc_snapshot_io::load_document_baseline(file)
            .ok()
            .flatten()
    });
    let Some(base) = baseline_owned.as_deref() else {
        return Ok(());
    };
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(base, current_content) else {
        return Ok(());
    };
    guard_no_exchange_compaction_request_for_diff(file, &diff_text)
}

fn consume_queue_prompts_for_done_ids_closeout(
    file: &Path,
    done_ids: &[String],
    force_disk: bool,
) -> Result<Option<QueueConsumptionOutcome>> {
    let result = if force_disk {
        queue_consume::consume_queue_prompts_with_outcome(
            file,
            done_ids,
            true,
            queue_consume_writeback_effects(true),
        )
    } else {
        queue_consume::consume_queue_prompts_for_done_ids_with_outcome(
            file,
            done_ids,
            queue_consume_writeback_effects(false),
        )
    };
    match result {
        Err(err) if !force_disk && error_requests_retry_without_disk(&err) => {
            Err(err.context(format!(
                "queue_consume: refused direct disk write for {} while editor authority is unavailable",
                file.display()
            )))
        }
        // `#qconsumenostrike`: refusing to strike an unaddressable head is a
        // SAFETY outcome, not a closeout failure. The head stays queued (the
        // conservative direction — unrun work is never marked complete), and
        // the response still commits. Making it fatal would leave every
        // document whose answered head is a multiline `---` prompt permanently
        // unable to close out, which is how this guard first presented.
        Err(err) if error_refused_unaddressable_queue_head(&err) => {
            eprintln!(
                "[queue] warning: leaving the answered head queued for {} — it is not addressable \
                 as a markdown node, so striking it could mark unrelated work complete \
                 (#qconsumenostrike). Closeout continues.",
                file.display()
            );
            Ok(None)
        }
        other => other,
    }
}

/// Give pending-only `--done` closeouts the same queue lifecycle boundary as a
/// response closeout. Consuming the active matching head records the
/// `Selected -> Completed` proof/fact pair in the reactive state plane; marking
/// any later matching rows keeps the Markdown projection convergent without
/// making that text the lifecycle authority.
fn complete_queue_prompts_for_pending_only_done(
    file: &Path,
    done_ids: &[String],
    commit_mode: CommitMode,
    force_disk: bool,
) -> Result<()> {
    if done_ids.is_empty() || commit_mode == CommitMode::None {
        return Ok(());
    }

    let complete = || -> Result<()> {
        consume_queue_prompts_for_done_ids_closeout(file, done_ids, force_disk)?;
        queue_consume::mark_completed_queue_prompts_for_done_ids(
            file,
            done_ids,
            force_disk,
            queue_consume_writeback_effects(force_disk),
        )?;
        Ok(())
    };
    match commit_mode {
        CommitMode::None => Ok(()),
        CommitMode::BestEffort => {
            if let Err(err) = complete() {
                eprintln!("[queue] warning: pending-only done lifecycle projection failed: {err}");
            }
            Ok(())
        }
        CommitMode::Required => complete(),
    }
}

/// True when a queue consume refused because the target head could not be
/// proven to be the node it would strike (`#qconsumenostrike`).
fn error_refused_unaddressable_queue_head(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("#qconsumenostrike")
}

fn set_status_with_options(file: &Path, text: &str, force_disk: bool) -> Result<()> {
    agent_doc_status_io::set_with_runtime_options(file, text, force_disk)
}

pub(crate) fn guard_stale_snapshot_recovery_only(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> bool {
    let _ = (file, snapshot_doc, current_doc, phase);
    false
}

fn apply_pending_and_status_mutations(
    file: &Path,
    options: &CommandOptions,
    pending_kept_open_ids: &[String],
    has_pending_ops: bool,
    reap_done_in_same_write: bool,
) -> Result<()> {
    if has_pending_ops || options.status.is_some() {
        let current_content =
            agent_doc_document_realtime_io::try_resolve_current_doc_from_file_with_source(
                file,
                "pending_status_write",
            )
            .map(|resolved| resolved.content)
            .with_context(|| format!("failed to resolve current document {}", file.display()))?;
        let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
            .ok()
            .flatten();
        guard_stale_snapshot_recovery_only(
            file,
            snapshot_doc.as_deref(),
            &current_content,
            "pending/status write",
        );
    }

    if has_pending_ops {
        // Record the requested mutation envelope before its editor projection.
        // A response write may already be retained in Lazily/CRDT and its ACK
        // worker may settle concurrently with this follow-up projection. The
        // cycle must therefore know that tracked work is still part of the same
        // closeout before any individual backlog write can block on its own ACK.
        // Concrete content guards still prove that promised ids actually land.
        agent_doc_cycle_state_io::mark_pending_mutations(file)?;
        if !options.pending_done.is_empty() {
            agent_doc_cycle_state_io::record_pending_done_ids(file, &options.pending_done)?;
        }
        if !options.pending_add.is_empty()
            || !options.pending_add_to.is_empty()
            || !options.pending_add_gated.is_empty()
            || !options.pending_add_after.is_empty()
            || !options.pending_add_before.is_empty()
            || !options.pending_add_back.is_empty()
            || !options.icebox_add.is_empty()
            || !options.icebox_add_after.is_empty()
            || !options.icebox_add_before.is_empty()
            || !options.icebox_add_back.is_empty()
            || !options.review_add.is_empty()
        {
            agent_doc_cycle_state_io::mark_pending_added(file)?;
        }
        agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || {
                backlog_cmd::with_force_disk_pending_writes(options.force_disk, || {
                    if options.pending_clear {
                        backlog_cmd::clear(file)?;
                    }
                    if options.icebox_clear {
                        backlog_cmd::icebox_clear(file)?;
                    }
                    // `#opsproof-samecycle-add`: track ids added this cycle so post-commit
                    // ops-proof auto-completion never reaps a brand-new same-cycle add.
                    let mut same_cycle_added_ids: Vec<String> =
                        backlog_cmd::add_many(file, &options.pending_add, false)?;
                    let pending_add_targets = group_pending_add_targets(&options.pending_add_to)?;
                    for (target, items) in &pending_add_targets {
                        ensure_pending_add_target(target)?;
                        backlog_cmd::add_many(target, items, false).with_context(|| {
                            format!(
                                "failed to apply --backlog-add-to target {}",
                                target.display()
                            )
                        })?;
                    }
                    same_cycle_added_ids.extend(backlog_cmd::add_many(
                        file,
                        &options.pending_add_gated,
                        true,
                    )?);
                    // #ah0s: explicit-position adds (after/before <id>, tail). Applied after
                    // the front-insert default so anchor ids added this same cycle resolve.
                    for pair in options.pending_add_after.chunks(2) {
                        if let [anchor, text] = pair {
                            let id =
                                backlog_cmd::add_after(file, anchor, text).with_context(|| {
                                    format!("failed to apply --backlog-add-after {anchor}")
                                })?;
                            same_cycle_added_ids.push(id);
                        } else {
                            anyhow::bail!("--backlog-add-after expects repeated ID TEXT pairs");
                        }
                    }
                    for pair in options.pending_add_before.chunks(2) {
                        if let [anchor, text] = pair {
                            let id =
                                backlog_cmd::add_before(file, anchor, text).with_context(|| {
                                    format!("failed to apply --backlog-add-before {anchor}")
                                })?;
                            same_cycle_added_ids.push(id);
                        } else {
                            anyhow::bail!("--backlog-add-before expects repeated ID TEXT pairs");
                        }
                    }
                    for text in &options.pending_add_back {
                        same_cycle_added_ids.push(backlog_cmd::add_back(file, text)?);
                    }
                    same_cycle_added_ids
                        .extend(backlog_cmd::icebox_add_many(file, &options.icebox_add)?);
                    for pair in options.icebox_add_after.chunks(2) {
                        if let [anchor, text] = pair {
                            let id = backlog_cmd::icebox_add_after(file, anchor, text)
                                .with_context(|| {
                                    format!("failed to apply --icebox-add-after {anchor}")
                                })?;
                            same_cycle_added_ids.push(id);
                        } else {
                            anyhow::bail!("--icebox-add-after expects repeated ID TEXT pairs");
                        }
                    }
                    for pair in options.icebox_add_before.chunks(2) {
                        if let [anchor, text] = pair {
                            let id = backlog_cmd::icebox_add_before(file, anchor, text)
                                .with_context(|| {
                                    format!("failed to apply --icebox-add-before {anchor}")
                                })?;
                            same_cycle_added_ids.push(id);
                        } else {
                            anyhow::bail!("--icebox-add-before expects repeated ID TEXT pairs");
                        }
                    }
                    for text in &options.icebox_add_back {
                        same_cycle_added_ids.push(backlog_cmd::icebox_add_back(file, text)?);
                    }
                    if !options.pending_add.is_empty()
                        || !options.pending_add_to.is_empty()
                        || !options.pending_add_gated.is_empty()
                        || !options.pending_add_after.is_empty()
                        || !options.pending_add_before.is_empty()
                        || !options.pending_add_back.is_empty()
                        || !options.icebox_add.is_empty()
                        || !options.icebox_add_after.is_empty()
                        || !options.icebox_add_before.is_empty()
                        || !options.icebox_add_back.is_empty()
                    {
                        agent_doc_cycle_state_io::mark_pending_mutations(file)?;
                        agent_doc_cycle_state_io::mark_pending_added(file)?;
                    }
                    if !same_cycle_added_ids.is_empty() {
                        agent_doc_cycle_state_io::record_pending_added_ids(
                            file,
                            &same_cycle_added_ids,
                        )?;
                    }
                    if !options.pending_edit.is_empty() {
                        let edits =
                            parse_tracked_work_edits(&options.pending_edit, "--backlog-edit")?;
                        backlog_cmd::edit_many(file, &edits)?;
                    }
                    if !options.icebox_edit.is_empty() {
                        let edits =
                            parse_tracked_work_edits(&options.icebox_edit, "--icebox-edit")?;
                        backlog_cmd::icebox_edit_many(file, &edits)?;
                    }
                    for id in &options.pending_gate {
                        backlog_cmd::gate(file, id)?;
                    }
                    if !options.pending_gate.is_empty() {
                        agent_doc_cycle_state_io::record_pending_gated_ids(
                            file,
                            &options.pending_gate,
                        )?;
                    }
                    for pair in &options.pending_set_gate_type {
                        let (id, gt) = pair.split_once('=').with_context(|| {
                            format!("--backlog-set-gate-type expects 'id=type', got: {}", pair)
                        })?;
                        backlog_cmd::set_gate_type(file, id, gt)?;
                    }
                    for pair in &options.pending_set_verify {
                        let (id, spec) = pair.split_once('=').with_context(|| {
                            format!(
                                "--backlog-set-verify expects 'id=<verify/disproof predicate spec>', got: {}",
                                pair
                            )
                        })?;
                        backlog_cmd::set_gate_verify(file, id, spec)?;
                    }
                    let mut review_added_ids: Vec<String> = Vec::new();
                    for value in &options.review_add {
                        if let Some(id) = backlog_cmd::review_add(file, value)? {
                            review_added_ids.push(id);
                        }
                    }
                    if !review_added_ids.is_empty() {
                        // `#opsproof-samecycle-add`: a freshly added gated review item must
                        // not be ops-proof auto-completed on the cycle it first appears.
                        agent_doc_cycle_state_io::record_pending_added_ids(
                            file,
                            &review_added_ids,
                        )?;
                    }
                    for pair in &options.review_edit {
                        let (id, text) = pair.split_once('=').with_context(|| {
                            format!("--review-edit expects 'id=text', got: {}", pair)
                        })?;
                        backlog_cmd::review_edit(file, id, text)?;
                    }
                    for id in &options.review_resolve {
                        backlog_cmd::review_resolve(file, id)?;
                    }
                    for id in &options.review_remove {
                        backlog_cmd::review_remove(file, id)?;
                    }
                    for id in &options.pending_ungate {
                        backlog_cmd::ungate(file, id)?;
                    }
                    record_pending_actionable_mutations(
                        file,
                        &same_cycle_added_ids,
                        &options.pending_ungate,
                    )?;
                    for gt in &options.pending_resolve_gate {
                        backlog_cmd::resolve_gate(file, gt)?;
                    }
                    for id in &options.pending_done {
                        agent_doc_session_check_io::enforce_review_done_guard(file, id)?;
                    }
                    if !options.pending_done.is_empty() {
                        let reap_outcome = if reap_done_in_same_write {
                            backlog_cmd::done_and_reap_many(file, &options.pending_done)?
                        } else {
                            for id in &options.pending_done {
                                backlog_cmd::done(file, id)?;
                            }
                            backlog_cmd::DoneAndReapOutcome {
                                removed_ids: Vec::new(),
                                target_content: None,
                            }
                        };
                        if let Some(target_content) = reap_outcome.target_content.as_deref() {
                            // Response closeouts commit from the response snapshot. Refresh
                            // that same commit input after the one-target reap so HEAD
                            // cannot lag the editor-authoritative tracked-work mutation.
                            agent_doc_snapshot_io::checkpoint_document_baseline(
                                file,
                                target_content,
                                agent_doc_ops_log_io::log_op,
                            )?;
                        }
                        agent_doc_cycle_state_io::record_pending_done_ids(
                            file,
                            &options.pending_done,
                        )?;
                        if !reap_outcome.removed_ids.is_empty() {
                            agent_doc_cycle_state_io::record_reaped_pending_ids(
                                file,
                                &reap_outcome.removed_ids,
                            )?;
                        }
                        agent_doc_cycle_state_io::mark_pending_mutations(file)?;
                    }
                    if let Some(ref order) = options.pending_reorder {
                        let ids = parse_id_order(order);
                        backlog_cmd::reorder(file, &ids)?;
                    }
                    if let Some(ref order) = options.icebox_reorder {
                        let ids = parse_id_order(order);
                        backlog_cmd::icebox_reorder(file, &ids)?;
                    }
                    if !pending_kept_open_ids.is_empty() {
                        agent_doc_cycle_state_io::record_pending_kept_open_ids(
                            file,
                            pending_kept_open_ids,
                        )?;
                    }
                    agent_doc_cycle_state_io::mark_pending_mutations(file)?;
                    Ok(())
                })
            },
        )?;
    }

    if let Some(ref status_text) = options.status {
        set_status_with_options(file, status_text, options.force_disk)?;
    }

    Ok(())
}

/// `#backlogqueuepopulation`: collapse every mutation that makes tracked work
/// executable into one binary-owned cycle fact. The queue reconciler consumes
/// this set after the backlog mutations land; unrelated open items are excluded
/// so operator queue deletions remain authoritative.
fn record_pending_actionable_mutations(
    file: &Path,
    added_ids: &[String],
    ungated_ids: &[String],
) -> Result<()> {
    let ids: Vec<String> = added_ids.iter().chain(ungated_ids).cloned().collect();
    if !ids.is_empty() {
        agent_doc_cycle_state_io::record_pending_actionable_ids(file, &ids)?;
    }
    Ok(())
}

fn write_outcome_retains_closeout_mutations(write_result: &Result<()>) -> bool {
    write_result.is_ok()
        || write_result
            .as_ref()
            .err()
            .is_some_and(error_requests_retry_without_disk)
}

fn run_command(options: CommandOptions, commit_mode: CommitMode) -> Result<()> {
    run_command_inner(options, commit_mode, None)
}

fn run_command_inner(
    options: CommandOptions,
    commit_mode: CommitMode,
    empty_response_recovery: Option<EmptyResponseRecovery>,
) -> Result<()> {
    let file = options.file.as_path();
    let closeout_role = WriteCloseoutOwnerRole::from_origin(options.origin.as_deref());
    let _closeout_owner = claim_foreground_closeout_owner(file, closeout_role)?;
    let _force_disk_authority_scope = if options.force_disk {
        Some(
            agent_doc_document_realtime_io::begin_force_disk_authority_scope(
                file,
                "finalize_write_force_disk_authorization",
            )?,
        )
    } else {
        None
    };
    let _ = agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
        file,
        "finalize_write_start",
    );

    if let Some(ref origin) = options.origin {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("write_origin file={} origin={}", file.display(), origin),
        );
    }
    // #jb-tsift-pane-sync diagnostic: capture a write/commit to `file` that is
    // executing inside a tmux pane owning a different document (the
    // cross-document contamination vector — e.g. a tsift.md-owned pane
    // committing agent-doc-bugs2.md's response).
    agent_doc_sync_io::sync::log_cross_document_execution_context(file, "write");

    // #manual-queue-head-loss: extend the `#queue-clear-unrun-items` removal-proof
    // anchor to user queue heads inserted AFTER preflight (for example a
    // `do [#id]` typed into `agent:queue` during a stalled / busy-pane dispatch
    // attempt). Read the live working-tree document here — before any pending
    // mutation or queue convergence mutates it — and union its directive heads
    // into the recorded set so closeout cannot silently drop a runnable manual
    // head whose backlog item is still open. Best-effort: an absent cycle state
    // is a no-op, and a read failure is logged (the real write path below reads
    // the document again and surfaces any genuine I/O error).
    match resolve_current_document(file, "observe_live_queue_heads") {
        Ok(live_doc) => {
            if let Err(err) =
                agent_doc_cycle_state_io::observe_live_queue_heads(file, live_doc.content())
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "observe_live_queue_heads_failed file={} err={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "observe_live_queue_heads_resolve_failed file={} err={}",
                    file.display(),
                    err
                ),
            );
        }
    }

    let has_pending_ops = options.has_pending_mutation();

    if options.pending_only && !has_pending_ops {
        anyhow::bail!("--backlog-only requires at least one backlog/icebox/review mutation flag");
    }
    if options.pending_only && (options.is_template || options.is_stream || options.is_ipc) {
        anyhow::bail!("--backlog-only cannot be combined with --template, --stream, or --ipc");
    }
    if options.pending_only && commit_mode == CommitMode::Required {
        anyhow::bail!("finalize does not support --backlog-only");
    }
    if !options.pending_add_to.len().is_multiple_of(2) {
        anyhow::bail!("--backlog-add-to expects repeated FILE TEXT pairs");
    }
    if options.commit_sibling.len() != options.commit_sibling_message.len() {
        anyhow::bail!(
            "--commit-sibling and --commit-sibling-message must be repeated the same number of times in positional pairs (got {} sibling(s), {} message(s))",
            options.commit_sibling.len(),
            options.commit_sibling_message.len()
        );
    }
    if !options.commit_sibling.is_empty() && commit_mode == CommitMode::None {
        anyhow::bail!(
            "--commit-sibling requires --commit (or `agent-doc finalize`); the sibling trailer URL needs the session-document commit sha"
        );
    }
    let pending_kept_open_ids = pending_kept_open_ids_from_mutations(
        &options.pending_edit,
        &options.pending_gate,
        &options.pending_ungate,
        &options.pending_set_gate_type,
        &options.pending_set_verify,
        &options.review_edit,
        options.pending_reorder.as_deref(),
    );
    let metadata_force_disk = options.force_disk || options.is_ipc;
    let commit_mode = match resolve_commit_mode(
        file,
        commit_mode,
        options.pending_only,
        metadata_force_disk,
    ) {
        Ok(mode) => mode,
        Err(err) if options.is_ipc && error_requests_retry_without_disk(&err) => {
            let response = read_response_input_for_closeout(commit_mode == CommitMode::Required)?;
            let retention_baseline = read_document_baseline(file).unwrap_or(None);
            if !response.trim().is_empty()
                && let Err(retain_err) = retain_ipc_patch_for_editor_authority_retry(
                    file,
                    retention_baseline.as_deref(),
                    &response,
                )
            {
                eprintln!(
                    "[write] warning: failed to retain IPC retry patch for {}: {}",
                    file.display(),
                    retain_err
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "commit_mode_ipc_retry_patch_retention_failed file={} error={} recovery=retry_without_disk_write",
                        file.display(),
                        retain_err
                    ),
                );
            }
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    // #final-response-transaction: session responses are never allowed to enter
    // the document through a non-committing write. Historically `write --stream`
    // was used for incremental response checkpoints and a later invocation wrote
    // the full response. That made a prefix authoritative, forced AlreadyApplied
    // recovery during the same healthy turn, and could leave malformed content for
    // unrelated maintenance to commit. Reject the operation before reading stdin,
    // creating a capture, or mutating the document. `finalize` and `write --commit`
    // own the one complete response+queue+backlog transaction.
    if commit_mode == CommitMode::None
        && !options.pending_only
        && is_session_document_with_force_disk(file, metadata_force_disk)?
    {
        anyhow::bail!(
            "partial or non-committing response writes are disabled for session documents; use `agent-doc finalize {}` (or `agent-doc write --commit {}`) once with the complete final response",
            file.display(),
            file.display(),
        );
    }
    if commit_mode == CommitMode::Required && !agent_doc_git_io::status::is_in_git_repo(file) {
        if is_session_document_with_force_disk(file, metadata_force_disk)? {
            anyhow::bail!(
                "write --commit requires a git repository for session documents so the cycle can reach a committed state"
            );
        }
        anyhow::bail!(
            "finalize requires a git repository so the cycle can reach a committed state"
        );
    }

    guard_historical_retained_write_before_new_capture(file, commit_mode, closeout_role)?;

    if options.pending_only {
        apply_pending_and_status_mutations(
            file,
            &options,
            &pending_kept_open_ids,
            has_pending_ops,
            commit_mode == CommitMode::Required,
        )?;
        complete_queue_prompts_for_pending_only_done(
            file,
            &options.pending_done,
            commit_mode,
            options.force_disk,
        )?;
        agent_doc_session_check_io::run_closeout_pending_maintenance(
            file,
            commit_mode == CommitMode::Required,
            options.force_disk,
            |file, force_disk| {
                if force_disk {
                    agent_doc_preflight_io::run_pending_maintenance_force_disk(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                } else {
                    agent_doc_preflight_io::run_pending_maintenance(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                }
            },
        )?;
        if commit_mode != CommitMode::None {
            if options.force_disk {
                agent_doc_lint_io::run_force_disk_with_logger(
                    file,
                    options.lint_override,
                    agent_doc_ops_log_io::log_op,
                )?;
            } else {
                agent_doc_lint_io::run_with_logger(
                    file,
                    options.lint_override,
                    agent_doc_ops_log_io::log_op,
                )?;
            }
        }
        // `#queueatcreate` / `#backlogqueuepopulation`: the `--backlog-only`
        // path used to return here, so a tracked-work-only write could commit an
        // add or ungate without mirroring the newly actionable ids into the
        // explicit queue. Run the same binary-owned reconciliation as the full
        // closeout path before returning.
        if commit_mode != CommitMode::None {
            let placement = follow_up_queue_placement(&options)?;
            if let Err(e) = with_backlog_effects(|| {
                agent_doc_preflight_io::sync_same_cycle_actionable_backlog_into_go_queue(
                    file, placement,
                )
            }) {
                eprintln!(
                    "[queue] warning: same-cycle actionable backlog queue sync failed: {}",
                    e
                );
            }
        }
        return finalize_commit(file, commit_mode, options.force_disk);
    }

    let write_flags = WriteFlags {
        allow_replace_pending: options.allow_replace_pending,
        has_pending_add: !options.pending_add.is_empty()
            || !options.pending_add_to.is_empty()
            || !options.pending_add_gated.is_empty()
            || !options.pending_add_after.is_empty()
            || !options.pending_add_before.is_empty()
            || !options.pending_add_back.is_empty()
            || !options.icebox_add.is_empty()
            || !options.icebox_add_after.is_empty()
            || !options.icebox_add_before.is_empty()
            || !options.icebox_add_back.is_empty()
            || !options.review_add.is_empty(),
        has_pending_done: !options.pending_done.is_empty(),
        has_pending_mutation: has_pending_ops,
        pending_done_ids: options.pending_done.clone(),
        queue_completion_ids: options.queue_completion_ids.clone(),
        pending_kept_open_ids: pending_kept_open_ids.clone(),
        strict_closeout: commit_mode == CommitMode::Required,
        force_disk: options.force_disk,
        no_pending_capture: options.no_pending_capture,
        mutation_plan_json: Some(serde_json::to_string(
            &options.captured_closeout_mutation_plan(),
        )?),
        empty_response_recovery,
        rerun_command_base: finalize_rerun_command_base(FinalizeRerunCommand {
            required_commit: commit_mode == CommitMode::Required,
            file,
            is_template: options.is_template,
            is_stream: options.is_stream,
            is_ipc: options.is_ipc,
            force_disk: options.force_disk,
            origin: options.origin.as_deref(),
            no_pending_capture: options.no_pending_capture,
            pending_add: &options.pending_add,
            pending_add_to: &options.pending_add_to,
            pending_add_gated: &options.pending_add_gated,
            pending_add_after: &options.pending_add_after,
            pending_add_before: &options.pending_add_before,
            pending_add_back: &options.pending_add_back,
            icebox_add: &options.icebox_add,
            icebox_add_after: &options.icebox_add_after,
            icebox_add_before: &options.icebox_add_before,
            icebox_add_back: &options.icebox_add_back,
            pending_done: &options.pending_done,
            pending_edit: &options.pending_edit,
            pending_clear: options.pending_clear,
            pending_reorder: options.pending_reorder.as_deref(),
            pending_gate: &options.pending_gate,
            pending_ungate: &options.pending_ungate,
            pending_resolve_gate: &options.pending_resolve_gate,
            pending_set_gate_type: &options.pending_set_gate_type,
            pending_set_verify: &options.pending_set_verify,
            review_add: &options.review_add,
            review_edit: &options.review_edit,
            allow_replace_pending: options.allow_replace_pending,
            pending_only: options.pending_only,
            status: options.status.as_deref(),
        }),
    };

    // `#committedwedge`: a write carrying explicit tracked-work mutations is a
    // pending-only closeout, not a response replay — there is no response body to
    // duplicate. It must not be rejected by the committed-cycle replay gate.
    let has_tracked_work_mutations = !options.pending_add.is_empty()
        || !options.pending_add_to.is_empty()
        || !options.pending_add_gated.is_empty()
        || !options.pending_add_after.is_empty()
        || !options.pending_add_before.is_empty()
        || !options.pending_add_back.is_empty()
        || !options.icebox_add.is_empty()
        || !options.icebox_add_after.is_empty()
        || !options.icebox_add_before.is_empty()
        || !options.icebox_add_back.is_empty()
        || !options.pending_done.is_empty()
        || !options.pending_edit.is_empty()
        || options.pending_clear
        || options.pending_reorder.is_some()
        || !options.pending_gate.is_empty()
        || !options.pending_ungate.is_empty()
        || !options.pending_resolve_gate.is_empty()
        || !options.pending_set_gate_type.is_empty()
        || !options.pending_set_verify.is_empty()
        || !options.review_add.is_empty()
        || !options.review_edit.is_empty()
        || options.status.is_some();

    let baseline = match guard_no_baseline_replay_after_committed_cycle(
        file,
        commit_mode,
        has_tracked_work_mutations,
    )? {
        // Auto-reopened a committed cycle for a genuinely new response: diff against
        // the fresh HEAD baseline, not a stale lifecycle projection.
        Some(fresh_head_baseline) => Some(fresh_head_baseline),
        None => read_document_baseline(file)?,
    };

    let current_content = if options.force_disk {
        Some(resolve_force_disk_document(file, "pre_write_guards")?.into_content())
    } else {
        match resolve_current_document(file, "pre_write_guards") {
            Ok(current) => Some(current.into_content()),
            Err(err) if options.is_ipc && error_requests_retry_without_disk(&err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pre_write_guards_deferred_to_ipc_retry file={} error={} recovery=retry_without_disk_write",
                        file.display(),
                        err
                    ),
                );
                None
            }
            Err(err) => return Err(err),
        }
    };
    if let Some(current_content) = current_content.as_deref() {
        guard_no_exchange_compaction_request_between(file, baseline.as_deref(), current_content)?;
    }
    let current_resolved_mode = if options.is_template || (!options.is_ipc && !options.is_stream) {
        let current_content = current_content
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("pre_write_guards unavailable before non-IPC write"))?;
        let (fm, _) = frontmatter::parse(current_content)?;
        Some(fm.resolve_mode())
    } else {
        None
    };
    let template_flag_on_crdt_doc = options.is_template
        && current_resolved_mode
            .as_ref()
            .is_some_and(|mode| mode.is_crdt());

    let write_result = if options.is_ipc {
        run_ipc(file, baseline.as_deref(), write_flags)
    } else if options.is_stream || template_flag_on_crdt_doc {
        if template_flag_on_crdt_doc && !options.is_stream {
            eprintln!(
                "[write] template write requested for realtime document; routing through stream document-model write path"
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "template_flag_realtime_routed_to_stream file={} recovery=reconcile_document_model",
                    file.display()
                ),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "template_flag_crdt_routed_to_stream file={} recovery=retry_crdt_instead",
                    file.display()
                ),
            );
        }
        run_stream(
            file,
            baseline.as_deref(),
            options.force_disk,
            options.origin.as_deref(),
            write_flags,
        )
    } else if options.is_template {
        run_template(
            file,
            baseline.as_deref(),
            options.origin.as_deref(),
            write_flags,
        )
    } else {
        if current_resolved_mode.is_some_and(|mode| mode.is_crdt()) {
            run_stream(
                file,
                baseline.as_deref(),
                options.force_disk,
                options.origin.as_deref(),
                write_flags,
            )
        } else {
            run(file, baseline.as_deref(), write_flags)
        }
    };

    let response_write_retained =
        write_result.is_err() && write_outcome_retains_closeout_mutations(&write_result);

    if write_outcome_retains_closeout_mutations(&write_result) {
        apply_pending_and_status_mutations(
            file,
            &options,
            &pending_kept_open_ids,
            has_pending_ops,
            write_result.is_ok() && commit_mode == CommitMode::Required,
        )
        .with_context(|| {
            if response_write_retained {
                format!(
                    "response target for {} is retained; failed to retain the same closeout's tracked-work mutations",
                    file.display()
                )
            } else {
                format!(
                    "failed to apply tracked-work mutations for {}",
                    file.display()
                )
            }
        })?;
    }

    if write_result.is_ok() {
        agent_doc_session_check_io::run_closeout_pending_maintenance(
            file,
            commit_mode == CommitMode::Required,
            options.force_disk,
            |file, force_disk| {
                if force_disk {
                    agent_doc_preflight_io::run_pending_maintenance_force_disk(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                } else {
                    agent_doc_preflight_io::run_pending_maintenance(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                }
            },
        )?;
    }

    // Phase 3b: pre-commit pending closeout gates (strict mode only).
    if write_result.is_ok() && commit_mode == CommitMode::Required {
        agent_doc_session_check_io::precommit_pending_capture_check_with_force_disk(
            file,
            options.force_disk,
        )?;
        agent_doc_session_check_io::precommit_pending_done_check_with_options(
            file,
            agent_doc_session_check_io::PendingDoneCheckOptions {
                force_disk: options.force_disk,
                backlog_effects: Some(
                    &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
                ),
            },
        )?;
    }

    // Phase 3b.1: tagpath agent-doc lint gate. Runs on the final file
    // state after the response/pending edits have merged, before the
    // snapshot/commit boundary. Errors fail the cycle closed so malformed
    // directives (for example `<!-- agent:done archive PATH -->` missing
    // `=`) cannot reach a committed state. Mode resolution: CLI override
    // > frontmatter `agent_doc_lint_dialect` > workspace `.agent-doc/
    // config.toml` `[lint] dialect` > default (`warn`).
    if write_result.is_ok() && commit_mode != CommitMode::None {
        if options.force_disk {
            agent_doc_lint_io::run_force_disk_with_logger(
                file,
                options.lint_override,
                agent_doc_ops_log_io::log_op,
            )?;
        } else {
            agent_doc_lint_io::run_with_logger(
                file,
                options.lint_override,
                agent_doc_ops_log_io::log_op,
            )?;
        }
    }

    // Phase 3c: consume queue prompt after all other strict closeout gates
    // have passed so a rejected closeout cannot advance the queue early. The
    // layered completion signals — explicit `do queue`/prompt-target/`--done`
    // triggers, explicit `--done`/`--pending-gate`/`--review-resolve`/
    // `--pending-edit` completion of an id-backed head, a synthetic/preset
    // heading-id match, and a free-text head answered by this cycle's response —
    // all resolve through
    // `queue_consumption_allowed_for_response` so every successful closeout uses
    // an identical decision. Unproven IPC retries fail before this phase and do
    // not advance the queue.
    if write_result.is_ok() {
        let response_body = active_capture_response_body_for_write(file)?;
        let refreshed_current_content;
        let current_content_for_queue = match current_content.as_deref() {
            Some(content) => content,
            None => {
                refreshed_current_content = if options.force_disk {
                    resolve_force_disk_document(file, "queue_consumption")?.into_content()
                } else {
                    resolve_current_document(file, "queue_consumption")?.into_content()
                };
                refreshed_current_content.as_str()
            }
        };
        let mut queue_completion_ids = agent_doc_queue::queue_heads::explicit_queue_completion_ids(
            &options.pending_done,
            &options.pending_gate,
            &options.pending_edit,
            &options.review_resolve,
        );
        queue_completion_ids.extend(options.queue_completion_ids.iter().cloned());
        // `#donestrikeextra`: the strike/consume set is the RESOLUTION set, which
        // excludes `--backlog-edit`. Editing an item's text is the "keep/narrow
        // it" outcome, so an edited id is still open; feeding it to the strike
        // matcher silently removed unfinished items from the drain.
        let mut queue_resolution_ids = agent_doc_queue::queue_heads::explicit_queue_resolution_ids(
            &options.pending_done,
            &options.pending_gate,
            &options.review_resolve,
        );
        queue_resolution_ids.extend(options.queue_completion_ids.iter().cloned());
        let queue_consumption_allowed = queue_consumption_allowed_for_response(
            file,
            baseline.as_deref(),
            current_content_for_queue,
            &response_body,
            &queue_completion_ids,
        )?;
        if queue_consumption_allowed
            && let Some(head_id) = queue_targeted_completion_id_for_current_head(
                file,
                baseline.as_deref(),
                current_content_for_queue,
                &response_body,
                &options.pending_done,
            )?
            && !queue_resolution_ids
                .iter()
                .any(|id| agent_doc_queue::queue_response::normalize_done_id(id) == head_id)
        {
            queue_resolution_ids.push(head_id);
        }
        match commit_mode {
            CommitMode::None => {}
            CommitMode::BestEffort => {
                if queue_consumption_allowed {
                    if let Err(e) = consume_queue_prompts_for_done_ids_closeout(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                    ) {
                        eprintln!("[queue] warning: consumption failed: {}", e);
                    }
                    if let Err(e) = queue_consume::mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                        queue_consume_writeback_effects(options.force_disk),
                    ) {
                        eprintln!("[queue] warning: done-id marking failed: {}", e);
                    }
                } else {
                    match queue_consume::mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                        queue_consume_writeback_effects(options.force_disk),
                    ) {
                        Ok(0) => {
                            eprintln!(
                                "{}",
                                queue_skip_diagnostic_for_current_document(
                                    file,
                                    options.force_disk
                                )?
                            )
                        }
                        Ok(_) => {}
                        Err(e) => eprintln!("[queue] warning: done-id marking failed: {}", e),
                    }
                }
            }
            CommitMode::Required => {
                if queue_consumption_allowed {
                    consume_queue_prompts_for_done_ids_closeout(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                    )?;
                    queue_consume::mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                        queue_consume_writeback_effects(options.force_disk),
                    )?;
                } else {
                    let marked = queue_consume::mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_resolution_ids,
                        options.force_disk,
                        queue_consume_writeback_effects(options.force_disk),
                    )?;
                    if marked == 0 {
                        eprintln!(
                            "{}",
                            queue_skip_diagnostic_for_current_document(file, options.force_disk)?
                        );
                    }
                }
            }
        }

        // `#ftstrike`: strike any free-text queue head this cycle's response
        // answered, regardless of position. The leading-head consume above only
        // strikes a contiguous leading run and stops at an id-backed head, so a
        // free-text report sitting BEHIND an unfinished `do [#id]` head (the
        // common case while a backlog directive head is still draining) was never
        // struck even after the response addressed it — the operator could not
        // tell which typed reports were answered. This runs independent of
        // `queue_consumption_allowed` (which governs the leading head only) and is
        // best-effort: a missed strike must never fail an otherwise-clean closeout.
        if commit_mode != CommitMode::None {
            match queue_consume::strike_answered_free_text_queue_heads(
                file,
                &response_body,
                options.force_disk,
                queue_consume_writeback_effects(options.force_disk),
            ) {
                Ok(0) => {}
                Ok(n) => eprintln!("[queue] struck {n} answered free-text head(s) (#ftstrike)"),
                Err(e) => eprintln!("[queue] warning: free-text head strike failed: {e}"),
            }
        }
    }

    // `#pendingaddqueuesync` / `#backlogqueuepopulation`: add and ungate
    // mutations are applied during write/finalize, after preflight's
    // backlog→queue sync has already run. Once the current head has been
    // consumed, reconcile ids made actionable this cycle into active go-mode
    // queues. Placement defaults to the queue head so a follow-up filed by this
    // turn is picked up next rather than buried behind the existing queue.
    if write_result.is_ok() && commit_mode != CommitMode::None {
        let placement = follow_up_queue_placement(&options)?;
        match commit_mode {
            CommitMode::None => {}
            CommitMode::BestEffort => {
                if let Err(e) = with_backlog_effects(|| {
                    agent_doc_preflight_io::sync_same_cycle_actionable_backlog_into_go_queue(
                        file, placement,
                    )
                }) {
                    eprintln!(
                        "[queue] warning: same-cycle actionable backlog queue sync failed: {}",
                        e
                    );
                }
            }
            CommitMode::Required => {
                with_backlog_effects(|| {
                    agent_doc_preflight_io::sync_same_cycle_actionable_backlog_into_go_queue(
                        file, placement,
                    )
                })?;
            }
        }
    }

    let commit_result = if write_result.is_ok() {
        let primary = finalize_commit(file, commit_mode, options.force_disk);
        if primary.is_ok() && !options.commit_sibling.is_empty() {
            let pairs: Vec<(std::path::PathBuf, String)> = options
                .commit_sibling
                .iter()
                .cloned()
                .zip(options.commit_sibling_message.iter().cloned())
                .collect();
            agent_doc_git_io::sibling::commit_siblings_for_session_doc(file, &pairs)?;
        }
        primary
    } else {
        Ok(())
    };
    match (write_result, commit_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(write_err), Ok(())) => Err(write_err),
        (Ok(()), Err(commit_err)) => Err(commit_err),
        (Err(write_err), Err(commit_err)) => Err(write_err.context(commit_err.to_string())),
    }
}

/// Reject a fresh response before it can create capture/intent state when an
/// older document-write effect is still unfinished.
///
/// Preflight owns the same gate, but `finalize` can auto-reopen a committed
/// cycle for a new response. Without this write-entry guard, that shortcut
/// captured the new response first and only then discovered the historical
/// delivery sink. Every rejected retry therefore left another captured-only
/// orphan behind the same already-answered heading.
fn guard_historical_retained_write_before_new_capture(
    file: &Path,
    commit_mode: CommitMode,
    closeout_role: WriteCloseoutOwnerRole,
) -> Result<()> {
    if commit_mode != CommitMode::Required {
        return Ok(());
    }
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let open_capture = state.as_ref().is_some_and(|state| {
        state.capture_id.is_some()
            && state.response_sha256.is_some()
            && matches!(
                state.phase,
                agent_doc_turn::CyclePhase::ResponseCaptured
                    | agent_doc_turn::CyclePhase::WriteApplied
            )
    });
    let retrying_same_capture = if open_capture {
        match (
            agent_doc_capture_io::load_active(file)?,
            agent_doc_document_realtime_io::pending_document_write(file),
        ) {
            (Some(capture), Some(pending)) => retained_target_contains_capture_response(
                &capture.response_body,
                &pending.target_content,
            ),
            _ => false,
        }
    } else {
        false
    };
    if historical_retained_write_guard_may_bypass(
        open_capture,
        retrying_same_capture,
        closeout_role,
    ) {
        if open_capture
            && !retrying_same_capture
            && closeout_role == WriteCloseoutOwnerRole::CapturedFinalizeResume
        {
            let editor_save_settled =
                agent_doc_document_realtime_io::settle_live_editor_projection_through_authority(
                    file,
                    "captured_finalize_resume_pre_capture_editor_save",
                )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "captured_finalize_resume_supersedes_unrelated_retained_target file={} action=continue_exact_captured_replay_without_prior_delivery_wait editor_native_save_settled={editor_save_settled}",
                    file.display(),
                ),
            );
        }
        return Ok(());
    }

    let boundary = agent_doc_document_realtime_io::RetainedWriteCycleBoundary::FinalizePreCapture;
    let recovered =
        agent_doc_document_realtime_io::recover_retained_document_write_before_new_cycle(
            file, boundary,
        )?;
    if recovered {
        agent_doc_document_realtime_io::retained_write_settlement(
            file,
            boundary.recovered_settlement_source(),
        );
    }
    if agent_doc_document_realtime_io::retained_write_blocks_new_cycle(file, boundary.gate_source())
    {
        anyhow::bail!(
            "[finalize] retained document-write delivery from a prior cycle remains unsettled for {}; refusing the new response before capture/admission. Automatic controller reconciliation remains scheduled. Run only `agent-doc session-check {}` after it settles; do not resubmit finalize, force disk, or replace the queued edit.",
            file.display(),
            file.display(),
        );
    }
    Ok(())
}

fn historical_retained_write_guard_may_bypass(
    open_capture: bool,
    retrying_same_capture: bool,
    closeout_role: WriteCloseoutOwnerRole,
) -> bool {
    retrying_same_capture
        || (open_capture && closeout_role == WriteCloseoutOwnerRole::CapturedFinalizeResume)
}

fn retained_target_contains_capture_response(response: &str, retained_target: &str) -> bool {
    agent_doc_turn::response_replay::response_materialized_in_content(response, retained_target)
}

/// `#queueatcreate`: resolve where items created this cycle land in the queue.
///
/// An explicit `--backlog-queue-placement` always wins. Otherwise the queue
/// mirrors the *backlog insertion intent* the agent already expressed:
///
/// - front-inserting flags (`--backlog-add`, `--backlog-add-gated`,
///   `--backlog-add-after/-before`) mean "this is the next most relevant work",
///   so the queue head is the matching placement. This is the common case and
///   the one that was broken: appending buried fresh follow-ups behind the whole
///   queue, where they were effectively never picked up.
/// - `--backlog-add-back` explicitly means "put this at the tail". Prepending it
///   to the queue would contradict the flag, so a back-insert-only cycle appends.
///
/// A mixed cycle prefers the head: the front-inserted items are the ones the
/// agent marked as most relevant, and the queue skips ids it already holds.
fn follow_up_queue_placement(
    options: &CommandOptions,
) -> Result<agent_doc_queue::backlog_sync::FollowUpQueuePlacement> {
    use agent_doc_queue::backlog_sync::FollowUpQueuePlacement;
    if let Some(raw) = options.backlog_queue_placement.as_deref() {
        return FollowUpQueuePlacement::parse(raw).with_context(|| {
            format!("--backlog-queue-placement expects `prepend` or `append`, got: {raw}")
        });
    }
    let front_inserts = !options.pending_add.is_empty()
        || !options.pending_add_gated.is_empty()
        || !options.pending_add_after.is_empty()
        || !options.pending_add_before.is_empty()
        || !options.pending_add_to.is_empty();
    let back_inserts_only = !options.pending_add_back.is_empty() && !front_inserts;
    Ok(if back_inserts_only {
        FollowUpQueuePlacement::Append
    } else {
        FollowUpQueuePlacement::Prepend
    })
}

/// Run `f` with the tracked-work write effects installed.
///
/// `#queueatcreate`: the same-cycle queue enqueue falls back to the tracked-work
/// write path (the one the accompanying backlog mutation used) when the
/// queue-maintenance path cannot reach a ready editor model. That path resolves
/// its IO through a thread-local effects scope, which the write runtime installs
/// around its backlog mutations but which is NOT in place at closeout — so
/// calling it bare failed with "backlog command write effects are not installed",
/// a structural error that reads like an authority failure.
fn with_backlog_effects<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    agent_doc_element_backlog_io::with_backlog_command_effects(
        &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
        f,
    )
}

fn active_capture_response_body_for_write(file: &Path) -> Result<String> {
    if let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        && let Some(capture_id) = state.capture_id.as_deref()
        && let Some(projected) =
            agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
        && projected.cycle_id == state.cycle_id
        && state
            .response_sha256
            .as_deref()
            .is_none_or(|sha| sha == projected.response_sha256)
    {
        return Ok(projected.response_body);
    }
    Ok(agent_doc_capture_io::load_active(file)?
        .map(|capture| capture.response_body)
        .unwrap_or_default())
}

fn finalize_commit(file: &Path, commit_mode: CommitMode, force_disk: bool) -> Result<()> {
    match commit_mode {
        CommitMode::None => Ok(()),
        CommitMode::BestEffort => {
            if agent_doc_git_io::status::is_in_git_repo(file) {
                let session_document = is_session_document_with_force_disk(file, force_disk)?;
                // `#crdtauth4` — authority-gated commit barrier (plan phase 4).
                // No-op under `GitAuthoritative` (Detached); under `MultiReplica`
                // flushes live editor replicas to a consistent cut before commit.
                let barrier_ready = match agent_doc_controller_io::project_controller::
                    commit_barrier_via_controller_model_for_doc(file)
                {
                    Ok(ready) => ready,
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "commit_editor_authority_unavailable file={} reason=controller_barrier_error error={err}",
                                file.display()
                            ),
                        );
                        false
                    }
                };
                if !barrier_ready {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "commit_editor_authority_unavailable file={} reason=relay_convergence_pending",
                            file.display()
                        ),
                    );
                    log_closeout_guard(
                        file,
                        agent_doc_flow::types::FlowStage::PreCommitGuard,
                        agent_doc_flow::types::FlowOutcome::Blocked,
                        agent_doc_turn::closeout_guard::CloseoutGuardReason::ReplicaDeliveryPending,
                    );
                    eprintln!(
                        "[commit] skipped: live editor relay convergence is still pending for {}",
                        file.display()
                    );
                    if session_document {
                        anyhow::bail!(
                            "{}",
                            agent_doc_git_io::live_buffer_guard::crdt_relay_pending_refusal(file)
                        );
                    }
                    return Ok(());
                }
                match agent_doc_flow_io::closeout::CloseoutEffects::commit_for_authority(
                    &agent_doc_closeout_runtime_io::closeout_effects(),
                    file,
                    force_disk,
                ) {
                    // `#staleinmem` — record what we just committed so a later
                    // out-of-band disk correction is detectable at the next barrier.
                    Ok(_) => {
                        if let Err(err) = agent_doc_controller_io::project_controller::
                            record_committed_baseline_via_controller_model_for_doc(file)
                        {
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "controller_crdt_record_committed_baseline_error file={} error={err}",
                                    file.display()
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[commit] warning: {}", e);
                        if session_document {
                            return Err(e.context(format!(
                                "session-document best-effort commit failed for {}",
                                file.display()
                            )));
                        }
                    }
                }
                agent_doc_session_check_io::enforce_clean_closeout_with_force_disk(
                    file,
                    force_disk,
                    &agent_doc_closeout_runtime_io::session_check_effects(),
                )?;
            } else {
                eprintln!("[commit] skipped (not in git repo)");
            }
            Ok(())
        }
        CommitMode::Required => complete_required_closeout(file, force_disk).map(|_| ()),
    }
}

fn complete_required_closeout(file: &Path, force_disk: bool) -> Result<bool> {
    agent_doc_closeout_runtime_io::complete_required_closeout(file, force_disk)
}

fn log_closeout_guard(
    file: &Path,
    stage: agent_doc_flow::types::FlowStage,
    outcome: agent_doc_flow::types::FlowOutcome,
    reason: agent_doc_turn::closeout_guard::CloseoutGuardReason,
) {
    agent_doc_flow_io::closeout::log_closeout_guard_event(file, stage, outcome, reason);
}

/// Minimum byte count for exchange content before the shrink guard triggers.
/// Below this threshold the exchange is too small to be worth protecting.
const SHRINK_GUARD_MIN_BYTES: usize = 100;

/// Maximum ratio (new / old) that the shrink guard allows without `--force`.
/// If the new exchange content is less than this fraction of the old, refuse.
const SHRINK_GUARD_MAX_RATIO: f64 = 0.10;

struct TemplatePatchApplicationBase<'a, 'b> {
    file: &'b Path,
    baseline: Option<&'a str>,
    content_at_start: &'a str,
    patches: &'b [template::PatchBlock],
    unmatched: &'b str,
    mode_overrides: &'b std::collections::HashMap<String, String>,
    source: &'b str,
    strict_closeout: bool,
}

fn template_patch_application_base<'a>(
    input: TemplatePatchApplicationBase<'a, '_>,
) -> Result<std::borrow::Cow<'a, str>> {
    let Some(base) = input.baseline else {
        return Ok(std::borrow::Cow::Borrowed(input.content_at_start));
    };
    if !input.strict_closeout
        || !exchange_append_patch_can_rebase_to_head(
            input.patches,
            input.unmatched,
            input.mode_overrides,
        )
    {
        return Ok(std::borrow::Cow::Borrowed(base));
    }

    let Some(head) = agent_doc_git_io::revision::show_head(input.file)? else {
        return Ok(std::borrow::Cow::Borrowed(base));
    };
    if !is_stale_baseline(base, &head) {
        return Ok(std::borrow::Cow::Borrowed(base));
    }

    eprintln!(
        "[write] explicit baseline is missing committed exchange content — using HEAD as {} patch base",
        input.source
    );
    agent_doc_ops_log_io::log_op(
        input.file,
        &format!(
            "explicit_baseline_rebased_to_head file={} source={} base_len={} head_len={} patches={} unmatched_len={}",
            input.file.display(),
            input.source,
            base.len(),
            head.len(),
            input.patches.len(),
            input.unmatched.trim().len()
        ),
    );
    Ok(std::borrow::Cow::Owned(head))
}

#[allow(clippy::too_many_arguments)]
fn log_exchange_write_diagnostic(
    file: &Path,
    source: &str,
    write_mode: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: &str,
    after: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) {
    let before_exchange = agent_doc_element_exchange::exchange_content(before);
    let after_exchange = agent_doc_element_exchange::exchange_content(after);
    let touches_exchange =
        before_exchange != after_exchange || patch_touches_exchange(patches, unmatched);
    if !touches_exchange {
        return;
    }

    let before_hash = agent_doc_hash::content_hash(before);
    let after_hash = agent_doc_hash::content_hash(after);
    let live_exchange_edited = exchange_has_live_user_edit(baseline, before);
    let prompt_text_duplicated = exchange_prompt_text_duplicated(before, after);
    let before_prefix_count = before_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let after_prefix_count = after_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let normalized_prefix_delta = after_prefix_count.saturating_sub(before_prefix_count);
    let prompt_text_normalized = normalized_prefix_delta > 0;
    let cycle_id = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()
        .map(|state| state.cycle_id)
        .or_else(|| {
            agent_doc_cycle_state_io::load_closeout_projection(file)
                .ok()
                .flatten()
                .and_then(|projection| projection.cycle_id)
        })
        .unwrap_or_else(|| "-".to_string());
    let writer_pid = std::process::id();
    let writer_exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "-".to_string());

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "exchange_write_diagnostic file={} writer_pid={} writer_exe={} source={} write_mode={} patch_id={} cycle_id={} before_hash={} after_hash={} live_exchange_edited={} prompt_text_duplicated={} prompt_text_normalized={} normalized_prefix_delta={} patches={} unmatched_len={}",
            file.display(),
            writer_pid,
            writer_exe,
            source,
            write_mode,
            patch_id.unwrap_or("-"),
            cycle_id,
            before_hash,
            after_hash,
            live_exchange_edited,
            prompt_text_duplicated,
            prompt_text_normalized,
            normalized_prefix_delta,
            patches.len(),
            unmatched.trim().len()
        ),
    );
}

/// Log a write dedup event to both stderr and a persistent file for diagnosis.
fn log_dedup(file: &Path, context: &str) {
    let msg = format!("[write] dedup: {} — {}", file.display(), context);
    eprintln!("{}", msg);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/agent-doc-write-dedup.log")
    {
        let ts = agent_doc_log_time::format_log_timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        let bt = std::backtrace::Backtrace::force_capture();
        let _ = writeln!(f, "[{}] {} backtrace:\n{}", ts, msg, bt);
    }
}

/// Verify the current tmux pane owns the session for this document.
///
/// Returns `Ok(())` when the check passes or cannot be performed (not in tmux,
/// no session ID, session not registered, pane indeterminate). Returns `Err`
/// only when a *different* pane definitively owns the session.
fn verify_pane_ownership(file: &Path) -> Result<()> {
    if !agent_doc_tmux_io::in_tmux() {
        return Ok(());
    }
    let current = match resolve_current_document(file, "verify_pane_ownership") {
        Ok(current) => current,
        Err(_) => return Ok(()),
    };
    let session_id = match frontmatter::parse(current.content()) {
        Ok((fm, _)) => match fm.session {
            Some(s) => s,
            None => return Ok(()),
        },
        Err(_) => return Ok(()),
    };
    let entry = match agent_doc_session_registry_io::lookup_entry(&session_id) {
        Ok(Some(e)) => e,
        _ => return Ok(()),
    };
    let tmux = tmux_router::Tmux::default_server();
    let Some(current) = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux) else {
        return Ok(());
    };
    if entry.pane != current {
        anyhow::bail!(
            "pane ownership mismatch: session {} owned by pane {}, current pane is {}. \
             Use `agent-doc claim` to reclaim.",
            session_id,
            entry.pane,
            current
        );
    }
    Ok(())
}

pub mod run_entry;
pub(crate) use run_entry::*;

#[cfg(test)]
mod ipc;
#[allow(unused_imports)]
#[cfg(test)]
pub(crate) use ipc::*;
// ---------------------------------------------------------------------------
// Internal helpers (same patterns as submit.rs)
// ---------------------------------------------------------------------------

fn capture_undo_checkpoint(path: &Path) -> Result<String> {
    let content_at_start = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    agent_doc_snapshot_io::checkpoint_undo_content(path, &content_at_start)?;
    Ok(content_at_start)
}

/// When the current file already contains the response block, prefer adopting
/// that current content and re-running transcript normalization instead of
/// CRDT-merging the same response a second time.
fn adopt_current_response_without_duplication(
    file: &Path,
    base: &str,
    content_ours: &str,
    content_current: &str,
    snapshot: Option<&str>,
    response: &str,
) -> Result<Option<String>> {
    if !agent_doc_turn::response_replay::response_already_applied(content_current, response)
        && !response_already_in_current(base, content_ours, content_current)
    {
        return Ok(None);
    }

    let mut repaired = content_current.to_string();
    if let Some(snapshot_doc) = snapshot {
        repaired = normalize_user_prompts_in_exchange_safe(&repaired, base, snapshot_doc, file);
    }
    repaired =
        normalize_template_structure_or_fail_preserving(&repaired, file, Some(content_current))?;
    Ok(Some(repaired))
}

fn normalize_final_template_content(
    file: &Path,
    base: &str,
    snapshot: Option<&str>,
    before_current: Option<&str>,
    content: &str,
    response: Option<&str>,
) -> Result<String> {
    let mut normalized = content.to_string();
    let prompt_authority = before_current.unwrap_or(base);
    let prompt_growth = PromptGrowthProvenanceInput::new(base, prompt_authority);
    if let Some(snapshot_doc) = snapshot {
        normalized = normalize_user_prompts_in_exchange_safe(
            &normalized,
            prompt_authority,
            snapshot_doc,
            file,
        );
    }
    if let Some(stripped) = strip_prompt_prefix_from_response_body_first_lines(&normalized) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "flow=document_mutation stage=crdt_post_merge_guard reason=response_body_prompt_prefix_leak file={}",
                file.display()
            ),
        );
        normalized = stripped;
    }
    let preserve_current_or_base = before_current.or(Some(base));
    normalized = normalize_template_structure_or_fail_preserving(
        &normalized,
        file,
        preserve_current_or_base,
    )?;
    if let Some(before) = before_current {
        let (deduped, report) =
            agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
                &normalized,
                file,
                DuplicatePromptRepairOptions::new("final-template")
                    .with_before(Some(before))
                    .preserving(Some(base))
                    .preserving_current(Some(before)),
                agent_doc_ops_log_io::log_op,
                log_duplicate_prompt_residue_guard,
            )?;
        if report.changed() {
            normalized = normalize_template_structure_or_fail_preserving(
                &deduped,
                file,
                preserve_current_or_base,
            )?;
        }
    }
    if let Some(repaired) =
        agent_doc_template_io::repair_response_prompt_order_for_file_with_prompt_growth(
            &normalized,
            response,
            file,
            prompt_growth,
        )?
    {
        normalized = repaired;
        if let Some(snapshot_doc) = snapshot {
            normalized = normalize_user_prompts_in_exchange_safe(
                &normalized,
                prompt_authority,
                snapshot_doc,
                file,
            );
        }
        normalized = normalize_template_structure_or_fail_preserving(
            &normalized,
            file,
            preserve_current_or_base,
        )?;
    }
    if response_precedes_prompt_in_exchange_with_prompt_growth(&normalized, response, prompt_growth)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "response_prompt_order_rejected file={} reason=response_precedes_prompt",
                file.display()
            ),
        );
        anyhow::bail!(
            "response patchback is still positioned before prompt-bearing exchange text; refusing to commit mis-ordered closeout"
        );
    }
    Ok(normalized)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FinalTemplateNormalizationMode {
    Required,
    PreserveAdoptedAuthority,
}

struct FinalTemplateCloseout {
    content: String,
    cleaned_resolved_backlog_prompts: bool,
}

struct FinalTemplateCloseoutRequest<'a> {
    file: &'a Path,
    base: &'a str,
    snapshot: Option<&'a str>,
    before_current: &'a str,
    current_at_response_capture: &'a str,
    content: &'a str,
    response: &'a str,
    mode: FinalTemplateNormalizationMode,
}

fn finalize_template_closeout_content(
    request: FinalTemplateCloseoutRequest<'_>,
) -> Result<FinalTemplateCloseout> {
    if request.mode == FinalTemplateNormalizationMode::PreserveAdoptedAuthority {
        return Ok(FinalTemplateCloseout {
            content: request.content.to_string(),
            cleaned_resolved_backlog_prompts: false,
        });
    }

    let mut content = normalize_final_template_content(
        request.file,
        request.base,
        request.snapshot,
        Some(request.before_current),
        request.content,
        Some(request.response),
    )?;
    let cleaned =
        agent_doc_document::write_normalization::cleanup_resolved_backlog_prompts_after_response(
            request.base,
            request.current_at_response_capture,
            &content,
        )?;
    let cleaned_resolved_backlog_prompts = cleaned.is_some();
    if let Some(cleaned) = cleaned {
        log_resolved_backlog_prompt_cleanup(request.file, cleaned.removed);
        content = normalize_template_structure_or_fail_preserving(
            &cleaned.content,
            request.file,
            Some(request.before_current),
        )?;
    }

    Ok(FinalTemplateCloseout {
        content,
        cleaned_resolved_backlog_prompts,
    })
}

#[cfg(test)]
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    agent_doc_document_realtime_io::atomic_write_through_authority(path, content)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    /// `#writestdinhang`: a non-interactive stdin that never closes must be
    /// bounded, or `read_to_string` waits forever. Two ~15-minute hangs were
    /// observed where fd 0 was a still-open socket owned by the calling harness.
    ///
    /// A TTY is exempt on purpose: a human composing a response is legitimately
    /// silent for minutes, and bounding that would truncate real input.
    #[test]
    fn stdin_read_deadline_bounds_only_non_interactive_input() {
        assert_eq!(
            stdin_read_deadline(true, 60),
            None,
            "a human typing at a terminal must never be cut off"
        );
        assert_eq!(
            stdin_read_deadline(false, 60),
            Some(Duration::from_secs(60)),
            "a pipe/socket that has sent nothing is hung, not slow"
        );
    }

    /// The bound must stay escapable: an operator who genuinely wants to wait
    /// forever on a pipe can opt out, and that opt-out must also apply to a TTY.
    #[test]
    fn stdin_read_deadline_zero_disables_the_bound() {
        assert_eq!(stdin_read_deadline(false, 0), None);
        assert_eq!(stdin_read_deadline(true, 0), None);
    }

    #[test]
    fn retained_response_outcome_keeps_same_closeout_mutation_envelope() {
        let retained = Err(anyhow::anyhow!(
            "canonical target retained; retry_without_disk_write"
        ));
        let semantic_failure = Err(anyhow::anyhow!("malformed component tree"));

        assert!(write_outcome_retains_closeout_mutations(&Ok(())));
        assert!(write_outcome_retains_closeout_mutations(&retained));
        assert!(!write_outcome_retains_closeout_mutations(&semantic_failure));
    }

    #[test]
    fn pre_capture_guard_only_bypasses_for_the_matching_retained_capture() {
        let current_response = "### Re: Disclosure requirement\n\nCurrent accepted answer.\n";
        let current_target =
            format!("<!-- agent:exchange -->\n{current_response}<!-- /agent:exchange -->\n");
        let historical_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: Disclosure requirement\n\nAn older, superseded answer.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(retained_target_contains_capture_response(
            current_response,
            &current_target,
        ));
        assert!(
            !retained_target_contains_capture_response(current_response, historical_target),
            "an unrelated historical retained effect must not authorize a fresh capture retry",
        );
        assert!(!historical_retained_write_guard_may_bypass(
            true,
            false,
            WriteCloseoutOwnerRole::ForegroundFinalize,
        ));
        assert!(historical_retained_write_guard_may_bypass(
            true,
            false,
            WriteCloseoutOwnerRole::CapturedFinalizeResume,
        ));
        assert_eq!(
            WriteCloseoutOwnerRole::from_origin(Some("captured_finalize_resume")),
            WriteCloseoutOwnerRole::CapturedFinalizeResume,
        );
    }

    #[test]
    fn actionable_mutation_record_unifies_adds_and_ungates_for_queue_closeout() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        record_pending_actionable_mutations(
            &doc,
            &["added".to_string()],
            &[
                "#Ungated".to_string(),
                "ungated".to_string(),
                "phase2".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(
            agent_doc_cycle_state_io::pending_actionable_ids(&doc),
            std::collections::HashSet::from([
                "added".to_string(),
                "ungated".to_string(),
                "phase2".to_string(),
            ])
        );
    }

    #[allow(clippy::too_many_arguments)]
    struct NoopRepairReplayWriteEffects;

    static NOOP_REPAIR_REPLAY_WRITE_EFFECTS: NoopRepairReplayWriteEffects =
        NoopRepairReplayWriteEffects;

    impl agent_doc_repair_io::RepairStrictReplayWriteEffects for NoopRepairReplayWriteEffects {
        fn run_strict_write_replay(
            &self,
            _file: &Path,
            _response: &str,
            _is_template: bool,
            _is_stream: bool,
            _force_disk: bool,
            _queue_completion_ids: &[String],
        ) -> Result<()> {
            anyhow::bail!("unexpected strict replay in write runtime test")
        }
    }

    impl agent_doc_repair_io::RepairFallbackWriteEffects for NoopRepairReplayWriteEffects {
        fn apply_template_from_string(
            &self,
            _file: &Path,
            _response: &str,
            _force_disk: bool,
        ) -> Result<()> {
            anyhow::bail!("unexpected template replay in write runtime test")
        }

        fn apply_append_from_string(&self, _file: &Path, _response: &str) -> Result<()> {
            anyhow::bail!("unexpected append replay in write runtime test")
        }
    }

    impl agent_doc_repair_io::RepairRecoveredQueueHeadEffects for NoopRepairReplayWriteEffects {
        fn strike_recovered_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn active_capture_response_body_for_write_uses_projection_without_capture_sidecar() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        let base = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nDone.\n";
        fs::write(&doc, base).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        assert!(!capture.capture_id.is_empty());

        assert_eq!(
            active_capture_response_body_for_write(&doc).unwrap(),
            response
        );
    }

    /// #pcp2: a document disk write records write-provenance, but `.agent-doc/`
    /// sidecar/snapshot writes do not (provenance is only meaningful for the
    /// editor-visible document).
    #[test]
    fn atomic_write_records_provenance_for_document_not_sidecar() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();

        let doc = tmp.path().join("prov-doc.md");
        atomic_write(&doc, "hello document").unwrap();
        let prov = agent_doc_cycle_state_io::load_document_disk_write(&doc)
            .unwrap()
            .expect("document write should record provenance");
        assert_eq!(prov.content_len, "hello document".len() as u64);
        assert_eq!(
            prov.content_hash,
            agent_doc_hash::content_hash("hello document")
        );
        assert_eq!(prov.actor, "agent");
        assert!(!prov.write_id.is_empty());

        // A write under .agent-doc/ (sidecar/snapshot) must NOT record provenance.
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "snapshot bytes").unwrap();
        assert!(
            agent_doc_cycle_state_io::load_document_disk_write(&sidecar)
                .unwrap()
                .is_none(),
            "an .agent-doc/ sidecar write must not record document provenance"
        );
    }

    #[test]
    fn status_set_writes_detached_disk_without_listener() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();

        set_status_with_options(&doc, "new status", false).unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            on_disk.contains("new status"),
            "status should be written through detached disk when no editor owns the doc: {on_disk}"
        );
        assert!(
            !on_disk.contains("old status"),
            "old status should be replaced: {on_disk}"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("status_set_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "detached status write should be attributable:\n{log}"
        );
    }

    #[test]
    fn force_disk_status_set_writes_without_listener() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();

        set_status_with_options(&doc, "new status", true)
            .expect("force-disk status update should write without listener");

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("old status"));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("status_set_writeback")
                && log.contains("transport=disk_force")
                && log.contains("reason=force_disk"),
            "force-disk status write should be attributable:\n{log}"
        );
    }

    #[test]
    fn fence_count_drop_is_logged_for_visible_document_write() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("fence-doc.md");
        let fenced = "intro\n```js\nconst x = 1;\n```\ntail\n";
        atomic_write(&doc, fenced).unwrap();
        // First write has no prior file, so no drop log expected.
        let log_path = tmp.path().join(".agent-doc").join("logs").join("ops.log");
        let log_before = fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            !log_before.contains("fence_count_dropped"),
            "first write (no prior file) must not log a fence drop"
        );
        // Second write removes the fence — must log the drop.
        fs::write(&log_path, "").unwrap();
        atomic_write(&doc, "intro\nconst x = 1;\ntail\n").unwrap();
        let log_after = fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            log_after.contains("fence_count_dropped") && log_after.contains("old_fences=2"),
            "a write that drops a fence must log fence_count_dropped; got: {}",
            log_after
        );
    }

    /// `#codefence-strip`: the fence-opening counter recognizes both backtick
    /// and tilde fences, ignores longer backtick runs that are not fence
    /// openings (e.g. inline ``````), and treats indented fences as openings.
    #[test]
    fn count_code_fence_openings_handles_backtick_and_tilde() {
        assert_eq!(count_code_fence_openings("```\ncode\n```\n"), 2);
        assert_eq!(count_code_fence_openings("~~~\ncode\n~~~\n"), 2);
        assert_eq!(
            count_code_fence_openings("  ```js\nconst x = 1;\n  ```\n"),
            2
        );
        assert_eq!(count_code_fence_openings("no fences here"), 0);
        assert_eq!(count_code_fence_openings("```python\nprint('hi')\n```"), 2);
        assert_eq!(
            count_code_fence_openings("``````\nnot a fence open by CommonMark\n``````\n"),
            0
        );
    }

    /// 08b end state: a routed visible-document write still records write
    /// provenance, because the queue job runs `atomic_write` on the owner thread
    /// where the owner-scope guard takes the raw path (`atomic_write_raw`), and
    /// that raw path is what records provenance.
    #[test]
    fn write_authority_routed_write_records_provenance() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        assert!(
            agent_doc_cycle_state_io::load_document_disk_write(&doc)
                .unwrap()
                .is_some(),
            "the routed write's inner raw path records provenance"
        );
    }
    /// 08b end state: every editor-visible document write is routed through the
    /// session actor's ordered write queue (no flag). The routed write executes
    /// `atomic_write` again on the owner thread; the owner-scope re-entrancy
    /// guard keeps that inner write on the raw path, so this must not deadlock
    /// and the content must land.
    #[test]
    fn write_authority_visible_write_routes_through_queue_without_deadlock() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc2.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        let ops = fs::read_to_string(tmp.path().join(".agent-doc").join("logs").join("ops.log"))
            .unwrap_or_default();
        assert!(
            ops.contains("write_authority action=routed transport=write_queue"),
            "a visible-document write must report the routed decision to ops.log: {ops:?}"
        );
    }
    /// `.agent-doc/` sidecar writes are never routed — they always take the raw
    /// path directly.
    #[test]
    fn write_authority_never_routes_agent_doc_sidecars() {
        let tmp = TempDir::new().unwrap();
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "sidecar bytes").unwrap();
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "sidecar bytes");
    }
    #[test]
    fn write_appends_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Simulate stdin by calling run logic directly
        let base = fs::read_to_string(&doc).unwrap();
        let response = "This is the assistant response.";

        let mut content_ours = base.clone();
        if !content_ours.ends_with('\n') {
            content_ours.push('\n');
        }
        content_ours.push_str("## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push('\n');
        content_ours.push_str("\n## User\n\n");

        atomic_write(&doc, &content_ours).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("## Assistant\n\nThis is the assistant response."));
        assert!(result.contains("\n\n## User\n\n"));
        assert!(result.contains("## User\n\nHello"));
    }
    #[test]
    fn write_updates_snapshot() {
        // Use a direct snapshot write/read to avoid CWD dependency.
        // The snapshot module uses relative paths (.agent-doc/snapshots/),
        // so we verify the pattern works via agent_doc_fs::snapshot_path_for + direct I/O.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();

        // Verify snapshot path computation works
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        assert!(
            snap_path
                .to_string_lossy()
                .contains(".agent-doc/snapshots/")
        );

        // Verify atomic_write + read roundtrip (the core of snapshot save)
        let snap_abs = dir.path().join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        let loaded = fs::read_to_string(&snap_abs).unwrap();
        assert_eq!(loaded, content);
    }
    #[test]
    fn force_disk_closeout_queue_consume_bypasses_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do the fix — opus-4-8\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the fix\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &source,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so the
        // non-force consume fails closed (protects the buffer) rather than taking
        // an unproven editor-delivery disk fallback.
        agent_doc_test_support::seed_lazily_editor_registration_default(doc.to_str().unwrap());

        let err = consume_queue_prompts_for_done_ids_closeout(&doc, &[], false).unwrap_err();
        let err = format!("{err:?}");
        assert!(
            err.contains("editor is the current authority")
                || err.contains("failed to resolve editor authority")
                || err.contains("refused direct disk write")
                || err.contains("no editor replica was registered"),
            "non-force queue consume must fail closed with an active listener: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "non-force closeout must not write behind an active listener"
        );

        let outcome = consume_queue_prompts_for_done_ids_closeout(&doc, &[], true)
            .expect("force-disk closeout queue consume should write directly")
            .expect("force-disk closeout should consume the answered head");

        assert_eq!(outcome.consumed_count, 1);
        let result = fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("queue: stop") && !result.contains("queue_active: true"),
            "drained queue consume should clear the active queue flag:\n{result}"
        );
        assert!(
            !result.contains("- do the fix"),
            "force-disk recovery must remove the answered queue head:\n{result}"
        );
        assert!(
            result.contains("> do the fix"),
            "force-disk recovery must retain the answered prompt in the response quote:\n{result}"
        );
    }

    #[test]
    fn force_disk_closeout_pending_maintenance_bypasses_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &source,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        agent_doc_test_support::seed_lazily_editor_registration_default(doc.to_str().unwrap());

        let err = agent_doc_session_check_io::run_closeout_pending_maintenance(
            &doc,
            true,
            false,
            |file, force_disk| {
                if force_disk {
                    agent_doc_preflight_io::run_pending_maintenance_force_disk(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                } else {
                    agent_doc_preflight_io::run_pending_maintenance(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                }
            },
        )
        .unwrap_err();
        let err = format!("{err:?}");
        assert!(
            (err.contains("editor is the current authority")
                || err.contains("failed to resolve editor authority")
                || err.contains("no editor replica was registered"))
                && (err.contains("disk is a non-authoritative replica")
                    || err.contains("disk remained non-authoritative")
                    || err.contains("disk is not consulted")
                    || err.contains("disk was not written")),
            "non-force closeout pending maintenance must protect the active listener: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "non-force closeout must not write behind an active listener"
        );

        agent_doc_session_check_io::run_closeout_pending_maintenance(
            &doc,
            true,
            true,
            |file, force_disk| {
                if force_disk {
                    agent_doc_preflight_io::run_pending_maintenance_force_disk(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                } else {
                    agent_doc_preflight_io::run_pending_maintenance(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                }
            },
        )
        .expect("force-disk closeout pending maintenance should write directly");

        let result = fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .unwrap()
            .content(&result)
            .to_string();
        assert!(
            !backlog_after.contains("[#done1]"),
            "force-disk maintenance must reap the completed item:\n{result}"
        );
        assert!(
            result.contains("[#keep1]"),
            "force-disk maintenance must retain unrelated active items:\n{result}"
        );
        assert!(
            result.contains("## Completed / Reaped") && result.contains("[#done1] Reap me"),
            "force-disk maintenance must archive the reaped item:\n{result}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let reason_marker = ["reason", "force_disk"].join("=");
        assert!(
            log.contains("pending_maintenance_writeback")
                && log.contains("transport=disk_force")
                && log.contains(&reason_marker),
            "force-disk maintenance should leave an attributable transport log:\n{log}"
        );
    }

    #[test]
    fn closeout_pending_maintenance_skips_status_only_housekeeping_with_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Ready. Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#new] Keep me\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &source,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&source), Some(&source)).unwrap();
        agent_doc_capture_io::capture_response(&doc, "Done.").unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        agent_doc_test_support::seed_lazily_editor_registration_default(doc.to_str().unwrap());

        agent_doc_session_check_io::run_closeout_pending_maintenance(
            &doc,
            true,
            false,
            |file, force_disk| {
                if force_disk {
                    agent_doc_preflight_io::run_pending_maintenance_force_disk(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                } else {
                    agent_doc_preflight_io::run_pending_maintenance(
                        file,
                        &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                    )
                    .map(|_| ())
                }
            },
        )
        .expect("status-only housekeeping should not block response closeout");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "closeout skip must leave visible operator-owned document text untouched"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_pending_maintenance_skipped")
                && log.contains("basis=no_tracked_work_closeout"),
            "skip should leave an attributable ops-log entry:\n{log}"
        );
    }

    #[test]
    fn capture_undo_checkpoint_records_current_content() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "current\n").unwrap();

        let captured_content = capture_undo_checkpoint(&doc).unwrap();

        assert_eq!(captured_content, "current\n");
        assert_eq!(
            agent_doc_snapshot_io::load_undo_content(&doc)
                .unwrap()
                .unwrap(),
            "current\n"
        );
    }
    #[test]
    fn guard_rejects_normal_write_when_diff_requests_compact_exchange() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n\n",
            "compact exchange\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = guard_no_exchange_compaction_request_between(&doc, Some(baseline), current)
            .expect_err("ordinary response write should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("compact exchange"));
        assert!(msg.contains("agent-doc compact"));
    }
    #[test]
    fn write_preserves_user_edits_via_merge() {
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        let response = "My response";

        // "ours" = base + response
        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // "theirs" = user added a follow-up to the User block
        let theirs = "---\nsession: test\n---\n\n## User\n\nOriginal question\nAnd a follow-up!\n";

        let merged = agent_doc_merge_io::merge_contents(base, &ours, theirs).unwrap();

        // Both the response and the user's follow-up should be in the merge
        assert!(
            merged.contains("My response"),
            "response missing from merge"
        );
        assert!(
            merged.contains("And a follow-up!"),
            "user edit missing from merge"
        );
    }
    #[test]
    fn write_no_merge_when_unchanged() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "Response here";

        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // theirs == base (no edit)
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, base).unwrap();

        let content_current = fs::read_to_string(&doc).unwrap();

        let final_content = if content_current == base {
            ours.clone()
        } else {
            agent_doc_merge_io::merge_contents(base, &ours, &content_current).unwrap()
        };

        assert_eq!(final_content, ours);
    }
    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }
    #[test]
    fn concurrent_writes_no_corruption() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&p).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = fs::read_to_string(&path).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }
    #[test]
    fn snapshot_matches_disk_state() {
        // Snapshot saved after write must equal the actual post-merge file on disk.
        // Using content_ours (pre-merge) as the snapshot risks phantom diffs when
        // the baseline is stale (e.g. streaming checkpoint with an outdated baseline).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc").join("snapshots");
        fs::create_dir_all(&agent_doc_dir).unwrap();

        let doc = dir.path().join("test.md");
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        fs::write(&doc, base).unwrap();

        // Build content_ours = baseline + response
        let response = "Agent response here";
        let mut content_ours = base.to_string();
        content_ours.push_str("\n## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push_str("\n\n## User\n\n");

        // Simulate user editing the file concurrently (adding a follow-up)
        let user_edited = format!("{}Follow-up question\n", base);
        fs::write(&doc, &user_edited).unwrap();

        // Merge: content_ours + user edits
        let merged = agent_doc_merge_io::merge_contents(base, &content_ours, &user_edited).unwrap();

        // Write merged content (includes both response and user edit)
        atomic_write(&doc, &merged).unwrap();
        assert!(merged.contains(response), "response missing from merged");
        assert!(
            merged.contains("Follow-up question"),
            "user edit missing from merged"
        );

        // KEY: Save snapshot as final_content (the actual disk state after merge)
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &merged,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Verify: snapshot matches what's on disk exactly
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let current = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            snap, current,
            "snapshot must match actual disk state after write"
        );
        assert!(
            snap.contains(response),
            "snapshot should contain agent response"
        );
        assert!(
            snap.contains("Follow-up question"),
            "snapshot should contain merged user edit"
        );
    }

    #[test]
    fn try_ipc_full_content_returns_false_when_disabled() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let result = agent_doc_write_converge_io::try_ipc_full_content(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            "new content",
        )
        .unwrap();
        assert!(
            !result,
            "full-content IPC is disabled and should return false"
        );
    }
    #[test]
    fn apply_patches_sanitize_unmatched_prevents_duplicate_exchange_block() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Existing answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        let unmatched = "### Re: deploy — gpt-5\n\nDeployed.\n\n<!-- agent:exchange -->\nLeaked content\n<!-- /agent:exchange -->\n";
        let mut sanitized_unmatched = unmatched.to_string();
        agent_doc_template::sanitize::sanitize_unmatched(&mut sanitized_unmatched);

        let result =
            agent_doc_template_io::apply_patches(doc, &[], &sanitized_unmatched, &file).unwrap();

        let exchange_opens = result.matches("<!-- agent:exchange").count();
        assert_eq!(
            exchange_opens, 1,
            "must have exactly one exchange opener, got {exchange_opens}:\n{result}"
        );
        assert!(
            !result.contains("<!-- agent:exchange -->\nLeaked content"),
            "leaked exchange markers must be escaped, not create a second block"
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers should appear in result"
        );
    }

    #[test]
    fn ipc_json_preserves_utf8_em_dash() {
        // Verify that serde_json serialization preserves em dashes in IPC payloads
        let content = "Response with \u{2014} em dash.";
        let payload = serde_json::json!({
            "file": "/tmp/test.md",
            "patches": [{
                "component": "exchange",
                "content": content,
            }],
            "unmatched": "",
            "baseline": "",
        });

        let json_str = serde_json::to_string_pretty(&payload).unwrap();
        // Parse it back and verify the content is preserved
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let parsed_content = parsed["patches"][0]["content"].as_str().unwrap();
        assert_eq!(
            parsed_content, content,
            "em dash must survive JSON round-trip"
        );

        // Also verify the raw JSON contains the UTF-8 bytes, not escaped sequences
        assert!(
            json_str.contains("\u{2014}"),
            "JSON should contain raw UTF-8 em dash"
        );
    }
    #[test]
    fn build_ipc_node_patches_json_tracks_strike_and_insert_by_node_key() {
        let before = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- ~~do [#beta]~~
- do [#gamma]
<!-- /agent:queue -->
";

        let patches =
            agent_doc_ipc_protocol::build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "strike"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- ~~do [#beta]~~"))
                && patch["expected_content"].as_str() == Some("- do [#beta]\n")
                && patch["expected_content_hash"].as_str().is_some()
        }));
        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:gamma:0"
                && patch["op"] == "insert"
                && patch["after"] == "queue:0:beta:0"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- do [#gamma]"))
        }));
    }
    #[test]
    fn build_ipc_node_patches_json_tracks_reorder_without_text_matching() {
        let before = "\
<!-- agent:queue priority go -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- do [#beta]
- do [#alpha]
<!-- /agent:queue -->
";

        let patches =
            agent_doc_ipc_protocol::build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "move"
                && patch["before"] == "queue:0:alpha:0"
        }));
    }
    #[test]
    fn stale_baseline_guard_prefix_check() {
        // Baseline that starts with snapshot content (user added text) = NOT stale
        let snapshot = "## Exchange\nResponse here.\n";
        let baseline_with_user_edit = "## Exchange\nResponse here.\nNew user question\n";
        let snap_clean = strip_boundary_for_dedup(snapshot);
        let base_clean = strip_boundary_for_dedup(baseline_with_user_edit);
        assert!(
            base_clean.starts_with(&snap_clean),
            "baseline with user edits should start with snapshot content"
        );

        // Baseline that doesn't contain snapshot content = STALE
        let stale_baseline = "## Exchange\nOld content only.\n";
        let stale_clean = strip_boundary_for_dedup(stale_baseline);
        assert!(
            !stale_clean.starts_with(&snap_clean),
            "stale baseline should not start with snapshot content"
        );
    }
    #[test]
    fn strip_boundary_for_dedup_removes_markers() {
        let with_boundary = "Hello\n<!-- agent:boundary:abc123 -->\nWorld\n";
        let without = strip_boundary_for_dedup(with_boundary);
        assert!(!without.contains("agent:boundary"));
        assert!(without.contains("Hello"));
        assert!(without.contains("World"));
    }
    #[test]
    fn effective_unmatched_cleared_when_synthesis_consumes_content() {
        // When synthesis consumes the unmatched content (patches input was empty,
        // ipc_patches output is non-empty), effective_unmatched should be "".
        // This prevents the plugin from applying the content twice (IPC duplicate bug).
        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let unmatched = "some response content";

        // Case 1: synthesis happened (patches empty → ipc_patches non-empty)
        let ipc_patches: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": unmatched,
        })];
        let effective = if patches.is_empty() && !ipc_patches.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective, "",
            "effective_unmatched must be empty when synthesis consumed content"
        );

        // Case 2: explicit patches (no synthesis) — unmatched passes through
        let explicit_patch = agent_doc_template::PatchBlock::new("exchange", "response");
        let patches_explicit = [explicit_patch];
        let ipc_explicit: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": "response",
        })];
        let effective2 = if patches_explicit.is_empty() && !ipc_explicit.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective2,
            unmatched.trim(),
            "effective_unmatched should pass through when explicit patches exist"
        );

        // Case 3: no patches, no synthesis (empty doc or dedup skipped it) — unmatched passes through
        let ipc_empty: Vec<serde_json::Value> = vec![];
        let effective3 = if patches.is_empty() && !ipc_empty.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective3,
            unmatched.trim(),
            "effective_unmatched should pass through when no synthesis occurred"
        );
    }
    // ── normalize_user_prompts_in_exchange ──────────────────────────────────
    #[test]
    fn exchange_write_diagnostic_logs_live_edit_provenance() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();
        let patches = vec![template::PatchBlock::new(
            "exchange",
            "### Re: response\nDone.\n",
        )];

        log_exchange_write_diagnostic(
            &doc,
            "test_source",
            "test_mode",
            Some("patch-123"),
            Some(baseline),
            before,
            after,
            &patches,
            "",
        );

        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("exchange_write_diagnostic"));
        assert!(log.contains("source=test_source"));
        assert!(log.contains("write_mode=test_mode"));
        assert!(log.contains("patch_id=patch-123"));
        assert!(log.contains("live_exchange_edited=true"));
        assert!(log.contains("prompt_text_duplicated=true"));
        assert!(log.contains("prompt_text_normalized=true"));
        assert!(log.contains("normalized_prefix_delta=1"));
        assert!(log.contains("before_hash="));
        assert!(log.contains("after_hash="));
        assert!(log.contains("writer_pid="));
    }
    #[test]
    fn prompt_dedupe_skips_assistant_response_quotes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:old -->\n",
            "quote this exact line\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ quote this exact line\n",
            "### Re: response — gpt-5\n\n",
            "quote this exact line\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
            &doc,
            Some(before),
            after,
            "test_ipc",
        )
        .unwrap();

        assert!(
            !changed,
            "assistant response quotes must not be treated as duplicate prompt text"
        );
        assert_eq!(repaired.matches("quote this exact line").count(), 2);
    }
    #[test]
    fn normalize_template_structure_repairs_live_prompt_prefix_variant_after_boundary() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
    }
    #[test]
    fn normalize_template_structure_rejects_mixed_duplicate_scaffold_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }
    #[test]
    fn normalize_template_structure_keeps_prefix_variant_inside_response_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: example — gpt-5\n\n",
            "This sentence is a prefix\n",
            "This sentence is a prefix variant in assistant prose\n",
            "<!-- agent:boundary:613974fd -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(repaired.contains("This sentence is a prefix\n"));
        assert!(repaired.contains("This sentence is a prefix variant in assistant prose\n"));
    }
    #[test]
    fn ipc_snapshot_dedupes_live_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n"
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
            &doc,
            Some(before),
            after,
            "test_ipc",
        )
        .unwrap();

        assert!(changed);
        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("live_prompt_prefix_variant_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }
    #[test]
    fn ipc_snapshot_scrubs_post_exchange_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
            &doc,
            Some(before),
            &after,
            "test_ipc",
        )
        .unwrap();

        assert!(changed);
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "IPC visible-write dedupe must scrub duplicate post-exchange prompt text:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "IPC visible-write dedupe must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("post_exchange_duplicate_prompt_comment_removed"));
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("post_exchange_comments=true"));
    }
    #[test]
    fn ipc_snapshot_preserves_owned_post_exchange_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The post-exchange IPC handoff scratch comment should not be deleted. #spec-test-build-install-commit-push";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Keep this owned scratch note visible.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let after = before.replace(
        "<!-- /agent:exchange -->",
        "### Re: IPC handoff — gpt-5\n\nHandled.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
    );
        fs::write(&doc, &before).unwrap();

        let (repaired, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
            &doc,
            Some(&before),
            &after,
            "test_ipc",
        )
        .unwrap();

        assert!(
            !changed,
            "owned post-exchange comments should not force IPC snapshot repair"
        );
        assert!(
        repaired.contains(&format!(
            "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep this owned scratch note visible.\n-->"
        )),
        "IPC visible-write dedupe must preserve owned mixed scratch comments:\n{repaired}"
    );
    }
    #[test]
    fn ipc_snapshot_rejects_plain_markdown_duplicate_prompt_residue() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: response — gpt-5\n\n",
                "Done.\n",
                "<!-- agent:boundary:new -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &before).unwrap();

        let err = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
            &doc,
            Some(&before),
            &after,
            "test_ipc",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "IPC snapshot dedupe must fail closed on duplicate prompt Markdown residue: {err}"
        );
    }
    #[test]
    fn normalize_patch_content_applies_prefix_to_matching_lines() {
        let patch_content =
            "transferred line 1\ntransferred line 2\n### Re: Response\nAgent answer\n";
        let prefix_lines = vec![
            "transferred line 1".to_string(),
            "transferred line 2".to_string(),
        ];
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
        let expected =
            "❯ transferred line 1\n❯ transferred line 2\n### Re: Response\nAgent answer\n";
        assert_eq!(
            result, expected,
            "prefix lines should get ❯  in patch content"
        );
    }
    #[test]
    fn normalize_patch_content_idempotent_already_prefixed() {
        let patch_content = "❯ already prefixed\nnot prefixed\n";
        let prefix_lines = vec!["already prefixed".to_string(), "not prefixed".to_string()];
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
        let expected = "❯ already prefixed\n❯ not prefixed\n";
        assert_eq!(
            result, expected,
            "already-prefixed lines should not get double prefix"
        );
    }
    #[test]
    fn normalize_patch_content_empty_prefix_lines_passthrough() {
        let patch_content = "some line\nanother line\n";
        let result =
            agent_doc_document_realtime::write_policy::normalize_patch_content(patch_content, &[]);
        assert_eq!(
            result, patch_content,
            "empty prefix_lines should leave content unchanged"
        );
    }
    #[test]
    fn normalize_patch_content_non_matching_lines_unchanged() {
        let patch_content = "agent response line\n### heading\n";
        let prefix_lines = vec!["user line".to_string()];
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
        assert_eq!(
            result, patch_content,
            "non-matching lines should pass through unchanged"
        );
    }
    #[test]
    fn normalize_patch_content_counts_duplicate_targets() {
        let patch_content = "spec-test-build-install-commit-push\nspec-test-build-install-commit-push\nspec-test-build-install-commit-push\n";
        let prefix_lines = vec![
            "spec-test-build-install-commit-push".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );

        assert_eq!(
            result,
            "❯ spec-test-build-install-commit-push\n❯ spec-test-build-install-commit-push\nspec-test-build-install-commit-push\n"
        );
    }
    #[test]
    fn normalize_prefix_lines_skipped_for_replace_mode_components() {
        // Regression: normalize_patch_content was applied to ALL patches including agent:pending.
        // When a line from the exchange user_added set also appeared in a pending patch, it would
        // incorrectly receive the ❯  prefix. The fix gates normalization on is_append_mode_component.
        let pending_content =
            "- [ ] Build Gutenberg replacement HTML for home page\n- [ ] Update page content\n";
        let prefix_lines = vec!["- [ ] Build Gutenberg replacement HTML for home page".to_string()];
        // Simulate the guard: only apply normalize_patch_content for exchange (append-mode) components.
        // For pending (replace-mode), content must pass through unchanged.
        let is_pending = !agent_doc_template::stale_baseline::is_append_mode_component("pending");
        assert!(is_pending, "pending should not be an append-mode component");
        // If the guard is respected, pending content is not normalized.
        let result = if agent_doc_template::stale_baseline::is_append_mode_component("pending") {
            agent_doc_document_realtime::write_policy::normalize_patch_content(
                pending_content,
                &prefix_lines,
            )
        } else {
            pending_content.to_string()
        };
        assert_eq!(
            result, pending_content,
            "agent:pending content must NOT receive ❯  prefix"
        );
        assert!(
            !result.contains("❯ "),
            "no ❯  prefix should appear in pending patches"
        );
    }

    #[test]
    fn empty_response_recovery_merges_missing_committed_head_response() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n\n",
            "### Re: latest committed — gpt-5\n\n",
            "Latest body.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git(&["add", "doc.md"]);
        git(&["commit", "-m", "commit response"]);

        let visible = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "restart/recycle your supervisor\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n",
            "<!-- agent:boundary:working -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, visible).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            visible,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert!(
            agent_doc_repair_runtime_io::recover_missing_committed_head_response(&doc).unwrap(),
            "missing committed response should be recovered"
        );

        let recovered = fs::read_to_string(&doc).unwrap();
        assert!(
            recovered.contains("restart/recycle your supervisor"),
            "operator/current status edit must be preserved:\n{recovered}"
        );
        assert!(
            recovered.contains("### Re: latest committed — gpt-5")
                && recovered.contains("Latest body."),
            "latest committed response must be merged back into the visible file:\n{recovered}"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, recovered);

        let head_after = std::process::Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:doc.md"])
            .output()
            .unwrap();
        assert!(head_after.status.success());
        let head_after = String::from_utf8(head_after.stdout).unwrap();
        assert_eq!(
            head_after, recovered,
            "recovery should commit the merged visible document"
        );
    }

    #[test]
    fn strict_empty_response_recovery_continues_after_stale_preflight_repair() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n\n",
            "### Re: latest committed — gpt-5\n\n",
            "Latest body.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git(&["add", "doc.md"]);
        git(&["commit", "-m", "commit response"]);

        let visible = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "restart/recycle your supervisor\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n",
            "<!-- agent:boundary:working -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, visible).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            visible,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(visible), Some(visible)).unwrap();

        assert!(
            agent_doc_repair_io::recover_empty_response_for_strict_closeout(
                agent_doc_repair_runtime_io::repair_coordinator_effects(
                    &NOOP_REPAIR_REPLAY_WRITE_EFFECTS
                ),
                &doc,
                true,
                false,
                Some(false),
            )
            .unwrap(),
            "strict empty response recovery should continue past stale preflight repair"
        );

        let recovered = fs::read_to_string(&doc).unwrap();
        assert!(
            recovered.contains("restart/recycle your supervisor")
                && recovered.contains("### Re: latest committed — gpt-5"),
            "strict recovery must preserve current edits and restore committed response:\n{recovered}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(head_after, recovered);
    }

    // ── agent-response-block tracking ────────────────────────────────────────
    // ── safety rail: normalize_user_prompts_in_exchange_safe ────────────────
    // --- exchange shrink guard tests ---
    #[test]
    fn splice_pending_replaces_content_when_both_have_pending() {
        // target has stale/empty pending (built from pre-mutation baseline)
        let target = "\
<!-- agent:exchange -->
response content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#aaaa] old item
<!-- /agent:pending -->
";
        // source is the current on-disk file with a pending-done mutation applied
        let source = "\
<!-- agent:exchange -->
original content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#aaaa] old item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source).content;
        // exchange content from target is preserved
        assert!(
            result.contains("response content"),
            "exchange content should come from target"
        );
        // pending content from source (with [x]) is used
        assert!(
            result.contains("- [x] [#aaaa] old item"),
            "pending done state should come from source"
        );
        // old pending from target is gone
        assert!(
            !result.contains("- [ ] [#aaaa] old item"),
            "stale open pending should be replaced"
        );
    }
    #[test]
    fn splice_pending_noop_when_source_has_no_pending() {
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#bbbb] task
<!-- /agent:pending -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
";
        let result = splice_pending_component(target, source).content;
        assert_eq!(
            result, target,
            "target should be returned unchanged when source has no pending"
        );
    }
    #[test]
    fn splice_pending_warns_when_target_missing_pending() {
        // target has no pending component; source does — should return target unchanged
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#cccc] done item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source).content;
        assert_eq!(
            result, target,
            "target should be returned unchanged when target has no pending"
        );
    }
}
