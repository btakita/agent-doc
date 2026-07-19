//! Extracted from `write.rs` (large-module split). See parent module for context.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fmt::Display;
use std::fs::OpenOptions;
use std::path::Path;

use crate::queue_consumption_proof::{
    QueueConsumptionProofEffects, QueueConsumptionProofStage,
    record_queue_consumption_proofs as record_queue_consumption_proofs_with_effects,
};
use agent_doc_document::queue_projection::strip_priority_markers;
use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_queue::{
    queue_consume::{
        IpcNodeOp, QueueConsumptionPlan, annotate_newly_struck_free_text_heads,
        answered_free_text_head_node_keys, consume_queue_nodes_by_key, first_n_queue_prompt_texts,
        head_id_names_open_backlog_item, id_backed_head_node_keys,
        mark_entries_completed_by_done_ids, normalized_done_id_bag,
        queue_consume_count_for_done_ids, queue_consume_node_ops, queue_prompt_node_keys_for_count,
        queue_prompt_node_keys_for_done_ids, strike_all_noise_queue_heads,
    },
    queue_response::{
        embed_consumed_prompt_in_response, first_nonempty_line, queue_head_is_free_text_prompt,
        queue_prompt_text_is_free_text,
    },
};

pub trait QueueConsumeWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        self.current_document_content(file, source)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()>;
}

fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = agent_doc_fs::state_lock_path_for(path)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open doc lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire doc lock on {}", lock_path.display()))?;
    Ok(file)
}

fn log_snapshot_recovery_warning(file: &Path, context: &str, detail: impl Display) {
    eprintln!("[queue] snapshot recovery warning during {context}: {detail}");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "snapshot_recovery_warning file={} context={} detail={}",
            file.display(),
            context,
            detail
        ),
    );
}

fn load_snapshot_recovery_only(file: &Path, context: &str) -> Option<String> {
    match agent_doc_snapshot_io::load_document_baseline(file) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            log_snapshot_recovery_warning(file, context, err);
            None
        }
    }
}

fn save_snapshot_recovery_only(file: &Path, content: &str, context: &str) {
    if let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        content,
        agent_doc_ops_log_io::log_op,
    ) {
        log_snapshot_recovery_warning(file, context, err);
    }
}

#[cfg(test)]
use agent_doc_queue::{
    queue_consume::{
        cycle_answered_foreign_exchange_prompt, queue_consumption_allowed_for_response,
        should_consume_queue_prompt_for_write,
    },
    queue_response::queue_head_is_bare_do_directive,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueConsumptionOutcome {
    pub consumed_text: String,
    pub consumed_count: usize,
    pub node_ops: Vec<IpcNodeOp>,
    pub remaining: usize,
    pub drained: bool,
    pub auto: bool,
}

#[allow(dead_code)]
pub fn consume_queue_prompt(file: &Path, effects: &dyn QueueConsumeWriteEffects) -> Result<bool> {
    Ok(consume_queue_prompt_with_outcome(file, effects)?.is_some())
}

pub fn consume_queue_prompt_with_outcome(
    file: &Path,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, &[], 1, false, None, effects)
}

/// Consume up to `count` leading free-text prompts in one revision-pinned
/// read/plan/write transaction. This is deliberately not implemented as a loop
/// of one-head writes: an editor ACK may be asynchronous, and re-resolving
/// authority between iterations can otherwise select the same logical head
/// repeatedly.
pub fn consume_free_text_queue_prompts_with_outcome(
    file: &Path,
    count: usize,
    skip_visible_guard: bool,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, &[], count.max(1), skip_visible_guard, None, effects)
}

pub fn consume_queue_prompts_for_done_ids_with_outcome(
    file: &Path,
    done_ids: &[String],
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, done_ids, 1, false, None, effects)
}

pub fn consume_queue_prompts_for_done_ids_force_disk_with_outcome(
    file: &Path,
    done_ids: &[String],
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, done_ids, 1, true, None, effects)
}

/// Strike the active queue head, **skipping the visible-write idle guard**, for
/// the repair recovery path (`#repair-strike-consumed-head`). Repair already
/// writes the recovered response straight to disk (bypassing IPC/IDE), so the
/// matching head strike must also bypass the guard — otherwise a live IDE buffer
/// would block the strike and leave the answered free-text head live for
/// preflight to re-present. Callers must scope this to heads the recovered
/// response actually answered.
pub fn consume_queue_prompt_force_disk(
    file: &Path,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, &[], 1, true, None, effects)
}

pub fn consume_queue_prompts_with_outcome(
    file: &Path,
    done_ids: &[String],
    skip_visible_guard: bool,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(file, done_ids, 1, skip_visible_guard, None, effects)
}

/// Consume exactly the head observed by the caller. If another write already
/// advanced the queue, the stale consume becomes a no-op instead of consuming
/// the next operator intent.
pub fn consume_queue_prompt_if_head_matches_with_outcome(
    file: &Path,
    expected_head: &str,
    done_ids: &[String],
    skip_visible_guard: bool,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_options(
        file,
        done_ids,
        1,
        skip_visible_guard,
        Some(expected_head),
        effects,
    )
}

fn consume_queue_prompts_with_options(
    file: &Path,
    done_ids: &[String],
    free_text_count: usize,
    skip_visible_guard: bool,
    expected_head: Option<&str>,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<Option<QueueConsumptionOutcome>> {
    // Hold the document lock for the entire read-parse-write cycle to prevent
    // concurrent edits from invalidating parsed offsets (TOCTOU fix).
    let _lock = acquire_doc_lock(file)?;
    let content = if skip_visible_guard {
        effects.force_disk_document_content(file, "queue_consume force_disk")?
    } else {
        effects.current_document_content(file, "queue_consume")?
    };
    let snapshot_content = load_snapshot_recovery_only(file, "queue consume planning");
    let Some(plan) = plan_queue_prompt_consumption_with_snapshot_and_count(
        file,
        &content,
        snapshot_content.as_deref(),
        done_ids,
        free_text_count,
    )?
    else {
        return Ok(None);
    };
    if expected_head.is_some_and(|expected| plan.consumed_text.trim() != expected.trim()) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_consume_stale_head_noop expected_hash={} observed_hash={} recovery=monotonic_noop",
                agent_doc_hash::content_hash(expected_head.unwrap_or_default().trim()),
                agent_doc_hash::content_hash(plan.consumed_text.trim())
            ),
        );
        return Ok(None);
    }

    record_queue_consumption_proofs(file, &plan, QueueConsumptionProofStage::BeforeMutation)?;

    // `#fcc0`: converge the queue-consume write through the editor IPC when a JB
    // listener is active (no `File Cache Conflict` dialog); fall back to the
    // guarded disk write otherwise. The force-disk repair path keeps its raw
    // bypass — it deliberately skips IPC/IDE and the visible-write guard.
    if skip_visible_guard {
        effects
            .atomic_write(file, &plan.new_document)
            .context("queue consume: failed to write document")?;
    } else {
        effects
            .converge_document_or_disk(file, &plan.new_document, &content, "queue_consume")
            .context("queue consume: failed to write document")?;
    }
    if plan.save_snapshot {
        save_snapshot_recovery_only(file, &plan.new_snapshot, "queue consume writeback");
    }
    record_queue_consumption_proofs(file, &plan, QueueConsumptionProofStage::AfterMutation)?;

    let outcome = QueueConsumptionOutcome {
        consumed_text: plan.consumed_text.clone(),
        consumed_count: plan.consumed_texts.len(),
        node_ops: plan.node_ops.clone(),
        remaining: plan.remaining,
        drained: plan.drained,
        auto: plan.auto,
    };
    if plan.consumed_texts.len() == 1 {
        eprintln!(
            "[queue] consumed: {:?} (remaining: {})",
            plan.consumed_text, plan.remaining
        );
    } else {
        eprintln!(
            "[queue] consumed {} item(s): {:?} (remaining: {})",
            plan.consumed_texts.len(),
            plan.consumed_texts,
            plan.remaining
        );
    }
    if plan.drained {
        eprintln!("[queue] drained — cleared queue_active");
    } else if plan.auto {
        eprintln!(
            "[queue] auto queue has {} prompt(s) remaining after this closeout",
            plan.remaining
        );
    }

    // #recguard-wedge-escape: a consumed head means the loop advanced, so reset
    // any owner-pane self-invocation wedge counter. Otherwise a future re-add of
    // the same head text could inherit a stale count and halt prematurely.
    if let Err(err) = agent_doc_owner_pane_io::clear(file) {
        eprintln!(
            "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
            file.display(),
            err
        );
    }

    Ok(Some(outcome))
}

struct QueueConsumptionProofRuntimeEffects;

impl QueueConsumptionProofEffects for QueueConsumptionProofRuntimeEffects {
    fn append_state_event(
        &self,
        project_root: &Path,
        event: &agent_doc_state_backbone::StateEvent,
    ) -> Result<bool> {
        agent_doc_controller_io::project_controller::append_state_event(project_root, event)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }

    fn now_millis(&self) -> u64 {
        now_millis()
    }
}

const QUEUE_CONSUMPTION_PROOF_EFFECTS: QueueConsumptionProofRuntimeEffects =
    QueueConsumptionProofRuntimeEffects;

pub fn record_queue_consumption_proofs(
    file: &Path,
    plan: &QueueConsumptionPlan,
    stage: QueueConsumptionProofStage,
) -> Result<()> {
    record_queue_consumption_proofs_with_effects(
        &QUEUE_CONSUMPTION_PROOF_EFFECTS,
        file,
        plan,
        stage,
    )
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Strike every free-text queue head that the committed `response_body` answers,
/// regardless of position (#ftstrike). The normal leading-head consume only
/// strikes a contiguous leading run and stops at an id-backed head, so a free-text
/// report sitting BEHIND an unfinished `do [#id]` head was never struck even after
/// the response addressed it. This pass closes that gap by matching answered
/// free-text heads to the response's quoted-prompt blockquotes. Strikes the doc
/// and the snapshot in sync; returns the number of heads struck. No-op when the
/// queue is inactive, the response is empty, or nothing matches.
pub fn strike_answered_free_text_queue_heads(
    file: &Path,
    response_body: &str,
    skip_visible_guard: bool,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<usize> {
    if response_body.trim().is_empty() {
        return Ok(0);
    }
    // Editor-authoritative lookup may cross the controller socket. Never hold
    // the document lock across that bounded RPC: a slow/unavailable controller
    // would otherwise serialize every preflight/commit contender behind the
    // timeout. The convergence write below still compares against this exact
    // source content. Commit-seam cleanup is explicitly disk-owned, so it reads
    // disk only after acquiring the lock and never contacts the controller.
    let resolved_editor_content = if skip_visible_guard {
        None
    } else {
        Some(effects.current_document_content(file, "free_text_queue_strike")?)
    };
    let _lock = acquire_doc_lock(file)?;
    let content = match resolved_editor_content {
        Some(content) => content,
        None => effects.force_disk_document_content(file, "free_text_queue_strike force_disk")?,
    };
    // A captured/console response is only an intent.  Queue completion requires
    // proof that the same response is present in the authoritative document cut
    // we are about to mutate.  Without this fence a retained write can time out,
    // leave the response absent, and still auto-strike its prompt (#ftstrike).
    if !agent_doc_turn::response_replay::response_materialized_in_content(response_body, &content) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "free_text_head_strike_deferred file={} reason=response_not_materialized response_hash={} authority_hash={}",
                file.display(),
                agent_doc_hash::content_hash(response_body),
                agent_doc_hash::content_hash(&content),
            ),
        );
        return Ok(0);
    }
    let (fm, _) = frontmatter::parse(&content)?;
    if fm.queue_active != Some(true) {
        return Ok(0);
    }
    // `#qstrikeexplain` Phase 2: the stable pre-turn baseline gates which heads may
    // be struck — a head absent from it is an in-flight operator edit and must not
    // be struck this cycle. A missing baseline (rare; preflight writes it each
    // cycle) skips the gate so legacy strike behavior is preserved.
    let baseline = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let keys = answered_free_text_head_node_keys(&content, response_body, baseline.as_deref())?;
    if keys.is_empty() {
        return Ok(0);
    }
    let struck_document = consume_queue_nodes_by_key(&content, &keys)?;
    if struck_document == content {
        return Ok(0);
    }
    // `#qstrikenote` Phase 1: append the deterministic auto-struck explanation to
    // each newly-struck free-text head, on the struck queue line itself (the
    // editor-authoritative `agent:queue` surface the strike already mutates) —
    // NEVER into `agent:exchange`, so on-disk exchange still equals `content_ours`.
    let new_document = annotate_newly_struck_free_text_heads(&content, &struck_document)?;

    // Snapshot sync: match the same answered free-text heads in the snapshot and
    // strike them by the snapshot's own node keys (keys are position/hash derived
    // and need not equal the document's). Required closeouts must prove both sides
    // converge on the struck state.
    let new_snapshot = match load_snapshot_recovery_only(file, "free-text strike snapshot sync") {
        Some(snap) => match (|| -> Result<Option<String>> {
            let snap_keys =
                answered_free_text_head_node_keys(&snap, response_body, baseline.as_deref())?;
            if snap_keys.is_empty() {
                Ok(None)
            } else {
                let snap_struck = consume_queue_nodes_by_key(&snap, &snap_keys)?;
                Ok(Some(annotate_newly_struck_free_text_heads(
                    &snap,
                    &snap_struck,
                )?))
            }
        })() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_recovery_warning(file, "free-text strike snapshot sync", err);
                None
            }
        },
        None => None,
    };

    if skip_visible_guard {
        effects
            .atomic_write(file, &new_document)
            .context("free-text strike: failed to write document")?;
    } else {
        effects
            .converge_document_or_disk(file, &new_document, &content, "free_text_strike")
            .context("free-text strike: failed to write document")?;
    }
    if let Some(snap) = new_snapshot {
        save_snapshot_recovery_only(file, &snap, "free-text strike snapshot sync");
    }

    eprintln!(
        "[queue] struck {} answered free-text head(s) by response match (#ftstrike)",
        keys.len()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "freetext_head_strike file={} struck={}",
            file.display(),
            keys.len()
        ),
    );
    // `#qstrikenote` observability: one marker per struck head naming a short text
    // prefix, so a struck line is auditable as an explained auto-strike.
    if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes(&content, "queue") {
        let key_set: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
        for node in nodes {
            if !key_set.contains(node.node_key.as_str()) {
                continue;
            }
            let prefix: String = node.item.text.trim().chars().take(48).collect();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "free_text_head_struck file={} note=auto_struck_answered head={:?} #qstrikenote",
                    file.display(),
                    prefix
                ),
            );
        }
    }
    Ok(keys.len())
}

/// Strike answered free-text queue heads at the commit seam.
///
/// Sources the answered response from the typed closeout projection first, then
/// the durable capture ledger as a compatibility fallback, and runs the same
/// focused free-text strike used by finalize.
/// Best-effort: a missing capture, inactive queue, or strike error never blocks
/// the commit.
pub fn strike_answered_free_text_heads_at_commit_seam(
    file: &Path,
    effects: &dyn QueueConsumeWriteEffects,
) {
    let Some(response_body) = capture_response_body_for_commit(file) else {
        return;
    };
    if response_body.trim().is_empty() {
        return;
    }
    // The commit seam is already the binary-owned closeout boundary and runs
    // before staging under the commit lock; use the force-disk strike branch so
    // recovery commits do not silently leave answered free-text heads live when
    // no editor listener is attached.
    match strike_answered_free_text_queue_heads(file, &response_body, true, effects) {
        Ok(0) => {}
        Ok(n) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_seam_free_text_strike file={} struck={} (#qheadstrike)",
                file.display(),
                n
            ),
        ),
        Err(err) => eprintln!(
            "[commit] warning: commit-seam free-text head strike failed: {err} (non-fatal)"
        ),
    }
}

fn capture_response_body_for_commit(file: &Path) -> Option<String> {
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()?;
    let capture_id = state.capture_id.as_deref()?;
    if let Ok(Some(projected)) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)
        && state.cycle_id == projected.cycle_id
        && state.response_sha256.as_deref() == Some(projected.response_sha256.as_str())
    {
        return Some(projected.response_body);
    }
    None
}

/// Strike every active queue head that is non-drainable **noise**, at ANY position
/// (`#goqstall2`). Unlike `queue consume` — which strikes only a contiguous LEADING
/// free-text run and stops at the first id-backed head — this clears noise
/// interleaved behind operator-verify `do [#id]` heads, which `queue consume` and
/// the answered-free-text strike could never reach. Id-backed directives and
/// genuinely drainable free-text heads are preserved. Strikes the document and
/// snapshot in sync via the editor-IPC-converged write path (`#fcc0`); returns the
/// number of heads struck. No-op when the queue is inactive or nothing is noise.
///
/// This is the binary-mediated answer to the `queue_stale_noise_lines=N`
/// session-check diagnostic: noise was previously "never auto-deleted (the live IPC
/// supervisor races on direct queue edits)", leaving the operator no safe way to
/// clear pasted console-evidence lines. Routing the strike through the
/// same converge path the closeout strikes use keeps it supervisor-safe.
///
/// Two head shapes are cleared (#qnoise-multiline-strike):
///   1. **Bulleted** single-line noise (`- <prose>`) — struck via durable node keys
///      (`markdown_ast` `item_nodes`), preserving the strike-through marker so a
///      closeout can prove the struck state.
///   2. **Multiline** `---`/```/~~~-fenced noise Prompt blocks (operator-pasted
///      `:round_pushpin:` console dumps) — these are NOT bulleted list items and
///      contain a fenced region, so `item_nodes` never enumerated them and they
///      accumulated forever. They are excised by exact byte range from the single
///      source of queue-head segmentation (`queue::parse_spans`).
pub fn prune_noise_queue_heads(
    file: &Path,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<usize> {
    let _lock = acquire_doc_lock(file)?;
    let content = effects.current_document_content(file, "queue_noise_prune")?;
    let (fm, _) = frontmatter::parse(&content)?;
    if fm.queue_active != Some(true) {
        return Ok(0);
    }
    let (new_document, struck) = strike_all_noise_queue_heads(&content)?;
    if struck == 0 || new_document == content {
        return Ok(0);
    }

    // Snapshot sync: clear the same noise heads in the snapshot (its own node keys /
    // spans, derived independently) so required closeouts prove both sides converge.
    let new_snapshot = match load_snapshot_recovery_only(file, "noise prune snapshot sync") {
        Some(snap) => match (|| -> Result<Option<String>> {
            let (new_snap, _) = strike_all_noise_queue_heads(&snap)?;
            if new_snap == snap {
                Ok(None)
            } else {
                Ok(Some(new_snap))
            }
        })() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_recovery_warning(file, "noise prune snapshot sync", err);
                None
            }
        },
        None => None,
    };

    effects
        .converge_document_or_disk(file, &new_document, &content, "noise_prune")
        .context("noise prune: failed to write document")?;
    if let Some(snap) = new_snapshot {
        save_snapshot_recovery_only(file, &snap, "noise prune snapshot sync");
    }

    let base_hash = agent_doc_hash::content_hash(&content);
    eprintln!(
        "[queue] pruned {struck} predicate-proven head(s): noise + orphan id-backed (#goqstall2/#orphanqhead)"
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_noise_prune file={} struck={} base_hash={} source_component=queue operation=prune proof=predicate_noise_or_orphan_id",
            file.display(),
            struck,
            base_hash
        ),
    );
    Ok(struck)
}

/// Strike an **orphaned id-backed queue head** by id (#orphanqhead). An id-backed
/// directive head (`do [#id]` / `[#id]` / `#id`) whose backing backlog item was
/// already reaped (`--done` reports "already resolved") or is otherwise gone has
/// no drain path: `queue consume` rejects id-backed heads ("reap via --done") and
/// `--done <id>` is a no-op, so the phantom head sits forever and keeps re-firing
/// the auto-loop. This is the explicit operator escape hatch
/// `agent-doc queue consume --id <id>`, which strikes that specific head in the
/// document and snapshot in sync.
///
/// Guard: refuses to strike a head whose id still names an OPEN (non-done) backlog
/// item — that is live work with a real `--done` / `--pending-gate` drain path, so
/// the operator should use those instead. Returns `true` when a head was struck,
/// `false` when nothing matched (already struck / drained).
pub fn strike_orphan_id_backed_queue_head(
    file: &Path,
    id: &str,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<bool> {
    let _lock = acquire_doc_lock(file)?;
    let content = effects.current_document_content(file, "orphan_queue_head_strike")?;
    let target_id =
        agent_doc_element_backlog::backlog::normalize_pending_id(id).to_ascii_lowercase();
    if target_id.is_empty() {
        anyhow::bail!("orphan strike: empty id");
    }
    // A head whose id still names OPEN backlog work has a real drain path — do not
    // let the escape hatch desync live work; require the normal closeout instead.
    if head_id_names_open_backlog_item(&content, &target_id) {
        anyhow::bail!(
            "{}: [#{target_id}] still names an OPEN backlog item with a real drain path — \
             reap it through `--done {target_id}` / `--pending-gate {target_id}` instead of \
             force-striking the queue head.",
            file.display()
        );
    }
    let keys = id_backed_head_node_keys(&content, &target_id)?;
    if keys.is_empty() {
        anyhow::bail!(
            "{}: no live id-backed queue head matching [#{target_id}] to strike \
             (already struck, drained, or the head is free-text).",
            file.display()
        );
    }
    let new_document = consume_queue_nodes_by_key(&content, &keys)?;
    if new_document == content {
        return Ok(false);
    }
    // Snapshot sync: strike the same id-backed head in the snapshot by its own node
    // keys so required closeouts prove both sides converge on the struck state.
    let new_snapshot = match load_snapshot_recovery_only(file, "orphan id head snapshot sync") {
        Some(snap) => match (|| -> Result<Option<String>> {
            let snap_keys = id_backed_head_node_keys(&snap, &target_id)?;
            if snap_keys.is_empty() {
                Ok(None)
            } else {
                Ok(Some(consume_queue_nodes_by_key(&snap, &snap_keys)?))
            }
        })() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_recovery_warning(file, "orphan id head snapshot sync", err);
                None
            }
        },
        None => None,
    };
    let base_hash = agent_doc_hash::content_hash(&content);
    effects
        .converge_document_or_disk(file, &new_document, &content, "orphan_id_head_strike")
        .context("orphan strike: failed to write document")?;
    if let Some(snap) = new_snapshot {
        save_snapshot_recovery_only(file, &snap, "orphan id head snapshot sync");
    }
    eprintln!(
        "[queue] struck orphaned id-backed head [#{target_id}] ({} node(s); #orphanqhead)",
        keys.len()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "orphan_id_head_strike file={} id={} struck={} base_hash={} source_component=queue operation=strike_head proof=orphan_id_no_open_backlog",
            file.display(),
            target_id,
            keys.len(),
            base_hash
        ),
    );
    Ok(true)
}

/// Acknowledge an exact id-backed queue head whose id still names open backlog
/// work, without marking that backlog item done or gated (#freshqueueauth).
///
/// This is intentionally separate from [`strike_orphan_id_backed_queue_head`]:
/// `--id` proves the head is an orphan, while `--ack-id` proves the operator is
/// acknowledging a correction/reminder head and wants the underlying backlog work
/// to remain open. The command still refuses prose that merely mentions `#id`;
/// those are free-text heads and should be answered + consumed through the
/// normal free-text path.
pub fn acknowledge_open_id_backed_queue_head(
    file: &Path,
    id: &str,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<bool> {
    let _lock = acquire_doc_lock(file)?;
    let content = effects.current_document_content(file, "open_id_head_ack")?;
    let target_id =
        agent_doc_element_backlog::backlog::normalize_pending_id(id).to_ascii_lowercase();
    if target_id.is_empty() {
        anyhow::bail!("open-id ack: empty id");
    }
    if !head_id_names_open_backlog_item(&content, &target_id) {
        anyhow::bail!(
            "{}: [#{target_id}] does not name an OPEN backlog item. Use \
            `agent-doc queue consume {} --id {target_id}` for orphan id-backed heads, \
            or leave the head queued.",
            file.display(),
            file.display()
        );
    }
    let keys = id_backed_head_node_keys(&content, &target_id)?;
    if keys.is_empty() {
        anyhow::bail!(
            "{}: no exact id-backed queue head matching [#{target_id}] to acknowledge \
            (already struck/drained, or the head is prose that merely mentions the id; \
            answer prose heads and use `agent-doc queue consume {} --count 1`).",
            file.display(),
            file.display()
        );
    }
    let new_document = consume_queue_nodes_by_key(&content, &keys)?;
    if new_document == content {
        return Ok(false);
    }
    let new_snapshot = match load_snapshot_recovery_only(file, "open id ack snapshot sync") {
        Some(snap) => match (|| -> Result<Option<String>> {
            let snap_keys = id_backed_head_node_keys(&snap, &target_id)?;
            if snap_keys.is_empty() {
                Ok(None)
            } else {
                Ok(Some(consume_queue_nodes_by_key(&snap, &snap_keys)?))
            }
        })() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_recovery_warning(file, "open id ack snapshot sync", err);
                None
            }
        },
        None => None,
    };
    let base_hash = agent_doc_hash::content_hash(&content);
    effects
        .converge_document_or_disk(file, &new_document, &content, "open_id_head_ack")
        .context("open-id ack: failed to write document")?;
    if let Some(snap) = new_snapshot {
        save_snapshot_recovery_only(file, &snap, "open id ack snapshot sync");
    }
    eprintln!(
        "[queue] acknowledged id-backed correction head [#{target_id}] ({} node(s); backlog left open; #freshqueueauth)",
        keys.len()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "open_id_head_ack file={} id={} struck={} base_hash={} source_component=queue operation=strike_head proof=operator_acknowledged_correction_preserve_open_backlog",
            file.display(),
            target_id,
            keys.len(),
            base_hash
        ),
    );
    Ok(true)
}

pub fn mark_completed_queue_prompts_for_done_ids(
    file: &Path,
    done_ids: &[String],
    skip_visible_guard: bool,
    effects: &dyn QueueConsumeWriteEffects,
) -> Result<usize> {
    if done_ids.is_empty() {
        return Ok(0);
    }

    let _lock = acquire_doc_lock(file)?;
    let content = effects.current_document_content(file, "queue_done_id_mark")?;
    let components = element::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(0);
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("queue done-id mark: failed to parse document queue")?;
    let (marked_entries, marked_texts) = mark_entries_completed_by_done_ids(&entries, done_ids);
    if marked_texts.is_empty() {
        return Ok(0);
    }

    let new_body = agent_doc_queue::document_queue::render(&marked_entries);
    let new_document = queue_component.replace_content(&content, &new_body);

    let new_snapshot = if let Some(snapshot_content) =
        load_snapshot_recovery_only(file, "queue done-id mark")
    {
        match (|| -> Result<Option<String>> {
            let snapshot_components = element::parse(&snapshot_content)?;
            let Some(snapshot_queue) = snapshot_components
                .iter()
                .find(|component| component.name == "queue")
            else {
                return Ok(None);
            };
            let snapshot_body =
                &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
            let snapshot_entries = agent_doc_queue::document_queue::parse(snapshot_body)
                .context("queue done-id mark: failed to parse snapshot queue")?;
            let (snapshot_marked_entries, snapshot_marked_texts) =
                mark_entries_completed_by_done_ids(&snapshot_entries, done_ids);
            if snapshot_marked_texts.len() != marked_texts.len() {
                log_snapshot_recovery_warning(
                    file,
                    "queue done-id mark",
                    format!(
                        "snapshot matched {} queue item(s) but document matched {}",
                        snapshot_marked_texts.len(),
                        marked_texts.len()
                    ),
                );
            }
            let snapshot_body = agent_doc_queue::document_queue::render(&snapshot_marked_entries);
            Ok(Some(
                snapshot_queue.replace_content(&snapshot_content, &snapshot_body),
            ))
        })() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_recovery_warning(file, "queue done-id mark", err);
                None
            }
        }
    } else {
        None
    };

    // `#fcc0`: converge the done-id mark write through the editor IPC when a JB
    // listener is active (no `File Cache Conflict` dialog); fall back to the
    // guarded disk write otherwise. The force-disk repair path keeps its raw
    // bypass — it deliberately skips IPC/IDE and the visible-write guard.
    if skip_visible_guard {
        effects
            .atomic_write(file, &new_document)
            .context("queue done-id mark: failed to write document")?;
    } else {
        effects
            .converge_document_or_disk(file, &new_document, &content, "queue_done_id_mark")
            .context("queue done-id mark: failed to write document")?;
    }
    if let Some(new_snapshot) = new_snapshot {
        save_snapshot_recovery_only(file, &new_snapshot, "queue done-id mark");
    }

    eprintln!(
        "[queue] marked {} completed item(s) by done id: {:?}",
        marked_texts.len(),
        marked_texts
    );
    Ok(marked_texts.len())
}

pub fn plan_queue_prompt_consumption(
    file: &Path,
    content: &str,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionPlan>> {
    let snapshot_content = load_snapshot_recovery_only(file, "queue consume planning");
    plan_queue_prompt_consumption_with_snapshot(
        file,
        content,
        snapshot_content.as_deref(),
        done_ids,
    )
}

pub fn plan_queue_prompt_consumption_with_snapshot(
    file: &Path,
    content: &str,
    snapshot_content: Option<&str>,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionPlan>> {
    plan_queue_prompt_consumption_with_snapshot_and_count(
        file,
        content,
        snapshot_content,
        done_ids,
        1,
    )
}

pub fn plan_queue_prompt_consumption_with_snapshot_and_count(
    file: &Path,
    content: &str,
    snapshot_content: Option<&str>,
    done_ids: &[String],
    requested_free_text_count: usize,
) -> Result<Option<QueueConsumptionPlan>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }

    let components = element::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but document has no agent:queue component"
            )
        })?;

    let body = &content[comp.open_end..comp.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("queue consume: failed to parse document queue")?;

    let leading_done_consume_count = queue_consume_count_for_done_ids(&entries, done_ids);
    if leading_done_consume_count > 0 {
        let (completed_entries, consumed_texts) =
            mark_entries_completed_by_done_ids(&entries, done_ids);
        if !consumed_texts.is_empty() {
            let consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "queue consume: done-id consumption matched no queue prompt to consume"
                )
            })?;
            let consumed_node_keys =
                queue_prompt_node_keys_for_done_ids(content, done_ids, &consumed_texts);
            let node_ops = queue_consume_node_ops(&consumed_node_keys.keys);

            let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs);
            let remaining = agent_doc_queue::document_queue::prompts(&completed_entries).len();
            let drained = remaining == 0;
            let new_entries = if drained {
                Vec::new()
            } else {
                completed_entries
            };
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            // `#qconsumenostrike`: a targeted consume whose head is NOT
            // AST-addressable must fail closed. The whole-component re-render
            // below marks entries complete by POSITION, so when `parse_spans`
            // and `markdown_ast::item_nodes` disagree about segmentation — as
            // they still do for multiline `---`-fenced prompts, which render
            // without a bullet — it strikes a NEIGHBOUR and silently marks
            // unrun work complete. Observed twice on this repo's own session
            // document (`#c8tb`, then `#orphandrain` + `#c8tb` again).
            //
            // A full drain has no targeting ambiguity (every entry goes), so it
            // keeps the re-render path.
            if !drained && !consumed_node_keys.ast_backed {
                anyhow::bail!(
                    "queue consume: refusing to strike — the target head is not addressable as a \
                     markdown node, so a positional re-render could mark unrelated queue work \
                     complete (#qconsumenostrike). This shape is typically a multiline `---` \
                     prompt, which renders without a `- ` bullet and is invisible to the node \
                     enumerator. Resolve the head through its id (`--done <id>` / \
                     `--backlog-gate <id>`), or leave it queued."
                );
            }
            let mut current = if drained {
                comp.replace_content(content, &new_body)
            } else {
                consume_queue_nodes_by_key(content, &consumed_node_keys.keys)?
            };

            if drained {
                if has_auto {
                    let comps = element::parse(&current)?;
                    if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                        let raw = &current[q.open_start..q.open_end];
                        let new_tag = agent_doc_queue::document_queue::strip_auto_from_tag(raw);
                        if new_tag != raw {
                            let mut rebuilt = String::with_capacity(current.len());
                            rebuilt.push_str(&current[..q.open_start]);
                            rebuilt.push_str(&new_tag);
                            rebuilt.push_str(&current[q.open_end..]);
                            current = rebuilt;
                        }
                    }
                }
                current = frontmatter::merge_queue_state(&current, false)?;
            }

            let response_first_line = capture_response_body_for_commit(file)
                .and_then(|body| first_nonempty_line(&body).map(str::to_string));
            current = embed_consumed_prompt_in_response(
                &current,
                &consumed_texts,
                response_first_line.as_deref(),
            );
            let mut new_snap = snapshot_content.unwrap_or(content).to_string();
            let mut save_snapshot = false;
            if let Some(snap) = snapshot_content {
                match (|| -> Result<Option<String>> {
                    let snap_comps = element::parse(snap)?;
                    let Some(snap_queue) = snap_comps.iter().find(|c| c.name == "queue") else {
                        return Ok(None);
                    };
                    let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
                    let snap_entries = agent_doc_queue::document_queue::parse(snap_body)
                        .context("queue consume: failed to parse snapshot queue")?;
                    let snap_has_auto =
                        agent_doc_queue::document_queue::has_auto_attr(&snap_queue.attrs);
                    let (snap_completed_entries, snapshot_consumed_texts) =
                        mark_entries_completed_by_done_ids(&snap_entries, done_ids);
                    if normalized_done_id_bag(&snapshot_consumed_texts)
                        != normalized_done_id_bag(&consumed_texts)
                    {
                        log_snapshot_recovery_warning(
                            file,
                            "queue consume done-id snapshot sync",
                            format!(
                                "snapshot done-id prompts {:?} do not match document done-id prompts {:?}",
                                snapshot_consumed_texts, consumed_texts
                            ),
                        );
                        return Ok(None);
                    }
                    let snapshot_node_keys = queue_prompt_node_keys_for_done_ids(
                        snap,
                        done_ids,
                        &snapshot_consumed_texts,
                    );
                    let snap_remaining =
                        agent_doc_queue::document_queue::prompts(&snap_completed_entries).len();
                    let snap_new_entries = if snap_remaining == 0 {
                        Vec::new()
                    } else {
                        snap_completed_entries
                    };
                    if snap_new_entries != new_entries {
                        let snap_remaining_prompts =
                            agent_doc_queue::document_queue::prompts(&snap_new_entries).len();
                        let doc_remaining_prompts =
                            agent_doc_queue::document_queue::prompts(&new_entries).len();
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "queue_done_id_consume_divergence_reconciled file={} cause=done_id_authoritative consumed={} snap_remaining={} doc_remaining={}",
                                file.display(),
                                consumed_texts.len(),
                                snap_remaining_prompts,
                                doc_remaining_prompts
                            ),
                        );
                    }

                    let mut new_snap = if drained
                        || snap_new_entries != new_entries
                        || !snapshot_node_keys.ast_backed
                    {
                        snap_queue.replace_content(snap, &new_body)
                    } else {
                        consume_queue_nodes_by_key(snap, &snapshot_node_keys.keys)?
                    };
                    if drained {
                        if snap_has_auto
                            && let Ok(sc2) = element::parse(&new_snap)
                            && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
                        {
                            let raw = &new_snap[sq2.open_start..sq2.open_end];
                            let new_tag = agent_doc_queue::document_queue::strip_auto_from_tag(raw);
                            if new_tag != raw {
                                let mut rebuilt = String::with_capacity(new_snap.len());
                                rebuilt.push_str(&new_snap[..sq2.open_start]);
                                rebuilt.push_str(&new_tag);
                                rebuilt.push_str(&new_snap[sq2.open_end..]);
                                new_snap = rebuilt;
                            }
                        }
                        new_snap = frontmatter::merge_queue_state(&new_snap, false)?;
                    }
                    Ok(Some(embed_consumed_prompt_in_response(
                        &new_snap,
                        &consumed_texts,
                        response_first_line.as_deref(),
                    )))
                })() {
                    Ok(Some(snapshot)) => {
                        save_snapshot = snapshot != snap;
                        new_snap = snapshot;
                    }
                    Ok(None) => log_snapshot_recovery_warning(
                        file,
                        "queue consume done-id snapshot sync",
                        "snapshot cannot be synchronized to the document queue",
                    ),
                    Err(err) => log_snapshot_recovery_warning(
                        file,
                        "queue consume done-id snapshot sync",
                        err,
                    ),
                }
            } else {
                log_snapshot_recovery_warning(
                    file,
                    "queue consume done-id snapshot sync",
                    "snapshot is missing",
                );
            }

            return Ok(Some(QueueConsumptionPlan {
                consumed_text,
                consumed_texts,
                node_ops,
                remaining,
                drained,
                auto: has_auto,
                new_document: current,
                new_snapshot: new_snap,
                save_snapshot,
            }));
        }
    }

    let requested_free_text_count = requested_free_text_count.max(1);
    let requested_texts = first_n_queue_prompt_texts(&entries, requested_free_text_count);
    let free_text_prefix_count = requested_texts
        .iter()
        .take_while(|text| queue_prompt_text_is_free_text(content, text))
        .count();
    // Preserve the fail-closed active-queue guard: an active component with no
    // prompt is malformed, while a first id-backed prompt is classified below
    // and left untouched without an explicit done/ack signal.
    let consume_count = leading_done_consume_count
        .max(free_text_prefix_count)
        .max(1);
    let consumed_texts = first_n_queue_prompt_texts(&entries, consume_count);
    // `#queuedrainednoop`: distinguish a DRAINED queue from a malformed one.
    //
    // The fail-closed guard below is right for an active component that never had
    // a prompt. But a queue whose heads are all already `Completed` (struck) is
    // the normal END STATE of a successful drain, not corruption — and with
    // `queue: go` still in frontmatter it kept reaching this error, so
    // `agent-doc write --commit` refused with "queue_active is true but document
    // queue has no prompt to consume" while `agent-doc commit` silently no-opped.
    // That left ordinary bookkeeping (a queue-head strike) impossible to commit
    // through the binary at all: frontmatter is not a patchable component, so the
    // only escape was writing `queue: go` off under a live editor — the exact
    // disk-vs-authority divergence the write path exists to prevent.
    //
    // A drained queue consumes nothing and succeeds, so the cycle can commit
    // whatever else it carries.
    if consumed_texts.is_empty()
        && entries
            .iter()
            .any(|entry| matches!(entry, agent_doc_queue::document_queue::QueueEntry::Completed(_)))
        && !entries.iter().any(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Prompt(_)
            )
        })
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_consume_drained_noop file={} reason=all_heads_struck (#queuedrainednoop)",
                file.display()
            ),
        );
        return Ok(None);
    }

    let consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "queue consume: queue_active is true but document queue has no prompt to consume"
        )
    })?;

    if leading_done_consume_count == 0 && !queue_head_is_free_text_prompt(content)? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_consume_refused_id_backed_head_without_explicit_signal file={} head={:?}",
                file.display(),
                consumed_text
            ),
        );
        return Ok(None);
    }
    let consumed_node_keys = queue_prompt_node_keys_for_count(content, consume_count)?;
    let completed_entries =
        agent_doc_queue::document_queue::mark_first_n_prompts_completed(&entries, consume_count);
    let node_ops = queue_consume_node_ops(&consumed_node_keys.keys);

    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs);
    let remaining = agent_doc_queue::document_queue::prompts(&completed_entries).len();
    let drained = remaining == 0;
    let new_entries = if drained {
        Vec::new()
    } else {
        completed_entries
    };
    let new_body = agent_doc_queue::document_queue::render(&new_entries);
    // `#qconsumenostrike`: see the sibling guard above — a targeted consume of a
    // head the node enumerator cannot address must fail closed rather than fall
    // back to a positional whole-component re-render that can strike a
    // neighbour and mark unrun work complete.
    if !drained && !consumed_node_keys.ast_backed {
        anyhow::bail!(
            "queue consume: refusing to strike — the target head is not addressable as a \
             markdown node, so a positional re-render could mark unrelated queue work complete \
             (#qconsumenostrike). This shape is typically a multiline `---` prompt, which renders \
             without a `- ` bullet and is invisible to the node enumerator. Resolve the head \
             through its id (`--done <id>` / `--backlog-gate <id>`), or leave it queued."
        );
    }
    let mut current = if drained {
        comp.replace_content(content, &new_body)
    } else {
        consume_queue_nodes_by_key(content, &consumed_node_keys.keys)?
    };

    if drained {
        if has_auto {
            let comps = element::parse(&current)?;
            if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                let raw = &current[q.open_start..q.open_end];
                let new_tag = agent_doc_queue::document_queue::strip_auto_from_tag(raw);
                if new_tag != raw {
                    let mut rebuilt = String::with_capacity(current.len());
                    rebuilt.push_str(&current[..q.open_start]);
                    rebuilt.push_str(&new_tag);
                    rebuilt.push_str(&current[q.open_end..]);
                    current = rebuilt;
                }
            }
        }
        current = frontmatter::merge_queue_state(&current, false)?;
    }
    // #queue-prompt-echo-in-response: an auto/synthetic queue head is never typed
    // into `agent:exchange`, so a consumed queue turn would otherwise record only
    // the `### Re:` answer with no trace of the originating prompt. Embed the
    // consumed prompt text into this cycle's response block (in BOTH the document
    // and the snapshot, so the selective-commit boundary stays consistent) when
    // the prompt is not already present in the exchange. Fail-safe: any locator
    // miss leaves the content unchanged rather than risk corrupting the exchange.
    let response_first_line = capture_response_body_for_commit(file)
        .and_then(|body| first_nonempty_line(&body).map(str::to_string));
    current = embed_consumed_prompt_in_response(
        &current,
        &consumed_texts,
        response_first_line.as_deref(),
    );
    let mut new_snap = snapshot_content.unwrap_or(content).to_string();
    let mut save_snapshot = false;
    if let Some(snap) = snapshot_content {
        match (|| -> Result<Option<String>> {
            let snap_comps = element::parse(snap)?;
            let Some(snap_queue) = snap_comps.iter().find(|c| c.name == "queue") else {
                return Ok(None);
            };
            let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
            let snap_entries = agent_doc_queue::document_queue::parse(snap_body)
                .context("queue consume: failed to parse snapshot queue")?;
            let snap_has_auto = agent_doc_queue::document_queue::has_auto_attr(&snap_queue.attrs);
            let snapshot_consumed_texts = first_n_queue_prompt_texts(&snap_entries, consume_count);
            let norm = |texts: &[String]| {
                texts
                    .iter()
                    .map(|text| strip_priority_markers(text))
                    .collect::<Vec<_>>()
            };
            let snapshot_heads_match = snapshot_consumed_texts.len() == consumed_texts.len()
                && norm(&snapshot_consumed_texts) == norm(&consumed_texts);
            if !snapshot_heads_match {
                log_snapshot_recovery_warning(
                    file,
                    "queue consume snapshot sync",
                    format!(
                        "snapshot head prompts {:?} do not match editor-authoritative document head prompts {:?}; rebasing snapshot queue projection onto the document result",
                        snapshot_consumed_texts, consumed_texts
                    ),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_consume_snapshot_rebased file={} authority=editor_document consumed={} recovery=replace_snapshot_queue_projection",
                        file.display(),
                        consume_count,
                    ),
                );
            }
            let (snap_new_entries, snapshot_node_keys) = if snapshot_heads_match {
                let snap_completed_entries =
                    agent_doc_queue::document_queue::mark_first_n_prompts_completed(
                        &snap_entries,
                        consume_count,
                    );
                let snap_remaining =
                    agent_doc_queue::document_queue::prompts(&snap_completed_entries).len();
                let snap_new_entries = if snap_remaining == 0 {
                    Vec::new()
                } else {
                    snap_completed_entries
                };
                (
                    snap_new_entries,
                    Some(queue_prompt_node_keys_for_count(snap, consume_count)?),
                )
            } else {
                // The document/editor cut is authoritative. The snapshot is a
                // recovery projection, so a stale snapshot head is rebased to
                // the exact post-consume document queue rather than vetoing the
                // mutation or preserving a second head authority.
                (new_entries.clone(), None)
            };
            if snap_new_entries != new_entries {
                let snap_remaining_prompts =
                    agent_doc_queue::document_queue::prompts(&snap_new_entries).len();
                let doc_remaining_prompts =
                    agent_doc_queue::document_queue::prompts(&new_entries).len();
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_consume_divergence_reconciled file={} reason=document_authoritative consumed={} snap_remaining={} doc_remaining={}",
                        file.display(),
                        consume_count,
                        snap_remaining_prompts,
                        doc_remaining_prompts
                    ),
                );
            }

            let mut new_snap = if drained
                || snap_new_entries != new_entries
                || snapshot_node_keys
                    .as_ref()
                    .is_none_or(|keys| !keys.ast_backed)
            {
                snap_queue.replace_content(snap, &new_body)
            } else {
                consume_queue_nodes_by_key(
                    snap,
                    &snapshot_node_keys
                        .expect("matching snapshot heads have node keys")
                        .keys,
                )?
            };
            if drained {
                if snap_has_auto
                    && let Ok(sc2) = element::parse(&new_snap)
                    && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
                {
                    let raw = &new_snap[sq2.open_start..sq2.open_end];
                    let new_tag = agent_doc_queue::document_queue::strip_auto_from_tag(raw);
                    if new_tag != raw {
                        let mut rebuilt = String::with_capacity(new_snap.len());
                        rebuilt.push_str(&new_snap[..sq2.open_start]);
                        rebuilt.push_str(&new_tag);
                        rebuilt.push_str(&new_snap[sq2.open_end..]);
                        new_snap = rebuilt;
                    }
                }
                new_snap = frontmatter::merge_queue_state(&new_snap, false)?;
            }
            Ok(Some(embed_consumed_prompt_in_response(
                &new_snap,
                &consumed_texts,
                response_first_line.as_deref(),
            )))
        })() {
            Ok(Some(snapshot)) => {
                save_snapshot = snapshot != snap;
                new_snap = snapshot;
            }
            Ok(None) => log_snapshot_recovery_warning(
                file,
                "queue consume snapshot sync",
                "snapshot cannot be synchronized to the document queue",
            ),
            Err(err) => log_snapshot_recovery_warning(file, "queue consume snapshot sync", err),
        }
    } else {
        log_snapshot_recovery_warning(file, "queue consume snapshot sync", "snapshot is missing");
    }

    Ok(Some(QueueConsumptionPlan {
        consumed_text,
        consumed_texts,
        node_ops,
        remaining,
        drained,
        auto: has_auto,
        new_document: current,
        new_snapshot: new_snap,
        save_snapshot,
    }))
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tempfile::TempDir;

    struct TestQueueConsumeEffects;

    static TEST_EFFECTS: TestQueueConsumeEffects = TestQueueConsumeEffects;

    impl QueueConsumeWriteEffects for TestQueueConsumeEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to read test document {}", file.display()))
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            agent_doc_fs::write_atomic(file, content.as_bytes())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            reason: &str,
        ) -> Result<()> {
            agent_doc_fs::write_atomic(file, target_content.as_bytes()).with_context(|| {
                format!(
                    "{reason}: failed detached disk write for {}",
                    file.display()
                )
            })?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{reason}_writeback file={} transport=disk_detached reason={} len={} hash={}",
                    file.display(),
                    reason,
                    target_content.len(),
                    agent_doc_hash::content_hash(target_content)
                ),
            );
            Ok(())
        }
    }

    #[derive(Default)]
    struct TrackingStrikeEffects {
        current_reads: AtomicUsize,
        force_disk_reads: AtomicUsize,
    }

    impl QueueConsumeWriteEffects for TrackingStrikeEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.current_reads.fetch_add(1, Ordering::SeqCst);
            let lock_path = agent_doc_fs::state_lock_path_for(file)?;
            if let Some(parent) = lock_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let probe = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)?;
            probe
                .try_lock_exclusive()
                .context("controller/current lookup ran while the document lock was held")?;
            FileExt::unlock(&probe)?;
            Ok(std::fs::read_to_string(file)?)
        }

        fn force_disk_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.force_disk_reads.fetch_add(1, Ordering::SeqCst);
            Ok(std::fs::read_to_string(file)?)
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            agent_doc_fs::write_atomic(file, content.as_bytes())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            agent_doc_fs::write_atomic(file, target_content.as_bytes())
        }
    }

    const HALT_QUEUE_DOC: &str = concat!(
        "---\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#foo]\n",
        "- do [#bar]\n",
        "<!-- /agent:queue -->\n",
    );

    fn consume_queue_prompt_with_outcome(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
        super::consume_queue_prompt_with_outcome(file, &TEST_EFFECTS)
    }

    fn consume_queue_prompt_force_disk(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
        super::consume_queue_prompt_force_disk(file, &TEST_EFFECTS)
    }

    #[test]
    fn expected_head_fence_never_consumes_a_newer_operator_intent() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        let content = concat!(
            "---\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- second operator intent\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, content).unwrap();

        let outcome = super::consume_queue_prompt_if_head_matches_with_outcome(
            &doc,
            "first operator intent",
            &[],
            true,
            &TEST_EFFECTS,
        )
        .unwrap();

        assert!(outcome.is_none());
        assert_eq!(fs::read_to_string(&doc).unwrap(), content);
    }

    fn mark_completed_queue_prompts_for_done_ids(
        file: &Path,
        done_ids: &[String],
        skip_visible_guard: bool,
    ) -> Result<usize> {
        super::mark_completed_queue_prompts_for_done_ids(
            file,
            done_ids,
            skip_visible_guard,
            &TEST_EFFECTS,
        )
    }

    fn strike_answered_free_text_queue_heads(
        file: &Path,
        response_body: &str,
        skip_visible_guard: bool,
    ) -> Result<usize> {
        super::strike_answered_free_text_queue_heads(
            file,
            response_body,
            skip_visible_guard,
            &TEST_EFFECTS,
        )
    }

    fn prune_noise_queue_heads(file: &Path) -> Result<usize> {
        super::prune_noise_queue_heads(file, &TEST_EFFECTS)
    }

    fn strike_orphan_id_backed_queue_head(file: &Path, id: &str) -> Result<bool> {
        super::strike_orphan_id_backed_queue_head(file, id, &TEST_EFFECTS)
    }

    fn acknowledge_open_id_backed_queue_head(file: &Path, id: &str) -> Result<bool> {
        super::acknowledge_open_id_backed_queue_head(file, id, &TEST_EFFECTS)
    }

    #[test]
    fn consume_queue_nodes_by_key_strips_in_progress_marker_before_strike_text() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- 🚧 do [#head]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        let mut keys = queue_prompt_node_keys_for_count(content, 1).unwrap().keys;
        let key = keys.remove(0);

        let updated = consume_queue_nodes_by_key(content, &[key]).unwrap();

        assert!(updated.contains("- ~~do [#head]~~\n"), "{updated}");
        assert!(!updated.contains("~~🚧"), "{updated}");
        assert!(updated.contains("- do [#tail]\n"), "{updated}");
    }

    #[test]
    fn free_text_head_struck_despite_prompt_prefix_flip_on_answered_prompt() {
        // #free-text-head-consume-genuine-not-struck: the consume decision diffs
        // the normalized snapshot baseline against the LIVE editor buffer. The
        // buffer preserves `❯` prefixes on already-answered prompts that the
        // snapshot normalized to the bare form. A pure `do x` → `❯ do x`
        // prefix flip then surfaces as an added `+❯ …` diff line. It must
        // NOT be read as a new foreign prompt — that wrongly blocked the
        // free-text head strike and stalled the auto-loop.
        let head = "Evaluate axocoatl thing";
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        // Live buffer: the prior prompt regained its `❯` prefix; this cycle
        // only added the `### Re: axocoatl` answer.
        let prefix_flip = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "### Re: axocoatl\n",
            "plan written.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), prefix_flip, head),
            "a `❯` prefix flip on an already-answered baseline prompt is not new foreign work"
        );

        // A genuinely new `❯` prompt whose text never appeared at baseline still
        // counts as foreign work, keeping the free-text head queued.
        let genuine_foreign = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "❯ a brand new unrelated prompt\n",
            "### Re: axocoatl\n",
            "plan.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), genuine_foreign, head),
            "a genuinely new unrelated `❯` prompt absent from baseline is foreign work"
        );
    }
    #[test]
    fn consumed_prompt_echo_skips_stale_blockquoted_echo_variant() {
        let prompt = ":pushpin: Fix the root cause of this issue that occurred in this document.";
        let content = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: root fix\n\n",
                "> **Queue prompt:**\n>\n",
                "> Fix the root cause of this issue that occurred in this document.\n\n",
                "Handled once.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue priority go -->\n",
                "- {prompt}\n",
                "<!-- /agent:queue -->\n",
            ),
            prompt = prompt
        );

        let updated = embed_consumed_prompt_in_response(
            &content,
            &[prompt.to_string()],
            Some("### Re: root fix"),
        );

        assert_eq!(
            updated, content,
            "a stale blockquoted queue-prompt echo with the priority marker stripped must not be reinserted"
        );
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);

        let one_line_echo = content.replace(
            "> **Queue prompt:**\n>\n> Fix the root cause of this issue that occurred in this document.",
            "> **Queue prompt:** Fix the root cause of this issue that occurred in this document.",
        );
        let updated = embed_consumed_prompt_in_response(
            &one_line_echo,
            &[prompt.to_string()],
            Some("### Re: root fix"),
        );
        assert_eq!(
            updated, one_line_echo,
            "legacy one-line queue-prompt echoes must also count as already present"
        );
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);
    }
    #[test]
    fn free_text_consume_preserves_following_id_backed_head() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        let prompt = ":pushpin: Fix the root cause of this issue that occurred in this document.";
        let content = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: root fix\n\n",
                "Handled.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue priority go -->\n",
                "- {prompt}\n",
                "- [#fccd]\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#fccd] Restore the accidentally consumed Clear Session Cache queue head.\n",
                "<!-- /agent:backlog -->\n",
            ),
            prompt = prompt
        );
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = plan_queue_prompt_consumption(&doc, &content, &[])
            .unwrap()
            .expect("free-text head should be consumable");

        assert_eq!(plan.consumed_texts, vec![prompt.to_string()]);
        assert_eq!(plan.remaining, 1);
        assert!(
            plan.new_document.contains("- [#fccd]\n"),
            "open id-backed heads behind the consumed free-text prompt must remain queued:\n{}",
            plan.new_document
        );
        assert!(
            !plan.new_document.contains("~~[#fccd]"),
            "the open id-backed head must not be struck by a free-text consume"
        );
        assert_eq!(plan.new_document.matches("> **Queue prompt:**").count(), 1);
    }
    #[test]
    fn done_head_consumes_despite_bundled_pending_add() {
        // #pending-add-suppresses-queue-consume: a finalize that completes the
        // queue head with --done must still consume it even when --pending-add
        // added a new backlog item in the same diff. The bundled add makes the
        // diff-based "active prompt" check return false, but the explicit --done
        // short-circuit authorizes consumption regardless.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#foo] head work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n- do [#bar]\n",
            "<!-- /agent:queue -->\n",
        );
        // Current = baseline + a bundled --pending-add backlog item (the diff
        // shape that used to suppress consumption).
        let current = baseline.replace(
            "- [ ] [#foo] head work\n",
            "- [ ] [#newitem] bundled follow-up\n- [ ] [#foo] head work\n",
        );
        std::fs::write(&doc, &current).unwrap();
        assert!(
            should_consume_queue_prompt_for_write(
                &doc,
                Some(baseline),
                &current,
                &["foo".to_string()],
            )
            .unwrap(),
            "--done naming the head must consume despite a bundled --pending-add"
        );
        // Without an explicit completion flag, the bare do[#id] head is NOT
        // consumed by the diff alone (#queue-strike-on-halt).
        assert!(
            !should_consume_queue_prompt_for_write(&doc, Some(baseline), &current, &[]).unwrap(),
            "bare do[#id] head needs an explicit completion flag"
        );
    }
    #[test]
    fn done_id_marks_later_queue_prompt_completed_without_consuming_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#opportunistic]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let marked =
            mark_completed_queue_prompts_for_done_ids(&doc, &["opportunistic".to_string()], true)
                .unwrap();
        assert_eq!(marked, 1);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("- do [#head]\n"), "{updated}");
        assert!(updated.contains("- ~~do [#opportunistic]~~\n"), "{updated}");
        assert!(updated.contains("- do [#tail]\n"), "{updated}");
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snapshot.contains("- ~~do [#opportunistic]~~\n"),
            "{snapshot}"
        );
    }

    #[test]
    fn done_id_syncs_snapshot_overlap_when_document_has_newer_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do first copy [#duplicate]\n",
            "- do newer copy [#duplicate]\n",
            "<!-- /agent:queue -->\n",
        );
        let snapshot = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do first copy [#duplicate]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let marked =
            mark_completed_queue_prompts_for_done_ids(&doc, &["duplicate".to_string()], true)
                .unwrap();
        assert_eq!(marked, 2);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(updated.matches("~~").count(), 4, "{updated}");
        let updated_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            updated_snapshot.contains("- ~~do first copy [#duplicate]~~\n"),
            "the independently matching snapshot item must still be synchronized: {updated_snapshot}"
        );
    }
    #[test]
    fn free_text_queue_head_detection() {
        // #free-text-queue-head-consume: a plain question typed into the queue
        // has no #id and is not a do-directive/preset/trigger → free text.
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Is tsift properly integrated into multi-crate architecture?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(doc).unwrap(),
            "a no-#id queue head is free text and consumable by being answered"
        );
        // A bare do[#id] head is NOT free text (needs an explicit completion flag).
        assert!(!queue_head_is_free_text_prompt(HALT_QUEUE_DOC).unwrap());
        let pinned_do = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- :pushpin: do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !queue_head_is_free_text_prompt(pinned_do).unwrap(),
            "a pinned do[#id] head is still id-backed, not free text"
        );
        // #qmisstrike: a bare `[#id]` head (no `do ` prefix) and a pin-prefixed
        // `:pushpin: [#id]` head are still id-backed. The repair free-text strike
        // guard delegates here, so these MUST classify as not-free-text or the
        // repair wrongly strikes the next open id-backed head by position.
        for head in [
            "- [#foo]\n",
            "- :pushpin: [#foo]\n",
            "- :round_pushpin: [#foo]\n",
        ] {
            let bracketed = format!(
                concat!(
                    "---\nqueue_active: true\n---\n\n",
                    "<!-- agent:queue auto -->\n",
                    "{}",
                    "<!-- /agent:queue -->\n",
                ),
                head
            );
            assert!(
                !queue_head_is_free_text_prompt(&bracketed).unwrap(),
                "a bare/pinned [#id] head {head:?} is id-backed, not free text"
            );
        }
        // A #-token head that is NOT a registered preset (no `prompt_presets`
        // frontmatter) carries an #id with no backlog row, so it stays id-backed
        // (cannot be silently struck without an explicit signal).
        let preset = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(preset).unwrap());
        // Inactive queue → no head → not free text.
        let inactive = doc.replace("queue_active: true", "queue_active: false");
        assert!(!queue_head_is_free_text_prompt(&inactive).unwrap());

        // #free-text-queue-owner-consume: a free-text head that MENTIONS ids in
        // prose (but is not a pure id directive) is still free text — it has no
        // single id to `--done`, so it must complete on being answered. This is
        // the live repro head from src/sample-app/tasks/sampleorders.md.
        let id_mentioning = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Approve [#shoptiers]. What are #next-steps?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(id_mentioning).unwrap(),
            "a free-text head that merely mentions #ids must stay free text (consumable by being answered)"
        );

        // A leading action verb + bracketed id alone (`re [#id]`) is NOT a pure
        // `#id`/`[#id]`/`do [#id]` directive, so it is treated as free text and
        // completes on answer (it still has a single mentioned id, but the verb
        // makes it prose, not a bare directive).
        let verb_prefixed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Summarize the findings for #report and ship it\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(verb_prefixed).unwrap(),
            "a prose head mentioning a single #id is still free text"
        );
    }
    /// `#qconsumenostrike` — a multiline `---`-fenced head renders without a
    /// `- ` bullet, so `markdown_ast::item_nodes` cannot address it while
    /// `parse_spans` can. Consuming it used to fall back to a positional
    /// whole-component re-render, which struck a NEIGHBOUR and silently marked
    /// unrun work complete (observed against `#c8tb` and `#orphandrain`).
    /// It must now fail closed, and must leave every other head untouched.
    #[test]
    fn multiline_unaddressable_head_fails_closed_instead_of_striking_a_neighbour() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: answered — opus\n\n",
            "> **Queue prompt:** I ran JB `Compact Exchange` but it did not compact.\n\n",
            "Handled.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "---\n",
            "I ran JB `Compact Exchange` but it did not compact.\n",
            "```\nsome fenced error output\n```\n",
            "---\n",
            "- do [#neighbour]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&file, content).unwrap();

        let result =
            consume_free_text_queue_prompts_with_outcome(&file, 1, true, &TestQueueConsumeEffects);

        assert!(
            result.is_err(),
            "an unaddressable multiline head must fail closed, got: {result:?}"
        );
        let rendered = format!("{:#}", result.unwrap_err());
        assert!(
            rendered.contains("#qconsumenostrike"),
            "error must name the defect: {rendered}"
        );

        // The critical property: nothing was struck.
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            after.contains("- do [#neighbour]"),
            "the neighbour head must remain unstruck: {after}"
        );
        assert!(
            !after.contains("~~"),
            "a refused consume must not strike anything: {after}"
        );
    }

    #[test]
    fn registered_preset_head_is_strikeable_like_free_text() {
        // #qpresetstrike: a bare queue head that is a registered `prompt_presets`
        // token has no backlog row, so `--done <id>` fails and `queue consume`
        // used to route it there and wedge the head. With the preset registered in
        // frontmatter, it is a synthetic prompt completed by being answered →
        // free text, strikeable by `queue consume` and the finalize heuristic.
        let registered = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through the review items.\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(registered).unwrap(),
            "a registered preset-token head completes on being answered (free text)"
        );
        assert!(
            agent_doc_queue::queue_response::head_id_is_registered_preset(
                registered,
                "advance-review"
            )
        );

        // A registered preset token that ALSO names a tracked backlog id stays
        // id-backed (the tracked-item reap path wins).
        let preset_and_tracked = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through the review items.\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#advance-review] tracked directive that shadows the preset name\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(
            !queue_head_is_free_text_prompt(preset_and_tracked).unwrap(),
            "a preset token that is also a tracked backlog id stays id-backed"
        );
        assert!(
            !agent_doc_queue::queue_response::head_id_is_registered_preset(
                preset_and_tracked,
                "advance-review"
            )
        );

        // An unregistered #-token (preset name not in frontmatter) is NOT treated
        // as a preset — it stays id-backed so it is never struck blind.
        let unregistered = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(unregistered).unwrap());
        assert!(
            !agent_doc_queue::queue_response::head_id_is_registered_preset(
                unregistered,
                "advance-review"
            )
        );
    }

    #[test]
    fn qmisstrike_regression_refuses_reordered_id_backed_head_without_explicit_signal() {
        // #qmisstrike-regression: a stale free-text/head-reconciliation decision may
        // arrive after the live queue has been reordered onto an id-backed head. The
        // planner itself must refuse to strike that new head unless an explicit
        // --done/--pending-gate/--pending-edit id matched it.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: old free-text head\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#staleheaddupcontent]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#staleheaddupcontent] still-open tracked work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_with_outcome(&doc).unwrap();
        assert!(
            outcome.is_none(),
            "planner must not consume an id-backed head without explicit id proof"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- do [#staleheaddupcontent]")
                && !result.contains("~do [#staleheaddupcontent]~"),
            "id-backed head must remain runnable:\n{result}"
        );
    }

    #[test]
    fn multi_head_plan_is_atomic_and_rebases_stale_snapshot_to_editor_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: batch\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- editor head one\n",
            "- editor head two\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n- [ ] [#keep] still open\n<!-- /agent:backlog -->\n",
        );
        let stale_snapshot = content.replace("editor head one", "stale snapshot head");
        std::fs::write(&doc, content).unwrap();

        let plan = plan_queue_prompt_consumption_with_snapshot_and_count(
            &doc,
            content,
            Some(&stale_snapshot),
            &[],
            8,
        )
        .unwrap()
        .expect("leading free-text prefix should plan");

        assert_eq!(
            plan.consumed_texts,
            vec!["editor head one".to_string(), "editor head two".to_string()]
        );
        assert!(plan.new_document.contains("~editor head one~"));
        assert!(plan.new_document.contains("~editor head two~"));
        assert!(plan.new_document.contains("- do [#keep]"));
        assert!(
            !plan.new_snapshot.contains("stale snapshot head"),
            "snapshot is a projection and must rebase to the editor-authoritative queue"
        );
        assert!(plan.new_snapshot.contains("~editor head one~"));
        assert!(plan.save_snapshot);
    }

    #[test]
    fn queue_consume_records_proof_ledger_before_and_after_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: Run queued thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- Run queued thing\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .unwrap()
            .expect("free-text head should be consumed");

        assert_eq!(outcome.consumed_text, "Run queued thing");
        assert_eq!(outcome.consumed_count, 1);
        assert!(outcome.drained);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- Run queued thing"),
            "drained queue head should be removed:\n{updated}"
        );

        let canonical = doc.canonicalize().unwrap();
        let ledger_path = agent_doc_workflow_io::proof_ledger::proof_ledger_path(root, &canonical);
        let records =
            agent_doc_workflow_io::proof_ledger::read_operation_proofs(&ledger_path).unwrap();
        assert_eq!(records.len(), 2, "ledger records: {records:#?}");
        assert_eq!(
            records[0].operation_kind,
            agent_doc_workflow_io::proof_ledger::ProofOperationKind::QueueHead
        );
        assert_eq!(
            records[0].outcome,
            agent_doc_workflow_io::proof_ledger::ProofOutcome::Recorded
        );
        assert_eq!(
            records[0].proof_kind,
            agent_doc_workflow_io::proof_ledger::ProofEvidenceKind::QueueHeadIdentity
        );
        assert!(records[0].proof.contains("phase=before_mutation"));
        assert!(records[0].proof.contains("Run queued thing"));
        assert_eq!(
            records[0].content_hash,
            agent_doc_hash::content_hash("Run queued thing")
        );
        assert_eq!(
            records[1].operation_kind,
            agent_doc_workflow_io::proof_ledger::ProofOperationKind::QueueHead
        );
        assert_eq!(
            records[1].outcome,
            agent_doc_workflow_io::proof_ledger::ProofOutcome::Consumed
        );
        assert_eq!(
            records[1].proof_kind,
            agent_doc_workflow_io::proof_ledger::ProofEvidenceKind::WriteResult
        );
        assert_eq!(records[0].operation_id, records[1].operation_id);
        assert!(records[1].proof.contains("phase=after_mutation"));
        assert!(records[1].proof.contains("drained=true"));

        let node_id = outcome
            .node_ops
            .first()
            .expect("consumed queue head should carry a node op")
            .node_id
            .clone();
        let state_ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(root)
                .expect("queue state events should reload from sqlite");
        let queue_events = state_ledger
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    &event.fact,
                    agent_doc_state_backbone::StateFact::QueueHeadSelected { .. }
                        | agent_doc_state_backbone::StateFact::QueueHeadCompleted { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(queue_events.len(), 2, "queue events: {queue_events:#?}");
        assert!(
            matches!(
                &queue_events[0].fact,
                agent_doc_state_backbone::StateFact::QueueHeadSelected { node_key, .. }
                    if node_key == &node_id
            ),
            "first queue state event should select the consumed head: {queue_events:#?}"
        );
        assert!(
            matches!(
                &queue_events[1].fact,
                agent_doc_state_backbone::StateFact::QueueHeadCompleted { node_key, .. }
                    if node_key == &node_id
            ),
            "second queue state event should complete the consumed head: {queue_events:#?}"
        );

        let document_hash = crate::queue_consumption_proof::queue_state_document_hash(&canonical);
        let projection = state_ledger
            .project_document(&document_hash)
            .expect("queue state events should project for document");
        assert_eq!(projection.queue.active_head, None);
        assert!(
            projection.queue.completed_heads.contains(&node_id),
            "completed head should be tracked in typed queue projection: {projection:#?}"
        );
        let head = projection
            .queue
            .heads
            .get(&node_id)
            .expect("completed head should be present in typed queue heads");
        assert_eq!(
            head.phase,
            agent_doc_state_backbone::QueueHeadPhase::Completed
        );
        assert_eq!(head.backlog_id, None);
        assert_eq!(head.prompt_text.as_deref(), Some("Run queued thing"));
    }

    #[test]
    fn queue_consume_selects_next_remaining_head_in_typed_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: First queued thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- First queued thing\n",
            "- do [#nextitem]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#nextitem] next item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .unwrap()
            .expect("first free-text head should be consumed");

        assert_eq!(outcome.consumed_text, "First queued thing");
        assert_eq!(outcome.remaining, 1);
        let old_node = outcome.node_ops[0].node_id.clone();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- First queued thing") && updated.contains("- do [#nextitem]"),
            "only the first head should be consumed:\n{updated}"
        );

        let next_node = queue_prompt_node_keys_for_count(&updated, 1)
            .unwrap()
            .keys
            .into_iter()
            .next()
            .expect("remaining head should have a node key");
        let document_hash =
            crate::queue_consumption_proof::queue_state_document_hash(&doc.canonicalize().unwrap());
        let state_ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(root)
                .expect("queue state events should reload from sqlite");
        let projection = state_ledger
            .project_document(&document_hash)
            .expect("queue state events should project for document");
        assert!(
            projection.queue.completed_heads.contains(&old_node),
            "consumed head should be completed in typed projection: {projection:#?}"
        );
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(next_node.as_str())
        );
        let next = projection
            .queue
            .heads
            .get(&next_node)
            .expect("remaining head should be selected in typed projection");
        assert_eq!(
            next.phase,
            agent_doc_state_backbone::QueueHeadPhase::Selected
        );
        assert_eq!(next.backlog_id.as_deref(), Some("nextitem"));
        assert_eq!(next.prompt_text.as_deref(), Some("do [#nextitem]"));
        assert!(next.drainable);
    }

    #[test]
    fn queue_consume_records_stop_fence_next_head_as_deferred_in_typed_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: First queued thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- First queued thing\n",
            "--- stop\n",
            "- do [#nextitem]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#nextitem] next item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .unwrap()
            .expect("first head should be consumed");

        assert_eq!(outcome.consumed_text, "First queued thing");
        assert_eq!(outcome.remaining, 1);
        let updated = std::fs::read_to_string(&doc).unwrap();
        let next_node = queue_prompt_node_keys_for_count(&updated, 1)
            .unwrap()
            .keys
            .into_iter()
            .next()
            .expect("remaining head should have a node key");
        let document_hash =
            crate::queue_consumption_proof::queue_state_document_hash(&doc.canonicalize().unwrap());
        let state_ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(root)
                .expect("queue state events should reload from sqlite");
        let projection = state_ledger
            .project_document(&document_hash)
            .expect("queue state events should project for document");
        assert_eq!(projection.queue.active_head, None);
        let next = projection
            .queue
            .heads
            .get(&next_node)
            .expect("remaining head should be tracked in typed projection");
        assert_eq!(
            next.phase,
            agent_doc_state_backbone::QueueHeadPhase::Deferred
        );
        assert_eq!(next.defer_reason.as_deref(), Some("stop_fence"));
        assert_eq!(next.prompt_text.as_deref(), Some("do [#nextitem]"));
        assert!(!next.drainable);
    }

    #[test]
    fn queue_consume_reconciles_diverged_snapshot_instead_of_bailing() {
        // #finalize-divergence-orphans-committed-head / IPC-CRDT resilience: when
        // the post-merge document queue diverges from the snapshot queue (a
        // concurrent user/editor edit the CRDT merge already reconciled), consume
        // must RECONCILE (the merged document wins) and strike the head — not
        // hard-bail and orphan the unstruck head. Regression for the divergence
        // error hit repeatedly under live editor races.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "- user added later\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // Snapshot diverges: same head, but missing the concurrently-added item.
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "<!-- /agent:queue -->\n",
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snap,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("consume must not bail on a reconcilable divergence");
        assert!(outcome.is_some(), "the answered head should be consumed");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~~do the thing~~"),
            "head must be struck after reconcile:\n{result}"
        );
        assert!(
            result.contains("- user added later"),
            "the concurrently-added item must be preserved (document wins):\n{result}"
        );
        let snap_result = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap_result.contains("- user added later"),
            "snapshot must adopt the reconciled document queue:\n{snap_result}"
        );
    }
    #[test]
    fn queue_consume_head_divergence_consumes_document_head_despite_dropped_queue_evidence() {
        // Snapshot sidecars are recovery-only. If the document head diverges from
        // the snapshot head, the visible document remains authoritative even when
        // older dropped-queue evidence exists.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the new live-buffer request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- handle the new live-buffer request\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // Snapshot carries the OLD head (the live user addition was not absorbed).
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the old request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- handle the old request\n",
            "<!-- /agent:queue -->\n",
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snap,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Record the live-buffer drift evidence for the document head.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snap), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_dropped_queue_prompts(
            &doc,
            &["handle the new live-buffer request".to_string()],
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("snapshot divergence must not hard-bail")
            .expect("document head should be consumed");
        assert_eq!(outcome.consumed_text, "handle the new live-buffer request");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !result.contains("- handle the new live-buffer request"),
            "the document queue head must be consumed:\n{result}"
        );
        let snap_result = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snap_result.contains("- handle the old request")
                && snap_result.contains("handle the new live-buffer request"),
            "the stale snapshot queue must rebase to the editor-authoritative consume result:\n{snap_result}"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("snapshot_recovery_warning")
                && ops_log.contains("snapshot head prompts"),
            "the snapshot rebase must be logged for forensics:\n{ops_log}"
        );
    }

    #[test]
    fn queue_consume_uses_document_head_when_snapshot_diverges() {
        // The visible document head is the turn target. A stale snapshot may warn
        // and skip sidecar sync, but it must not retarget consumption.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the old request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- test\n",
            "- handle the old request\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the old request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- handle the old request\n",
            "<!-- /agent:queue -->\n",
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snap,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(snap), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_dropped_queue_prompts(&doc, &["test".to_string()])
            .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("consume must not hard-bail on snapshot divergence")
            .expect("document head should be consumed");
        assert_eq!(outcome.consumed_text, "test");

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~~test~~"),
            "document head must be consumed in place:\n{result}"
        );
        assert!(
            result.contains("- handle the old request"),
            "following document queue item must stay live:\n{result}"
        );
        let snap_result = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap_result.contains("- handle the old request") && !snap_result.contains("- test"),
            "diverged recovery snapshot must not retarget the hot path:\n{snap_result}"
        );
    }

    #[test]
    fn queue_consume_head_divergence_without_evidence_warns_and_consumes_document_head() {
        // Snapshot sidecars are not a hot-path source of truth. Even without
        // dropped-queue evidence, a stale snapshot mismatch warns and the document
        // head is consumed.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the new live-buffer request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- handle the new live-buffer request\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: handle the old request\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- handle the old request\n",
            "<!-- /agent:queue -->\n",
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snap,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // No cycle_state dropped-queue evidence recorded.

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("snapshot divergence must not hard-bail")
            .expect("document head should be consumed");
        assert_eq!(outcome.consumed_text, "handle the new live-buffer request");
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("snapshot_recovery_warning")
                && ops_log.contains("snapshot head prompts"),
            "snapshot mismatch must be logged for recovery diagnostics:\n{ops_log}"
        );
    }

    #[test]
    fn strike_orphan_id_backed_queue_head_writes_detached_disk_without_listener() {
        // #orphanqhead: an id-backed head whose backing backlog item was reaped
        // (absent from agent:backlog) is undrainable — `queue consume` rejects it
        // and `--done` is a no-op. With no editor listener or live sidecar, the
        // escape hatch writes the guarded detached disk replica.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#orphangone]\n",
            "- do [#liveone]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#liveone] still open and drainable\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let keys = id_backed_head_node_keys(content, "orphangone").unwrap();
        assert_eq!(keys.len(), 1, "the orphaned head must be targetable");
        assert!(strike_orphan_id_backed_queue_head(&doc, "orphangone").unwrap());
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("~~do [#orphangone]~~"),
            "detached disk strike should mark the orphaned head:\n{result}"
        );
        assert!(
            result.contains("- do [#liveone]\n"),
            "drainable open id head must remain queued:\n{result}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, result, "detached disk strike must update snapshot");
    }
    #[test]
    fn strike_orphan_id_backed_queue_head_refuses_open_backlog_item() {
        // The escape hatch must NOT desync live work: an id still naming an OPEN
        // backlog item has a real `--done` drain path and must be refused.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#stillopen] genuine open work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = strike_orphan_id_backed_queue_head(&doc, "stillopen").unwrap_err();
        assert!(
            err.to_string().contains("OPEN backlog item"),
            "must refuse to strike an open backlog id: {err}"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- do [#stillopen]") && !result.contains("~~do [#stillopen]~~"),
            "open backlog head must remain runnable:\n{result}"
        );
    }
    #[test]
    fn queue_consume_uses_node_keys_to_preserve_duplicate_prompt_identity() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: duplicate prose\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- duplicate prose\n",
            "- duplicate prose\n",
            "- keep\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("node-keyed queue consume should handle duplicates")
            .expect("the answered duplicate head should be consumed");

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~~duplicate prose~~\n- duplicate prose\n- keep\n"),
            "only the first duplicate prompt should be struck:\n{result}"
        );
        assert_eq!(outcome.consumed_count, 1);
        assert_eq!(outcome.node_ops.len(), 1);
        assert_eq!(outcome.node_ops[0].component, "queue");
        assert_eq!(outcome.node_ops[0].op, "consume");
        assert!(
            outcome.node_ops[0].node_id.starts_with("queue:")
                && outcome.node_ops[0].node_id.contains(":ft-"),
            "node op should carry the queue node key, got {:?}",
            outcome.node_ops[0]
        );
        assert_eq!(
            outcome.node_ops[0].to_json()["op"].as_str(),
            Some("consume")
        );
    }
    #[test]
    fn consume_decision_strikes_answered_free_text_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let head =
            "JB `Run Agent Doc` on a `queue: stop` + `agent:queue go` doc should start the queue.";
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: JB Run Agent Doc should start the queue\n\nFixed in route.rs.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- JB `Run Agent Doc` on a `queue: stop` + `agent:queue go` doc should start the queue.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let response = format!(
            "### Re: JB Run Agent Doc should start the queue\n\n> **Queue prompt:** {head}\n\nFixed."
        );
        // baseline == current (no new exchange prompt this cycle), and the
        // response quotes this exact free-text head, so it may be consumed.
        assert!(
            queue_consumption_allowed_for_response(&doc, Some(content), content, &response, &[],)
                .unwrap(),
            "an answered free-text head must be consumed on successful closeout"
        );
    }

    #[test]
    fn consume_decision_keeps_free_text_head_without_exact_response_proof() {
        // #qstrikework: a generic repair/recovery response must not consume the
        // current free-text queue head merely because the response body is non-empty.
        // The response has to quote/target this exact head in the same way the
        // free-text strike pass proves answered heads.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier repair prompt\n\nNarrowed the repair path.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- Queue items are being struck without being worked on.\n",
            "- do [#operatorverify]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();

        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: earlier repair prompt\n\nRepaired the interrupted closeout state.",
                &[],
            )
            .unwrap(),
            "a free-text head must stay queued when this response never quotes it"
        );
    }
    // ---- #ftstrike: position-independent answered free-text head strike ----

    const FTSTRIKE_RESPONSE: &str = concat!(
        "### Re: two reports — opus\n\n",
        "> **Queue prompts:**\n",
        "> - JB `Run Agent Doc` is stalled on this document when I tried to start the queue run. No notification.\n",
        "> - My free-text queue items are not immediately struck as if they are addressed.\n\n",
        "Triaged both.\n",
    );

    #[test]
    fn answered_free_text_heads_selected_behind_id_backed_head() {
        // The exact regression: two free-text reports sit BEHIND an unfinished
        // `do [#fullboundary]` id head. The response answers both. The selector must
        // return the two free-text node keys and NOT the id-backed head.
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fullboundary]\n",
            "- :pushpin: JB `Run Agent Doc` is stalled on this document when I tried to start the queue run. No notification.\n",
            "- :pushpin: My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        let keys = answered_free_text_head_node_keys(content, FTSTRIKE_RESPONSE, None).unwrap();
        assert_eq!(
            keys.len(),
            2,
            "both answered free-text heads behind the id head must be selected: {keys:?}"
        );
        // The id-backed `do [#fullboundary]` head must never be selected by this pass.
        assert!(
            !keys.iter().any(|k| k.contains("fullboundary")),
            "id-backed head must not be struck by the free-text pass: {keys:?}"
        );
    }

    #[test]
    fn answered_free_text_head_not_struck_when_absent_from_baseline() {
        // #qstrikeexplain Phase 2: the operator is TYPING a new queue line this turn.
        // It is answered by the response (it fuzzy-matches a quoted prompt) but is
        // NOT present in the stable pre-turn baseline, so it must NOT be struck —
        // it defers to the cycle that actually answers it.
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- :pushpin: My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        // Baseline (pre-turn) does NOT contain that head — it first appeared this turn.
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- some entirely different earlier queue line about another topic\n",
            "<!-- /agent:queue -->\n",
        );
        // Without the gate the head IS selected (answered).
        let ungated = answered_free_text_head_node_keys(content, FTSTRIKE_RESPONSE, None).unwrap();
        assert_eq!(ungated.len(), 1, "answered head selected without the gate");
        // With the baseline gate the in-flight head is deferred (not struck).
        let gated =
            answered_free_text_head_node_keys(content, FTSTRIKE_RESPONSE, Some(baseline)).unwrap();
        assert!(
            gated.is_empty(),
            "a head absent from the pre-turn baseline must not be struck: {gated:?}"
        );
    }

    #[test]
    fn answered_free_text_head_struck_when_present_in_baseline() {
        // #qstrikeexplain Phase 2: a stable head the operator authored in a PRIOR
        // turn (present in the baseline) and answered this turn is still struck —
        // the gate only defers brand-new in-flight heads, never legitimate ones.
        let head = "- :pushpin: My free-text queue items are not immediately struck as if they are addressed.\n";
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
        );
        let content = format!("{content}{head}<!-- /agent:queue -->\n");
        // Baseline contains the SAME head (cosmetic differences must not matter).
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        let gated =
            answered_free_text_head_node_keys(&content, FTSTRIKE_RESPONSE, Some(baseline)).unwrap();
        assert_eq!(
            gated.len(),
            1,
            "a baseline-present answered head must still be struck: {gated:?}"
        );
    }

    #[test]
    fn marker_head_struck_without_blockquote_quote_qheadstrikeauto() {
        // #qheadstrikeauto: the cycle's drain target carries the in-progress `🚧`
        // marker (stamped by preflight `set_first_prompt_in_progress`). A committed
        // response that answers it in PLAIN PROSE — never quoting it as a
        // `> **Queue prompt:**` blockquote — must still strike it. The strike is
        // keyed off the binary's drain-target marker identity, not agent prose
        // (operator: "the binary should do this automatically...not the agent").
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        // Plain-prose response with NO blockquote echo of the head: the legacy
        // prose/blockquote answer-match returns false for this head...
        let prose_only = "### Re: fixed it — opus\n\nI addressed the queue-strike behavior and shipped the fix.\n";
        assert!(
            !agent_doc_queue::queue_response::free_text_head_answered_by_response(
                prose_only,
                "🚧 My free-text queue items are not immediately struck as if they are addressed."
            ),
            "precondition: a plain-prose response must NOT prose-match the head"
        );
        // ...but the drain-target marker path strikes it on a committed response.
        let keys = answered_free_text_head_node_keys(content, prose_only, None).unwrap();
        assert_eq!(
            keys.len(),
            1,
            "the 🚧 drain-target marker head must be struck on a committed response: {keys:?}"
        );
    }

    #[test]
    fn marker_head_not_struck_when_absent_from_baseline_qheadstrikeauto() {
        // The marker path still respects the #qstrikeexplain Phase 2 baseline gate:
        // a 🚧 head that first appeared this turn (an in-flight operator edit) is
        // deferred, never same-cycle struck.
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- some entirely different earlier queue line about another topic\n",
            "<!-- /agent:queue -->\n",
        );
        let prose_only = "### Re: fixed it — opus\n\nDone.\n";
        let gated = answered_free_text_head_node_keys(content, prose_only, Some(baseline)).unwrap();
        assert!(
            gated.is_empty(),
            "a 🚧 marker head absent from the pre-turn baseline must not be struck: {gated:?}"
        );
    }

    #[test]
    fn id_backed_marker_head_not_struck_by_free_text_pass_qheadstrikeauto() {
        // P4: an id-backed head carrying the 🚧 marker is reaped via `--done`, never
        // by the free-text auto-strike — even though it is the drain target. The
        // marker path only ever strikes FREE-TEXT marker heads.
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 do [#fullboundary]\n",
            "<!-- /agent:queue -->\n",
        );
        let prose_only = "### Re: did the work — opus\n\nImplemented #fullboundary.\n";
        let keys = answered_free_text_head_node_keys(content, prose_only, None).unwrap();
        assert!(
            keys.is_empty(),
            "an id-backed 🚧 marker head must not be struck by the free-text pass: {keys:?}"
        );
    }

    #[test]
    fn queue_prompt_text_is_free_text_classification() {
        let content =
            "---\nqueue_active: true\n---\n<!-- agent:queue -->\n- x\n<!-- /agent:queue -->\n";
        assert!(!queue_prompt_text_is_free_text(
            content,
            "do [#fullboundary]"
        ));
        assert!(!queue_prompt_text_is_free_text(content, "#orphanqhead"));
        assert!(queue_prompt_text_is_free_text(
            content,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
    }

    #[test]
    fn free_text_strike_resolves_editor_content_before_document_lock() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        std::fs::write(&doc, "---\nqueue_active: false\n---\n").unwrap();
        let effects = TrackingStrikeEffects::default();

        let struck = super::strike_answered_free_text_queue_heads(
            &doc,
            "### Re: answer\n\nDone.\n",
            false,
            &effects,
        )
        .unwrap();

        assert_eq!(struck, 0);
        assert_eq!(effects.current_reads.load(Ordering::SeqCst), 1);
        assert_eq!(effects.force_disk_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn commit_seam_free_text_strike_never_reads_controller_content() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        std::fs::write(&doc, "---\nqueue_active: false\n---\n").unwrap();
        let effects = TrackingStrikeEffects::default();

        let struck = super::strike_answered_free_text_queue_heads(
            &doc,
            "### Re: answer\n\nDone.\n",
            true,
            &effects,
        )
        .unwrap();

        assert_eq!(struck, 0);
        assert_eq!(effects.current_reads.load(Ordering::SeqCst), 0);
        assert_eq!(effects.force_disk_reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn strike_answered_free_text_heads_strikes_behind_id_head_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n",
                "- do [#fullboundary]\n",
                "- :pushpin: JB `Run Agent Doc` is stalled on this document when I tried to start the queue run. No notification.\n",
                "- :pushpin: My free-text queue items are not immediately struck as if they are addressed.\n",
                "<!-- /agent:queue -->\n",
            ),
            FTSTRIKE_RESPONSE,
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let struck = strike_answered_free_text_queue_heads(&doc, FTSTRIKE_RESPONSE, true).unwrap();
        assert_eq!(struck, 2, "both answered free-text heads must be struck");

        let updated = std::fs::read_to_string(&doc).unwrap();
        // The id head stays runnable (not struck); the two free-text heads are struck.
        assert!(
            updated.contains("- do [#fullboundary]\n"),
            "id-backed head must remain unstruck:\n{updated}"
        );
        assert!(
            updated.matches("~~").count() >= 4,
            "two heads struck => four ~~ markers:\n{updated}"
        );
        // `#qstrikenote`: each struck free-text head carries the deterministic
        // auto-struck explanation, on the queue line itself — NOT in exchange.
        assert_eq!(
            updated
                .matches("— auto-struck: answered this cycle (#ftstrike)")
                .count(),
            2,
            "both struck heads must carry exactly one auto-struck note:\n{updated}"
        );
        // The note lives outside the strike wrapper, after the closing `~~`.
        assert!(
            updated.contains("~~ — auto-struck: answered this cycle (#ftstrike)"),
            "note must sit outside the ~~…~~ wrapper:\n{updated}"
        );
        // Idempotent: a second pass strikes nothing more and adds no second note.
        let again = strike_answered_free_text_queue_heads(&doc, FTSTRIKE_RESPONSE, true).unwrap();
        assert_eq!(again, 0, "already-struck heads must not be re-struck");
        let after_again = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            after_again
                .matches("— auto-struck: answered this cycle (#ftstrike)")
                .count(),
            2,
            "the note must not be duplicated on re-strike:\n{after_again}"
        );
        // Zero-drift: nothing was written into an exchange component.
        let exchange = element::parse(&after_again)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap();
        assert!(
            !exchange.content(&after_again).contains("auto-struck"),
            "the auto-strike note must never target exchange:\n{after_again}"
        );
    }

    #[test]
    fn answered_free_text_head_waits_until_response_is_materialized() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "The response was printed to the console but never landed here.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();

        let struck = strike_answered_free_text_queue_heads(&doc, FTSTRIKE_RESPONSE, true).unwrap();

        assert_eq!(struck, 0);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn consume_decision_strikes_synthetic_preset_head_on_heading_match() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #spec-test-build-install-commit-push\n\nDone.",
                &[],
            )
            .unwrap(),
            "a preset head answered by a matching heading id must be consumed"
        );
    }
    #[test]
    fn consume_decision_keeps_bare_do_id_head_without_explicit_flag() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // A bare do[#id] head is halt-safe: a response that does not record an
        // explicit --done/--gate/--edit outcome must NOT strike it.
        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: not doing this, here is why",
                &[],
            )
            .unwrap(),
            "a bare do[#id] head must stay queued without an explicit completion flag"
        );
        // The same head WITH --done foo is consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: do [#foo]\n\nDone.",
                &["foo".to_string()],
            )
            .unwrap(),
            "--done naming the head id must consume it"
        );
        // Resolving a tracked review item is also explicit proof for an id-backed
        // queue head that names the same id.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: do [#foo]\n\nResolved the review item.",
                &["foo".to_string()],
            )
            .unwrap(),
            "--review-resolve naming the head id must consume it"
        );
    }
    #[test]
    fn consume_decision_keeps_operator_pinned_tracked_backlog_head_without_explicit_flag() {
        // #zwn5: an operator-pinned bare id head (`:round_pushpin: [#ktw8]`) whose
        // id names a tracked agent:backlog item is an id-backed directive — e.g. an
        // operator-drive live-verify item the agent answers with a log-check but can
        // never close itself. A `### Re: #ktw8` log-check heading must NOT strike it
        // (the old synthetic/preset heading-id path wrongly consumed it, then
        // session-check dropped the struck head and locked the snapshot).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ktw8] operator live-verify: destructive /clear path, operator drives.\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- :round_pushpin: [#ktw8]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #ktw8 — destructive /clear live-verify (operator-drive log check)\n\nops.log shows 0 markers; stays open.",
                &[],
            )
            .unwrap(),
            "an operator-pinned head naming a tracked backlog item must stay queued without an explicit completion flag"
        );
        // The same head WITH --pending-gate naming its id is a real completion signal.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #ktw8\n\nGated pending live verification.",
                &["ktw8".to_string()],
            )
            .unwrap(),
            "--pending-gate naming the head id must consume it"
        );
    }
    #[test]
    fn free_text_head_kept_only_when_cycle_answered_foreign_prompt() {
        // #queue-head-struck-on-foreign-exchange-answer: the predicate that gates
        // free-text head consumption. A drain cycle (only this turn's `### Re:`
        // response added, no new user prompt) is NOT foreign → head drains. A
        // cycle that added a NEW unrelated `❯` exchange prompt IS foreign → the
        // free-text head stays queued so its work is not silently struck.
        let head = "lazily-rs plan-update";
        let baseline = "\
---
agent_doc_format: template
queue_active: true
---

<!-- agent:exchange -->
### Re: older
Old.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- lazily-rs plan-update
<!-- /agent:queue -->
";
        let drain = baseline.replace(
            "<!-- agent:boundary:x -->",
            "### Re: updated the plan\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), &drain, head),
            "a drain cycle (only a new response, no new prompt) is not foreign work"
        );

        let foreign = baseline.replace(
            "<!-- agent:boundary:x -->",
            "❯ Fix the JB cache conflict instead\n### Re: fix jb\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), &foreign, head),
            "a cycle that added a new unrelated exchange prompt answered foreign work"
        );
    }
    #[test]
    fn bare_do_directive_detection() {
        // Queue parser strips the `- ` bullet, so heads arrive as `do [#id]`.
        assert!(queue_head_is_bare_do_directive("do [#foo]"));
        assert!(queue_head_is_bare_do_directive("do #foo"));
        assert!(queue_head_is_bare_do_directive(":pushpin: do [#foo]"));
        assert!(queue_head_is_bare_do_directive(":round_pushpin: do #foo"));
        // A synthetic/preset prompt carrying a trailing `#preset` id is NOT a
        // bare directive.
        assert!(!queue_head_is_bare_do_directive(
            "JB Run Agent Doc on tsift.md add the prompt into agent:queue.\n#spec-test-build-install-commit-push"
        ));
        // A bare preset id on its own line is also not a `do` directive.
        assert!(!queue_head_is_bare_do_directive(
            "#spec-test-build-install-commit-push"
        ));
    }
    #[test]
    fn multi_id_directive_head_is_id_backed_not_free_text() {
        // End-to-end through both public classifiers. No `prompt_presets`
        // frontmatter, so these ids have a `--done` reap path (not presets).
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#syncbarrier] [#crdtsvdom]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !queue_head_is_free_text_prompt(content).unwrap(),
            "multi-id `do [#a] [#b]` head must be id-backed (not strikeable by position)"
        );
        assert!(!queue_prompt_text_is_free_text(
            content,
            "do [#syncbarrier] [#crdtsvdom]"
        ));
        // Single-id head stays id-backed (unchanged behavior).
        assert!(!queue_prompt_text_is_free_text(content, "do [#qeditdup]"));
        // A head mixing an id with free-text prose stays free text.
        assert!(queue_prompt_text_is_free_text(
            content,
            "Approve [#shoptiers] then ship it"
        ));
    }

    #[test]
    fn prune_noise_plans_interleaved_noise_and_writes_detached_disk_without_listener() {
        // #goqstall2: `queue prune-noise` strikes non-drainable NOISE at ANY
        // position — including noise interleaved BEHIND id-backed `do [#id]` heads,
        // which the leading-run `queue consume` stops at and can never reach — while
        // preserving id-backed directives and genuinely drainable free-text/prose heads.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- [route] target tmux session: 0\n", // artifact noise
            "- do [#keepme]\n",                   // id-backed -> preserved
            "- [error] dispatch blocked by stale pane\n", // noise BEHIND an id head
            "- fix the tokenizer now\n",          // `fix` verb -> drainable
            "- Queue items are being struck without being worked on.\n", // prose -> preserved
            "- do [#keepme2]\n",                  // id-backed -> preserved
            "- [warning] stale queue marker\n",   // noise (tail)
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let (planned, struck) = strike_all_noise_queue_heads(content).unwrap();
        assert_eq!(
            struck, 3,
            "exactly the 3 artifact noise heads must be struck"
        );

        // id-backed directives preserved (incl. the one with noise behind it).
        assert!(
            planned.contains("- do [#keepme]\n") && !planned.contains("~~do [#keepme]~~"),
            "id-backed head must be preserved:\n{planned}"
        );
        assert!(
            planned.contains("- do [#keepme2]\n"),
            "interleaved id-backed head must be preserved:\n{planned}"
        );
        // Drainable free-text directive preserved.
        assert!(
            planned.contains("fix the tokenizer now") && !planned.contains("~~fix the tokenizer"),
            "drainable directive head must be preserved:\n{planned}"
        );
        assert!(
            planned.contains("Queue items are being struck without being worked on.")
                && !planned.contains("~~Queue items are being struck"),
            "operator prose report must be preserved:\n{planned}"
        );
        // All three artifact noise heads struck — including the one BEHIND `do [#keepme]`,
        // which a leading-run consume could never reach.
        assert!(
            planned.contains("~~[route] target tmux session: 0~~"),
            "leading noise struck:\n{planned}"
        );
        assert!(
            planned.contains("~~[error] dispatch blocked by stale pane~~"),
            "noise interleaved behind an id head struck:\n{planned}"
        );
        assert!(
            planned.contains("~~[warning] stale queue marker~~"),
            "tail noise struck:\n{planned}"
        );

        assert_eq!(prune_noise_queue_heads(&doc).unwrap(), 3);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, planned, "detached disk prune should apply plan");
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, planned, "detached disk prune should update snapshot");
    }

    #[test]
    fn prune_noise_plans_orphan_id_heads_and_writes_detached_disk_without_listener() {
        // #orphanqhead / #qchurn: `queue prune-noise` strikes an orphan id-backed
        // head (id absent from the open backlog) — which `queue consume` rejects and
        // `--done` cannot reap — so it stops blocking the leading-run consume and
        // stops churning the go-mode loop. Open backlog ids, including deferred
        // `[operator-verify]` / `[focused-cycle]` items, are preserved.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- :pushpin: [#kcb5]\n",    // ORPHAN (no backlog item) → struck
            "- :pushpin: do [#6b5h]\n", // deferred [focused-cycle] but OPEN → preserved
            "- do [#keepme]\n",         // open backlog → preserved
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] real open work\n",
            "- [ ] [#6b5h] [focused-cycle] dedicated cycle work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let (planned, struck) = strike_all_noise_queue_heads(content).unwrap();
        assert_eq!(struck, 1, "only the orphan #kcb5 head must be struck");

        assert!(
            planned.contains("~~:pushpin: [#kcb5]~~") || planned.contains("~~[#kcb5]~~"),
            "orphan id head must be struck:\n{planned}"
        );
        assert!(
            planned.contains("- :pushpin: do [#6b5h]\n"),
            "deferred-but-open backlog id head must be preserved:\n{planned}"
        );
        assert!(
            planned.contains("- do [#keepme]\n"),
            "open backlog id head must be preserved:\n{planned}"
        );

        assert_eq!(prune_noise_queue_heads(&doc).unwrap(), 1);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, planned, "detached disk prune should apply plan");
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, planned, "detached disk prune should update snapshot");
    }

    #[test]
    fn acknowledge_open_id_head_writes_detached_disk_without_listener() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- [#freshqueueauth]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#freshqueueauth] preserve fresh operator queue heads\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let keys = id_backed_head_node_keys(content, "freshqueueauth").unwrap();
        assert_eq!(
            keys.len(),
            1,
            "exact id-backed correction head should be targetable"
        );
        assert!(acknowledge_open_id_backed_queue_head(&doc, "freshqueueauth").unwrap());

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_ne!(
            result, content,
            "detached disk ack must mutate the queue head"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .expect("snapshot saved");
        assert_ne!(snap, content, "detached disk ack must mutate snapshot");
        assert!(
            result.contains("- [ ] [#freshqueueauth] preserve fresh operator queue heads"),
            "underlying backlog item must remain open:\n{result}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("open_id_head_ack_writeback")
                && ops_log.contains("transport=disk_detached"),
            "detached acknowledgement writeback must be auditable:\n{ops_log}"
        );
    }

    #[test]
    fn acknowledge_open_id_head_refuses_prose_that_mentions_open_id() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- Please keep [#freshqueueauth] open until the implementation lands\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#freshqueueauth] preserve fresh operator queue heads\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = acknowledge_open_id_backed_queue_head(&doc, "freshqueueauth")
            .expect_err("prose correction heads stay on the free-text consume path");
        assert!(
            err.to_string()
                .contains("prose that merely mentions the id"),
            "error should route prose heads to answer+consume guidance: {err}"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("Please keep [#freshqueueauth] open"),
            "prose queue head must be left intact:\n{result}"
        );
    }

    #[test]
    fn prune_noise_preserves_id_heads_when_no_backlog_component() {
        // A free-form id-head queue (no `agent:backlog`) treats id-heads AS the work:
        // the orphan prune must NOT fire, so nothing is struck (#orphanqhead gate).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(
            prune_noise_queue_heads(&doc).unwrap(),
            0,
            "no backlog component → id-heads are the work → nothing pruned"
        );
    }

    #[test]
    fn prune_noise_plans_all_log_multiline_blocks_and_writes_detached_disk_without_listener() {
        // #qnoise-multiline-strike: operator-pasted console dumps
        // land in the queue as multiline `---`-fenced Prompt blocks. They are NOT
        // bulleted list items and contain a ``` fence, so the bullet-only
        // `item_nodes` strike path never enumerated them — `queue prune-noise`
        // reported "nothing to prune" while the flood persisted on disk forever.
        // They must still be excised by byte range. A `preset` attr makes prose
        // reports drainable even when followed by fenced diagnostics, so prune
        // must preserve those operator prompts and delete only all-log blocks.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" go -->\n",
            "- [#sqedit-race]\n", // id-backed → preserved
            "- [#keepme]\n",      // id-backed → preserved
            // Shape 1: all-log `---`-wrapped block whose text contains a nested
            // ``` fence and no prose lead.
            "---\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: dispatch blocked: only the gated #5eq8 remains.\n",
            "```\n",
            "---\n",
            // Shape 2: prose report with diagnostic evidence. Under a preset this
            // is real drainable work and must not be pruned.
            "---\n",
            ":pushpin: JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The actor record did not switch.\n",
            "```\n",
            "Error: authoritative actor record is bound to harness claude-code, not codex\n",
            "```\n",
            "---\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let (planned, struck) = strike_all_noise_queue_heads(content).unwrap();
        assert_eq!(struck, 1, "only the all-log pasted block must be excised");

        assert!(
            planned.contains("- [#sqedit-race]\n") && planned.contains("- [#keepme]\n"),
            "id-backed heads must survive:\n{planned}"
        );
        assert!(
            !planned.contains("[route] target tmux session: 0") && !planned.contains("#5eq8"),
            "the all-log block must be gone:\n{planned}"
        );
        assert!(
            planned.contains("JB `Run Agent Doc` on agent-loop.md")
                && planned.contains("bound to harness claude-code, not codex"),
            "the prose diagnostic report must be preserved as drainable work:\n{planned}"
        );

        assert_eq!(prune_noise_queue_heads(&doc).unwrap(), 1);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, planned, "detached disk prune should apply plan");
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, planned, "detached disk prune should update snapshot");
    }
}
